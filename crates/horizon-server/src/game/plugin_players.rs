//! Where the server fires the plugins' players events (docs/plugins.md):
//! what changed about each player from one tick to the next (health, keys,
//! client options, recipe books, armor and slots, respawns, jumps), their
//! plugin channels, the server's own events (plugins, ticks, reloads, the
//! whitelist, unknown commands, the clock), and the hooks in the gameplay
//! code (pickups, buckets, respawn points, bows, throws, flower pots, picks).
use super::Dim;
use super::respawn_anchor::RespawnConfig;
use super::server_net::Network;
use crate::hc::items::ItemStack;
use crate::hc::server_net::player::ServerPlayer;
use crate::hc::text::Text;
use horizon_protocol::packets::{
    configuration::{ClientInformation, ServerLink, ServerLinkKind, ServerLinks},
    play::{Clientbound as C, Serverbound as S},
};
use horizon_server_api::{self as api, CommandSender, Location, event as ev};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// What the plugins last saw of a player.
#[derive(Clone)]
struct Seen {
    health: f32,
    dead: bool,
    input: u8,
    info: ClientInformation,
    book: ([bool; 4], [bool; 4]),
    /// Kept only while someone listens for `PlayerArmorChangeEvent`
    /// (`has_armor`).
    armor: [Option<ItemStack>; 4],
    has_armor: bool,
    /// The 36 slots, the armor (head..feet) and the off hand, kept only while
    /// someone listens for `PlayerInventorySlotChangeEvent`.
    slots: Vec<Option<ItemStack>>,
}
impl Seen {
    fn of(g: &ServerPlayer, armor: bool, slots: bool) -> Self {
        Self {
            health: g.health,
            dead: g.dead,
            input: g.input,
            info: g.info.clone(),
            book: (g.recipe_book.open, g.recipe_book.filtering),
            armor: if armor {
                g.armor.clone()
            } else {
                Default::default()
            },
            has_armor: armor,
            slots: if slots {
                let mut s = g.inventory.clone();
                s.extend(g.armor.iter().cloned());
                s.push(g.offhand.clone());
                s
            } else {
                Vec::new()
            },
        }
    }
}

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    players: HashMap<u32, Seen>,
    /// Each player's registered plugin channels.
    channels: HashMap<u32, HashSet<String>>,
    /// `MinecraftServer.tickCount` as the tick events count it.
    tick: u64,
}

const ARMOR: [ev::ArmorSlot; 4] = [
    ev::ArmorSlot::Head,
    ev::ArmorSlot::Chest,
    ev::ArmorSlot::Legs,
    ev::ArmorSlot::Feet,
];
const BOOKS: [ev::RecipeBookType; 4] = [
    ev::RecipeBookType::Crafting,
    ev::RecipeBookType::Furnace,
    ev::RecipeBookType::BlastFurnace,
    ev::RecipeBookType::Smoker,
];

fn stack_ref(s: &ItemStack) -> api::ItemStack {
    super::plugin_events::stack_ref(s)
}

fn options_of(i: &ClientInformation) -> ev::ClientOptions {
    ev::ClientOptions {
        locale: i.language.clone(),
        view_distance: i32::from(i.view_distance),
        chat_visibility: match i.chat_visibility {
            1 => ev::ChatVisibility::System,
            2 => ev::ChatVisibility::Hidden,
            _ => ev::ChatVisibility::Full,
        },
        chat_colors: i.chat_colors,
        skin_parts: i.model_customisation,
        main_hand: if i.main_hand == 0 {
            ev::MainHand::Left
        } else {
            ev::MainHand::Right
        },
        text_filtering: i.text_filtering,
        allows_server_listings: i.allows_listing,
        particle_visibility: match i.particle_status {
            1 => ev::ParticleVisibility::Decreased,
            2 => ev::ParticleVisibility::Minimal,
            _ => ev::ParticleVisibility::All,
        },
    }
}

fn link_ref(l: &ServerLink) -> ev::ServerLink {
    match &l.kind {
        ServerLinkKind::Known(id) => ev::ServerLink {
            kind: Some(
                ev::ServerLinkType::ALL
                    .get(*id as usize)
                    .copied()
                    .unwrap_or(ev::ServerLinkType::ReportBug),
            ),
            label: String::new(),
            url: l.link.clone(),
        },
        ServerLinkKind::Custom(tag) => {
            use horizon_world::nbt::Tag;
            let label = match tag {
                Tag::String(s) => s.to_string_lossy(),
                Tag::Compound(_) => match tag.get("text") {
                    Some(Tag::String(s)) => s.to_string_lossy(),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            ev::ServerLink {
                kind: None,
                label,
                url: l.link.clone(),
            }
        }
    }
}

fn link_of(l: &ev::ServerLink) -> ServerLink {
    let kind = match l.kind {
        Some(k) => ServerLinkKind::Known(
            ev::ServerLinkType::ALL
                .iter()
                .position(|t| *t == k)
                .unwrap_or(0) as i32,
        ),
        None => ServerLinkKind::Custom(crate::hc::server_net::data::component_nbt(
            &Text::legacy(&l.label).component(),
        )),
    };
    ServerLink {
        kind,
        link: l.url.clone(),
    }
}

/// `FlowerPotBlock.POTTED_BY_CONTENT`'s potted block for an item.
fn potted_name(item: &str) -> String {
    let path = item.trim_start_matches("minecraft:");
    match path {
        "azalea" | "flowering_azalea" => format!("minecraft:potted_{path}_bush"),
        _ => format!("minecraft:potted_{path}"),
    }
}

/// The item a potted block holds.
fn potted_item(block: &str) -> Option<String> {
    let path = block.strip_prefix("minecraft:potted_")?;
    let path = path
        .strip_suffix("_bush")
        .filter(|p| *p == "azalea" || *p == "flowering_azalea")
        .unwrap_or(path);
    Some(format!("minecraft:{path}"))
}

impl crate::game::Server<'_> {
    // -----------------------------------------------------------------------
    // Once a tick.

    /// Once a tick, before the players' own ticks: what changed about each
    /// player since the last (`PlayerHealthChangeEvent`, `PlayerInputEvent`,
    /// `PlayerClientOptionsChangeEvent`, `PlayerRecipeBookSettingsChangeEvent`,
    /// `PlayerArmorChangeEvent`, `PlayerInventorySlotChangeEvent`,
    /// `PlayerPostRespawnEvent`, `PlayerJumpEvent`), and `PlayerLinksSendEvent`
    /// for each player seen the first time.
    pub(super) fn plugin_watch_players(&mut self, net: &mut Network) {
        let live: HashSet<u32> = net.hub.players.iter().map(|g| g.conn).collect();
        self.plugin_players
            .channels
            .retain(|conn, _| live.contains(conn));
        let health = self.plugins_listen::<ev::PlayerHealthChangeEvent>();
        let input = self.plugins_listen::<ev::PlayerInputEvent>();
        let options = self.plugins_listen::<ev::PlayerClientOptionsChangeEvent>();
        let book = self.plugins_listen::<ev::PlayerRecipeBookSettingsChangeEvent>();
        let armor = self.plugins_listen::<ev::PlayerArmorChangeEvent>();
        let slots = self.plugins_listen::<ev::PlayerInventorySlotChangeEvent>();
        let respawn = self.plugins_listen::<ev::PlayerPostRespawnEvent>();
        let jump = self.plugins_listen::<ev::PlayerJumpEvent>();
        let links = self.plugins_listen::<ev::PlayerLinksSendEvent>();
        if !(health || input || options || book || armor || slots || respawn || jump || links) {
            self.plugin_players.players.clear();
            return;
        }
        self.plugin_players
            .players
            .retain(|conn, _| live.contains(conn));
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            if !g.placed {
                continue;
            }
            let conn = g.conn;
            let now = Seen::of(g, armor, slots);
            let Some(was) = self.plugin_players.players.insert(conn, now.clone()) else {
                if links {
                    self.plugin_links(net, i);
                }
                continue;
            };
            let player = crate::game::Server::plugin_player(g);
            // PlayerJumpEvent: ServerGamePacketListenerImpl.handleMovePlayer's
            // jumpFromGround, a move off the ground upwards.
            if jump && !g.dead {
                self.plugin_jump(net, i, player.clone());
            }
            if health && was.health != now.health {
                let mut e = ev::PlayerHealthChangeEvent {
                    player: player.clone(),
                    previous_health: f64::from(was.health),
                    new_health: f64::from(now.health),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if respawn && was.dead && !now.dead {
                let mut e = ev::PlayerPostRespawnEvent {
                    respawned_location: player.location.clone(),
                    player: player.clone(),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if input && was.input != now.input {
                let mut e = ev::PlayerInputEvent {
                    player: player.clone(),
                    input: ev::Input::from_flags(now.input),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if options && was.info != now.info {
                let mut e = ev::PlayerClientOptionsChangeEvent {
                    player: player.clone(),
                    options: options_of(&now.info),
                    previous: options_of(&was.info),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if book {
                for (k, kind) in BOOKS.into_iter().enumerate() {
                    if (was.book.0[k], was.book.1[k]) == (now.book.0[k], now.book.1[k]) {
                        continue;
                    }
                    let mut e = ev::PlayerRecipeBookSettingsChangeEvent {
                        player: player.clone(),
                        recipe_book_type: kind,
                        open: now.book.0[k],
                        filtering: now.book.1[k],
                    };
                    self.plugin_fire(Some(net), &mut e);
                }
            }
            // A listener that just came sees no change from nothing.
            if armor && was.has_armor {
                for k in 0..4 {
                    if was.armor[k] == now.armor[k] {
                        continue;
                    }
                    let mut e = ev::PlayerArmorChangeEvent {
                        player: player.clone(),
                        slot_type: ARMOR[k],
                        old_item: was.armor[k].as_ref().map(stack_ref),
                        new_item: now.armor[k].as_ref().map(stack_ref),
                    };
                    self.plugin_fire(Some(net), &mut e);
                }
            }
            if slots && was.slots.len() == now.slots.len() {
                for k in 0..now.slots.len() {
                    if was.slots[k] == now.slots[k] {
                        continue;
                    }
                    // InventoryMenu's slots, and Bukkit's PlayerInventory's.
                    let (raw, slot) = match k {
                        0..9 => (36 + k as i32, k as i32),
                        9..36 => (k as i32, k as i32),
                        36..40 => (5 + (k as i32 - 36), 39 - (k as i32 - 36)),
                        _ => (45, 40),
                    };
                    let mut e = ev::PlayerInventorySlotChangeEvent {
                        player: player.clone(),
                        raw_slot: raw,
                        slot,
                        old_item_stack: was.slots[k].as_ref().map(stack_ref),
                        new_item_stack: now.slots[k].as_ref().map(stack_ref),
                    };
                    self.plugin_fire(Some(net), &mut e);
                }
            }
        }
    }
    /// The player `i`'s moves this tick, for `PlayerJumpEvent`: a cancelled
    /// jump puts them back.
    fn plugin_jump(&mut self, net: &mut Network, i: usize, player: api::PlayerRef) {
        let g = &net.hub.players[i];
        if g.moves.is_empty() || g.vehicle.is_some() || g.flying {
            return;
        }
        let total = g.moves.iter().fold([0.0; 3], |a, m| {
            [a[0] + m.0[0], a[1] + m.0[1], a[2] + m.0[2]]
        });
        let mut at = [0, 1, 2].map(|k| g.position[k] - total[k]);
        let mut ground = g.moves_from_ground;
        let mut jumped = None;
        for (d, on) in &g.moves {
            let to = [0, 1, 2].map(|k| at[k] + d[k]);
            if ground && !*on && d[1] > 0.0 {
                jumped = Some((at, to));
                break;
            }
            (at, ground) = (to, *on);
        }
        let Some((from, to)) = jumped else {
            return;
        };
        let world = g.dimension.clone();
        let (yaw, pitch) = (g.yaw, g.pitch);
        let place = |p: [f64; 3]| Location {
            world: world.clone(),
            x: p[0],
            y: p[1],
            z: p[2],
            yaw,
            pitch,
        };
        let mut e = ev::PlayerJumpEvent {
            player,
            from: place(from),
            to: place(to),
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut e);
        if !e.cancelled {
            return;
        }
        // internalTeleport to `from`.
        let g = &mut net.hub.players[i];
        (g.yaw, g.pitch) = (e.from.yaw, e.from.pitch);
        net.hub.teleport(i, Some([e.from.x, e.from.y, e.from.z]));
    }
    /// `PlayerLinksSendEvent` for the player `i`, who just came: the links
    /// their configuration got, sent again as a plugin made them.
    fn plugin_links(&mut self, net: &mut Network, i: usize) {
        let links: Vec<ev::ServerLink> = net
            .hub
            .network
            .shared
            .config
            .server_links
            .iter()
            .map(link_ref)
            .collect();
        let mut e = ev::PlayerLinksSendEvent {
            player: crate::game::Server::plugin_player(&net.hub.players[i]),
            links: links.clone(),
        };
        self.plugin_fire(Some(net), &mut e);
        if e.links != links {
            let conn = net.hub.players[i].conn;
            net.hub.send_to(
                conn,
                &C::ServerLinks(ServerLinks {
                    links: e.links.iter().map(link_of).collect(),
                }),
            );
        }
    }

    // -----------------------------------------------------------------------
    // A player's packets.

    /// A player's packet after the other groups' hooks: the plugin channels
    /// (`minecraft:register` / `minecraft:unregister`, names split by NUL):
    /// `PlayerRegisterChannelEvent` / `PlayerUnregisterChannelEvent` for
    /// each one new or gone. Never drops the packet.
    pub(super) fn plugin_players_packet(
        &mut self,
        net: &mut Network,
        conn: u32,
        packet: &S,
    ) -> bool {
        let S::CustomPayload(p) = packet else {
            return false;
        };
        let register = match p.channel.as_str() {
            "minecraft:register" => true,
            "minecraft:unregister" => false,
            _ => return false,
        };
        let listens = if register {
            self.plugins_listen::<ev::PlayerRegisterChannelEvent>()
        } else {
            self.plugins_listen::<ev::PlayerUnregisterChannelEvent>()
        };
        let names: Vec<String> = p
            .data
            .split(|b| *b == 0)
            .filter(|n| !n.is_empty())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect();
        let set = self.plugin_players.channels.entry(conn).or_default();
        let changed: Vec<String> = names
            .into_iter()
            .filter(|n| {
                if register {
                    set.insert(n.clone())
                } else {
                    set.remove(n)
                }
            })
            .collect();
        if !listens {
            return false;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return false;
        };
        for channel in changed {
            if register {
                let mut e = ev::PlayerRegisterChannelEvent {
                    player: player.clone(),
                    channel,
                };
                self.plugin_fire(Some(net), &mut e);
            } else {
                let mut e = ev::PlayerUnregisterChannelEvent {
                    player: player.clone(),
                    channel,
                };
                self.plugin_fire(Some(net), &mut e);
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Items.

    /// The player `gi` touches the dropped item `index` they may take:
    /// `PlayerAttemptPickupItemEvent`, before `PlayerPickupItemEvent` and
    /// whatever room they have. False when cancelled (with `fly_at_player`,
    /// the player is still shown taking it).
    pub(super) fn plugin_attempt_pickup(&mut self, net: &Network, gi: usize, index: usize) -> bool {
        if !self.plugins_listen::<ev::PlayerAttemptPickupItemEvent>() {
            return true;
        }
        let g = &net.hub.players[gi];
        let d = &self.dropped[index];
        // Inventory.canHold: the room in the 36 slots and the off hand.
        let items = self.res.items();
        let max = items.get(&d.stack.item).map_or(64, |i| i.max_stack);
        let room: u32 = g
            .inventory
            .iter()
            .take(36)
            .chain(std::iter::once(&g.offhand))
            .map(|s| match s {
                None => max,
                Some(s) if s.same_item_same_components(&d.stack) => max.saturating_sub(s.count),
                Some(_) => 0,
            })
            .sum();
        let count = d.stack.count;
        let mut e = ev::PlayerAttemptPickupItemEvent {
            player: crate::game::Server::plugin_player(g),
            item: api::EntityRef {
                id: d.id as i32,
                kind: "minecraft:item".into(),
                location: Location::new(self.dim.id(), d.position.x, d.position.y, d.position.z),
                player: None,
            },
            stack: stack_ref(&d.stack),
            remaining: count.saturating_sub(room),
            fly_at_player: true,
            cancelled: false,
        };
        let (id, entity, at) = (d.id, g.entity_id, g.position);
        self.plugin_fire(Some(net), &mut e);
        if !e.cancelled {
            return true;
        }
        if e.fly_at_player {
            net.hub.take_item(id as i32, entity, count as i32, at);
        }
        false
    }
    /// The player whose turn it is is about to scoop a fish (the mob `id`,
    /// into `filled`): `PlayerBucketFishEvent`, after `PlayerBucketEntityEvent`.
    /// False when cancelled.
    pub(super) fn plugin_bucket_fish(
        &mut self,
        id: u32,
        bucket: Option<&ItemStack>,
        filled: &ItemStack,
    ) -> bool {
        // AbstractFish's buckets.
        let fish = matches!(
            &*filled.item,
            "minecraft:cod_bucket"
                | "minecraft:salmon_bucket"
                | "minecraft:pufferfish_bucket"
                | "minecraft:tropical_fish_bucket"
        );
        if !fish || !self.plugins_listen::<ev::PlayerBucketFishEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerBucketFishEvent {
            player,
            entity: self.plugin_entity(None, id as i32),
            original_bucket: bucket
                .map(stack_ref)
                .unwrap_or_else(|| api::ItemStack::new("water_bucket", 1)),
            entity_bucket: stack_ref(filled),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// `Player.getProjectile` for `weapon` with `PlayerReadyArrowEvent` for
    /// each candidate (the held one, then the inventory's): the first a
    /// plugin lets the weapon use.
    pub(super) fn plugin_projectile_slot(&mut self, weapon: &str) -> Option<usize> {
        if !self.plugins_listen::<ev::PlayerReadyArrowEvent>() {
            return self.projectile_slot(weapon);
        }
        let Some(player) = self.plugin_acting() else {
            return self.projectile_slot(weapon);
        };
        let crossbow = weapon == "minecraft:crossbow";
        let arrow = |s: &ItemStack| &*s.item == "minecraft:arrow";
        let held = |s: &ItemStack| arrow(s) || crossbow && &*s.item == "minecraft:firework_rocket";
        let mut candidates: Vec<usize> = [super::shield::OFFHAND_SLOT, self.selected]
            .into_iter()
            .find(|&slot| self.stack_in(slot).is_some_and(held))
            .into_iter()
            .collect();
        candidates.extend(
            self.inventory
                .iter()
                .enumerate()
                .filter(|(_, s)| s.as_ref().is_some_and(arrow))
                .map(|(i, _)| i),
        );
        let bow = self
            .held()
            .map(stack_ref)
            .unwrap_or_else(|| api::ItemStack::new(weapon, 1));
        for slot in candidates {
            let Some(ammo) = self.stack_in(slot).map(stack_ref) else {
                continue;
            };
            let event = ev::PlayerReadyArrowEvent {
                player: player.clone(),
                bow: bow.clone(),
                arrow: ammo,
                cancelled: false,
            };
            if self.plugin_allows(None, event) {
                return Some(slot);
            }
        }
        None
    }
    /// The player whose turn it is just threw the last projectile added from
    /// `stack`: `PlayerLaunchProjectileEvent`. Whether to use the item, or
    /// None when cancelled (the projectile is gone again).
    pub(super) fn plugin_launch_projectile(&mut self, stack: &ItemStack) -> Option<bool> {
        if !self.plugins_listen::<ev::PlayerLaunchProjectileEvent>() {
            return Some(true);
        }
        let Some(player) = self.plugin_acting() else {
            return Some(true);
        };
        let p = self.creatures.projectiles.last()?;
        let kind = match &*stack.item {
            "minecraft:blue_egg" | "minecraft:brown_egg" => "minecraft:egg",
            other => other,
        };
        let mut e = ev::PlayerLaunchProjectileEvent {
            player,
            item_stack: stack_ref(stack),
            projectile: api::EntityRef {
                id: p.id as i32,
                kind: kind.to_owned(),
                location: Location::new(self.dim.id(), p.position.x, p.position.y, p.position.z),
                player: None,
            },
            should_consume: true,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            self.creatures.projectiles.pop();
            self.plugin_resync_acting();
            return None;
        }
        if !e.should_consume {
            self.plugin_resync_acting();
        }
        Some(e.should_consume)
    }
    /// The player whose turn it is let go of `stack` after `ticks`:
    /// `PlayerStopUsingItemEvent`.
    pub(super) fn plugin_stop_using(&mut self, stack: &ItemStack, ticks: u32) {
        if !self.plugins_listen::<ev::PlayerStopUsingItemEvent>() {
            return;
        }
        let Some(player) = self.plugin_acting() else {
            return;
        };
        let mut e = ev::PlayerStopUsingItemEvent {
            player,
            item: stack_ref(stack),
            ticks_held_for: ticks,
        };
        self.plugin_fire(None, &mut e);
    }
    /// The player of `conn` clicks the block at `pos` with the hand `off` or
    /// the main one: `PlayerFlowerPotManipulateEvent` when that puts a plant
    /// in an empty flower pot or takes one out (`FlowerPotBlock.useItemOn` /
    /// `useWithoutItem`). False when cancelled.
    pub(super) fn plugin_flower_pot(
        &mut self,
        net: &Network,
        conn: u32,
        off: bool,
        pos: [i32; 3],
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerFlowerPotManipulateEvent>() {
            return true;
        }
        let Some(g) = net.hub.player(conn) else {
            return true;
        };
        let registry = self.pack.assets.registry;
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let Some(name) = registry.state(state).map(|s| s.name.clone()) else {
            return true;
        };
        let held = if off {
            g.offhand.clone()
        } else {
            g.inventory.get(g.selected).cloned().flatten()
        };
        let pottable = held
            .as_ref()
            .is_some_and(|s| registry.default_state(&potted_name(&s.item)).is_some());
        // ServerPlayerGameMode.useItemOn: no block use while sneaking with
        // something in hand.
        let full_hands =
            g.inventory.get(g.selected).is_some_and(Option::is_some) || g.offhand.is_some();
        if g.sneaking && full_hands {
            return true;
        }
        let (item, placing) = if name == "minecraft:flower_pot" {
            match held.filter(|_| pottable) {
                Some(s) => (stack_ref(&s), true),
                None => return true,
            }
        } else if let Some(plant) = potted_item(&name) {
            // A pottable item against a full pot is CONSUME; the off hand
            // takes nothing (useWithoutItem is the main hand's).
            if pottable || off {
                return true;
            }
            (api::ItemStack::new(&plant, 1), false)
        } else {
            return true;
        };
        let event = ev::PlayerFlowerPotManipulateEvent {
            player: crate::game::Server::plugin_player(g),
            flowerpot: self.plugin_block(pos),
            item,
            placing,
            cancelled: false,
        };
        let allowed = self.plugin_allows(Some(net), event);
        if !allowed {
            self.plugin_resync.push(conn);
        }
        allowed
    }
    /// Where the player whose turn it is has `stack` already (-1: nowhere).
    fn plugin_source_slot(&self, stack: &ItemStack) -> i32 {
        self.inventory
            .iter()
            .take(36)
            .position(|s| {
                s.as_ref()
                    .is_some_and(|s| s.same_item_same_components(stack))
            })
            .map_or(-1, |i| i as i32)
    }
    /// The player whose turn it is picks the block at `pos` for `stack`:
    /// `PlayerPickBlockEvent`. False when cancelled.
    pub(super) fn plugin_pick_block(
        &mut self,
        pos: [i32; 3],
        include_data: bool,
        stack: &ItemStack,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerPickBlockEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerPickBlockEvent {
            player,
            block: self.plugin_block(pos),
            item: stack_ref(stack),
            include_data,
            source_slot: self.plugin_source_slot(stack),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The player whose turn it is picks the entity `id` for `stack`:
    /// `PlayerPickEntityEvent`. False when cancelled.
    pub(super) fn plugin_pick_entity(
        &mut self,
        id: i32,
        include_data: bool,
        stack: &ItemStack,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerPickEntityEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerPickEntityEvent {
            player,
            entity: self.plugin_entity(None, id),
            item: stack_ref(stack),
            include_data,
            source_slot: self.plugin_source_slot(stack),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }

    // -----------------------------------------------------------------------
    // Respawn points.

    /// The player whose turn it is gets the respawn point `config` (a bed,
    /// an anchor, `/spawnpoint`): `PlayerSpawnChangeEvent`. The point to
    /// set, or None when there is none to set: cancelled (the old one
    /// stays), or cleared by a plugin (done here).
    pub(super) fn plugin_spawn_point(
        &mut self,
        cause: ev::SpawnChangeCause,
        config: RespawnConfig,
    ) -> Option<RespawnConfig> {
        if !self.plugins_listen::<ev::PlayerSpawnChangeEvent>() {
            return Some(config);
        }
        let Some(player) = self.plugin_acting() else {
            return Some(config);
        };
        let [x, y, z] = config.pos.map(f64::from);
        let start = Location {
            world: config.dim.id().to_owned(),
            x,
            y,
            z,
            yaw: config.yaw,
            pitch: 0.0,
        };
        let mut e = ev::PlayerSpawnChangeEvent {
            player,
            new_spawn: Some(start.clone()),
            cause,
            forced: config.forced,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        if e.cancelled {
            return None;
        }
        let Some(to) = e.new_spawn else {
            self.respawn_config = None;
            if self.net_acting.is_some() {
                self.spawn = None;
            }
            return None;
        };
        if to == start && e.forced == config.forced {
            return Some(config);
        }
        Some(RespawnConfig::wrapped(
            Dim::from_id(&to.world),
            [
                to.x.floor() as i32,
                to.y.floor() as i32,
                to.z.floor() as i32,
            ],
            to.yaw,
            e.forced,
        ))
    }
    /// The player whose turn it is respawns without the bed or anchor of
    /// their point: `PlayerSpawnChangeEvent` (RESET). Whether to forget it.
    pub(super) fn plugin_spawn_reset(&mut self) -> bool {
        if !self.plugins_listen::<ev::PlayerSpawnChangeEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerSpawnChangeEvent {
            player,
            new_spawn: None,
            cause: ev::SpawnChangeCause::Reset,
            forced: false,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }

    // -----------------------------------------------------------------------
    // Chat and commands.

    /// `AsyncChatEvent` after `AsyncPlayerChatEvent`, writing back what it
    /// changed.
    pub(super) fn plugin_async_chat(&mut self, net: &Network, chat: &mut ev::PlayerChatEvent) {
        if !self.plugins_listen::<ev::AsyncChatEvent>() {
            return;
        }
        let mut e = ev::AsyncChatEvent::after(chat);
        self.plugin_fire(Some(net), &mut e);
        chat.message = e.message;
        chat.recipients = e.viewers;
        chat.cancelled = e.cancelled;
    }
    /// `command` (without its slash) did not parse: `UnknownCommandEvent`
    /// (Commands.finishParsing's failure). True when vanilla's error should
    /// be sent as it is; else the plugin's message was, or nothing.
    pub(super) fn plugin_unknown_command(
        &mut self,
        command: &str,
        error: &crate::hc::command::CommandError,
    ) -> bool {
        if !self.plugins_listen::<ev::UnknownCommandEvent>() {
            return true;
        }
        let sender = match self.plugin_acting() {
            Some(p) => CommandSender::Player(p),
            None => CommandSender::Console,
        };
        let mut start = format!("§c{}", error.message.plain());
        if let Some(input) = &error.input {
            let cursor = error.cursor.min(input.len());
            let cursor = (0..=cursor)
                .rev()
                .find(|&i| input.is_char_boundary(i))
                .unwrap_or(0);
            let from = (0..=cursor.saturating_sub(10))
                .rev()
                .find(|&i| input.is_char_boundary(i))
                .unwrap_or(0);
            let dots = if cursor > 10 { "..." } else { "" };
            start.push_str(&format!(
                "\n§7{dots}{}§c§n{}§r§c§o<--[HERE]",
                &input[from..cursor],
                &input[cursor..]
            ));
        }
        let mut e = ev::UnknownCommandEvent {
            sender,
            command_line: command.to_owned(),
            message: Some(start.clone()),
        };
        self.plugin_fire(None, &mut e);
        match e.message {
            Some(m) if m == start => true,
            Some(m) => {
                for line in m.split('\n') {
                    self.command_failure(Text::legacy(line));
                }
                false
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // The server.

    /// `PluginEnableEvent` (`enabled`) or `PluginDisableEvent` for `plugin`.
    pub(super) fn plugin_enabled(&mut self, plugin: &str, enabled: bool) {
        let plugin = plugin.to_owned();
        if enabled {
            if self.plugins_listen::<ev::PluginEnableEvent>() {
                self.plugin_fire(None, &mut ev::PluginEnableEvent { plugin });
            }
        } else if self.plugins_listen::<ev::PluginDisableEvent>() {
            self.plugin_fire(None, &mut ev::PluginDisableEvent { plugin });
        }
    }
    /// `/time` is about to move the server's clock by `amount`:
    /// `ClockTimeSkipEvent` (COMMAND). The amount, or None when cancelled.
    pub(super) fn plugin_clock_skip(&mut self, amount: i64) -> Option<i64> {
        if !self.plugins_listen::<ev::ClockTimeSkipEvent>() {
            return Some(amount);
        }
        let mut e = ev::ClockTimeSkipEvent {
            skip_reason: ev::TimeSkipReason::Command,
            skip_amount: amount,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        (!e.cancelled).then_some(e.skip_amount)
    }
    /// The start of a server tick: `ServerTickStartEvent`.
    pub(super) fn plugin_tick_start(&mut self) {
        self.plugin_players.tick += 1;
        if self.plugins.is_none() || !self.plugins_listen::<ev::ServerTickStartEvent>() {
            return;
        }
        let mut e = ev::ServerTickStartEvent {
            tick_number: self.plugin_players.tick,
        };
        self.plugin_fire(None, &mut e);
    }
    /// The end of a server tick that took `took`, the next due a tick after
    /// `due`: `ServerTickEndEvent`.
    pub(super) fn plugin_tick_end(&mut self, took: Duration, due: Instant) {
        if self.plugins.is_none() || !self.plugins_listen::<ev::ServerTickEndEvent>() {
            return;
        }
        let next = due + self.clock.tick_duration();
        let now = Instant::now();
        let remaining = if next >= now {
            (next - now).as_nanos() as i64
        } else {
            -((now - next).as_nanos() as i64)
        };
        let mut e = ev::ServerTickEndEvent {
            tick_number: self.plugin_players.tick,
            tick_duration: took.as_secs_f64() * 1000.0,
            time_remaining: remaining,
        };
        self.plugin_fire(None, &mut e);
    }
    /// The whitelist was turned on or off: `WhitelistToggleEvent`.
    pub(super) fn plugin_whitelist_toggle(&mut self, enabled: bool) {
        if self.plugins_listen::<ev::WhitelistToggleEvent>() {
            self.plugin_fire(None, &mut ev::WhitelistToggleEvent { enabled });
        }
    }
    /// `/reload` reloaded the data packs: `ServerResourcesReloadedEvent`.
    pub(super) fn plugin_resources_reloaded(&mut self) {
        if self.plugins_listen::<ev::ServerResourcesReloadedEvent>() {
            let mut e = ev::ServerResourcesReloadedEvent {
                cause: ev::ReloadCause::Command,
            };
            self.plugin_fire(None, &mut e);
        }
    }
}
