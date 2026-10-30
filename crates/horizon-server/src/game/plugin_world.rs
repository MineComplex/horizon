//! Where the server fires the plugins' world events (docs/plugins.md): the
//! rest of a player's events (statistics, item wear and mending, riptide,
//! harvests, armor stands, leads, buckets, signs, clicks at entities, the
//! client's options, the command tree, velocity, eggs, swings, tab
//! completion, the pre-login), the console's broadcast, chunks and levels
//! coming and going, bone-meal growth, portals, the world spawn, time skips,
//! raids, game events and loot.
use super::{Dim, server_net::Network};
use crate::hc::{
    items::ItemStack,
    mobs::{Mob, Who, drops},
    raid,
    server_net::menu::Slots,
};
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::play::{Clientbound as C, Serverbound as S};
use horizon_server_api::{self as api, BlockPos, Location, event as ev};
use horizon_world::registry::BlockStateId;
use std::collections::{HashMap, HashSet};

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    /// A berry harvest under way (`plugin_harvest_begin`): the plant, its
    /// state, and the first of its drops in the spill.
    harvest: Option<([i32; 3], BlockStateId, usize)>,
    /// What each player's client last told (language, main hand) and the
    /// command tree they were last sent (permission, the commands' keys).
    clients: HashMap<u128, Client>,
    /// The chunks of each level the plugins last saw, with the chunk map's
    /// membership revision then.
    chunks: HashMap<Dim, (u64, HashSet<(i32, i32)>)>,
    /// The raids of each level the plugins last saw, still running.
    raids: HashMap<Dim, HashMap<i32, api::event::RaidInfo>>,
    /// Players `AsyncPlayerPreLoginEvent` turned away, by connection, and
    /// what they are told.
    turned_away: HashMap<u32, String>,
    /// Containers whose rolled loot the plugins have seen.
    looted: HashSet<(Dim, (i32, i32, i32))>,
}

#[derive(Clone, PartialEq)]
struct Client {
    language: String,
    main_hand: i32,
    commands: (u8, Option<u64>, usize),
}

/// The custom statistics that go up every tick, which fire no
/// `PlayerStatisticIncrementEvent`.
const TICKING_STATS: [&str; 5] = [
    "minecraft:play_time",
    "minecraft:total_world_time",
    "minecraft:time_since_death",
    "minecraft:time_since_rest",
    "minecraft:sneak_time",
];

/// `ArmorStand`'s equipment slots, as Bukkit names them.
const STAND_SLOTS: [&str; 6] = ["HAND", "OFF_HAND", "FEET", "LEGS", "CHEST", "HEAD"];

fn raid_info(id: i32, r: &raid::Raid) -> ev::RaidInfo {
    ev::RaidInfo {
        id,
        center: BlockPos::new(r.center[0], r.center[1], r.center[2]),
        omen_level: r.omen_level,
        spawned_groups: r.groups_spawned,
        total_groups: r.num_groups,
        started: r.started,
    }
}

fn stack_ref(s: &ItemStack) -> api::ItemStack {
    super::plugin_events::stack_ref(s)
}

/// A plugin's stack in place of `old`'s: the first of them of the same
/// item keeps its components.
fn stacks_from(new: &[api::ItemStack], old: &[ItemStack]) -> Vec<ItemStack> {
    new.iter()
        .filter(|s| s.count > 0)
        .map(|s| match old.iter().find(|o| *o.item == *s.item) {
            Some(o) => ItemStack {
                count: s.count,
                ..o.clone()
            },
            None => ItemStack::new(&s.item, s.count),
        })
        .collect()
}

impl crate::game::Server<'_> {
    fn plugin_location(&self, at: [f64; 3]) -> Location {
        Location::new(self.dim.id(), at[0], at[1], at[2])
    }
    /// The entity of the player whose turn it is.
    fn plugin_turn_entity(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .plugin_seen_ids
            .iter()
            .find(|(_, uuid)| *uuid == player.uuid)
            .map_or(0, |(id, _)| *id);
        Some(api::EntityRef::of_player(id, player))
    }
    fn plugin_who_entity(&self, who: Who) -> Option<api::EntityRef> {
        match who {
            Who::Mob(id) => Some(self.plugin_entity(None, id as i32)),
            Who::Player => self.plugin_turn_entity(),
        }
    }

    // -----------------------------------------------------------------------
    // The player whose turn it is.

    /// As the turn ends: `PlayerStatisticIncrementEvent` for each statistic
    /// it raised; a cancelled one goes back down.
    pub(super) fn plugin_statistics(&mut self) {
        if !self.plugins_listen::<ev::PlayerStatisticIncrementEvent>()
            || self.stats.events.is_empty()
        {
            return;
        }
        let Some(player) = self.plugin_acting() else {
            return;
        };
        let events = std::mem::take(&mut self.stats.events);
        // What each counter was before the turn's raises.
        let mut raised: HashMap<(String, String), i32> = HashMap::new();
        for (ty, key, n) in &events {
            if let Some(n) = n {
                *raised.entry((ty.clone(), key.clone())).or_default() += n;
            }
        }
        let value = |s: &Self, ty: &str, key: &str| {
            s.stats
                .values
                .get(ty)
                .and_then(|m| m.get(key))
                .copied()
                .unwrap_or(0)
        };
        let mut at: HashMap<(String, String), i32> = raised
            .into_iter()
            .map(|(k, n)| {
                let now = value(self, &k.0, &k.1);
                (k, now.saturating_sub(n))
            })
            .collect();
        let mut kept = Vec::with_capacity(events.len());
        for (ty, key, n) in events {
            let Some(n) = n.filter(|n| *n > 0) else {
                kept.push((ty, key, n));
                continue;
            };
            if ty == "minecraft:custom" && TICKING_STATS.contains(&key.as_str()) {
                kept.push((ty, key, Some(n)));
                continue;
            }
            let previous = at.get(&(ty.clone(), key.clone())).copied().unwrap_or(0);
            let event = ev::PlayerStatisticIncrementEvent {
                player: player.clone(),
                statistic: ty.clone(),
                key: key.clone(),
                previous_value: previous,
                new_value: previous.saturating_add(n),
                cancelled: false,
            };
            if self.plugin_allows(None, event) {
                at.insert((ty.clone(), key.clone()), previous.saturating_add(n));
                kept.push((ty, key, Some(n)));
            } else if let Some(v) = self.stats.values.get_mut(&ty).and_then(|m| m.get_mut(&key)) {
                *v = v.saturating_sub(n);
            }
        }
        kept.append(&mut self.stats.events);
        self.stats.events = kept;
    }
    /// `stack` of the player whose turn it is is about to lose `loss`
    /// (`original` before Unbreaking): `PlayerItemDamageEvent`, then, when
    /// it `breaks` that way, `PlayerItemBreakEvent`. The points it loses.
    pub(super) fn plugin_item_damage(
        &mut self,
        stack: &ItemStack,
        original: u32,
        loss: u32,
        breaks: bool,
    ) -> u32 {
        let damage = self.plugins_listen::<ev::PlayerItemDamageEvent>();
        let broken = breaks && self.plugins_listen::<ev::PlayerItemBreakEvent>();
        if !damage && !broken {
            return loss;
        }
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(0, |i| i.max_damage);
        let Some(player) = self.plugin_acting().filter(|_| max > 0) else {
            return loss;
        };
        let mut loss = loss;
        if damage {
            let mut event = ev::PlayerItemDamageEvent {
                player: player.clone(),
                item: stack_ref(stack),
                damage: loss,
                original_damage: original,
                cancelled: false,
            };
            self.plugin_fire(None, &mut event);
            if event.cancelled {
                self.plugin_resync_acting();
                return 0;
            }
            if event.damage != loss {
                self.plugin_resync_acting();
            }
            loss = event.damage;
        }
        if broken && loss > 0 && stack.damage.saturating_add(loss) >= max {
            let mut event = ev::PlayerItemBreakEvent {
                player,
                item: stack_ref(stack),
            };
            self.plugin_fire(None, &mut event);
        }
        loss
    }
    /// Mending is about to repair `repaired` points of `stack`:
    /// `PlayerItemMendEvent`. The points to repair, or None when cancelled.
    pub(super) fn plugin_item_mend(&mut self, stack: &ItemStack, repaired: u32) -> Option<u32> {
        if !self.plugins_listen::<ev::PlayerItemMendEvent>() {
            return Some(repaired);
        }
        let Some(player) = self.plugin_acting() else {
            return Some(repaired);
        };
        let mut event = ev::PlayerItemMendEvent {
            player,
            item: stack_ref(stack),
            repair_amount: repaired,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        (!event.cancelled).then_some(event.repair_amount)
    }
    /// The player whose turn it is launched with a Riptide trident:
    /// `PlayerRiptideEvent`.
    pub(super) fn plugin_riptide(&mut self, stack: &ItemStack) {
        if !self.plugins_listen::<ev::PlayerRiptideEvent>() {
            return;
        }
        let Some(player) = self.plugin_acting() else {
            return;
        };
        let mut event = ev::PlayerRiptideEvent {
            player,
            item: stack_ref(stack),
        };
        self.plugin_fire(None, &mut event);
    }
    /// The player whose turn it is uses the block at `pos`: a berry plant's
    /// drops wait for `plugin_harvest_drops`.
    pub(super) fn plugin_harvest_begin(&mut self, pos: [i32; 3]) {
        self.plugin_world.harvest = None;
        if !self.plugins_listen::<ev::PlayerHarvestBlockEvent>() {
            return;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if matches!(
            &*self.res.facts().get(state).name,
            "minecraft:sweet_berry_bush" | "minecraft:cave_vines" | "minecraft:cave_vines_plant"
        ) {
            self.plugin_world.harvest = Some((pos, state, self.spill.spawns.len()));
        }
    }
    /// Before the spill's items spawn: `PlayerHarvestBlockEvent` for the
    /// harvest under way. A plugin's items replace the drops; cancelled, there
    /// are none and the plant is put back.
    pub(super) fn plugin_harvest_drops(&mut self) {
        let Some((pos, state, from)) = self.plugin_world.harvest.take() else {
            return;
        };
        let Some(player) = self.plugin_acting() else {
            return;
        };
        if self.spill.spawns.len() <= from {
            return;
        }
        let old: Vec<ItemStack> = self.spill.spawns[from..]
            .iter()
            .map(|s| s.stack.clone())
            .collect();
        let items: Vec<api::ItemStack> = old.iter().map(stack_ref).collect();
        let mut event = ev::PlayerHarvestBlockEvent {
            player,
            block: api::Block {
                world: self.dim.id().to_owned(),
                pos: BlockPos::new(pos[0], pos[1], pos[2]),
                state: self.state_string(state),
            },
            items: items.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            self.spill.spawns.truncate(from);
            let _ = self.world.set(pos[0], pos[1], pos[2], state);
            return;
        }
        if event.items == items {
            return;
        }
        let spots: Vec<[f64; 3]> = self.spill.spawns[from..].iter().map(|s| s.pos).collect();
        let template = self.spill.spawns.drain(from..).next();
        let Some(template) = template else {
            return;
        };
        for (k, stack) in stacks_from(&event.items, &old).into_iter().enumerate() {
            self.spill.spawns.push(super::spill::Spawn {
                stack,
                pos: spots[k % spots.len()],
                motion: template.motion,
                delay: template.delay,
            });
        }
    }
    /// The player whose turn it is is about to swap `held` with the armor
    /// stand `index`'s `current` in `slot`: `PlayerArmorStandManipulateEvent`.
    /// False when cancelled.
    pub(super) fn plugin_armor_stand(
        &mut self,
        index: usize,
        held: Option<&ItemStack>,
        current: Option<&ItemStack>,
        slot: usize,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerArmorStandManipulateEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let stand = &self.armor_stands[index];
        let p = stand.position;
        let mut location = self.plugin_location([p.x, p.y, p.z]);
        location.yaw = stand.yaw;
        let entity = api::EntityRef {
            id: stand.id as i32,
            kind: "minecraft:armor_stand".into(),
            location,
            player: None,
        };
        let event = ev::PlayerArmorStandManipulateEvent {
            player,
            entity,
            player_item: held.map(stack_ref),
            armor_stand_item: current.map(stack_ref),
            slot: STAND_SLOTS.get(slot).copied().unwrap_or("HAND").to_owned(),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The player whose turn it is is about to put a lead on the mob `id`
    /// (`PlayerLeashEntityEvent`) or take it off (`PlayerUnleashEntityEvent`,
    /// `drop`: the lead falls). False when cancelled.
    pub(super) fn plugin_leash(&mut self, id: u32, leash: bool, drop: bool) -> bool {
        let listens = if leash {
            self.plugins_listen::<ev::PlayerLeashEntityEvent>()
        } else {
            self.plugins_listen::<ev::PlayerUnleashEntityEvent>()
        };
        if !listens {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let entity = self.plugin_entity(None, id as i32);
        let allowed = if leash {
            self.plugin_allows(
                None,
                ev::PlayerLeashEntityEvent {
                    player,
                    entity,
                    cancelled: false,
                },
            )
        } else {
            self.plugin_allows(
                None,
                ev::PlayerUnleashEntityEvent {
                    player,
                    entity,
                    drop_leash: drop,
                    cancelled: false,
                },
            )
        };
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The player whose turn it is is about to scoop the mob `id` with
    /// `bucket` into `filled`: `PlayerBucketEntityEvent`. False when cancelled.
    pub(super) fn plugin_bucket_entity(
        &mut self,
        id: u32,
        bucket: Option<&ItemStack>,
        filled: &ItemStack,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerBucketEntityEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerBucketEntityEvent {
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
    /// The player whose turn it is threw the egg `p`, which broke: their
    /// `PlayerEggThrowEvent`. Whether, how many and what hatches.
    pub(super) fn plugin_egg(
        &mut self,
        p: &crate::hc::projectiles::Projectile,
        hatching: bool,
        hatches: u8,
    ) -> (bool, u8, String) {
        let chicken = "minecraft:chicken".to_owned();
        if !self.plugins_listen::<ev::PlayerEggThrowEvent>() {
            return (hatching, hatches, chicken);
        }
        let Some(player) = self.plugin_shooter(p).and_then(|e| e.player) else {
            return (hatching, hatches, chicken);
        };
        let at = p.position;
        let mut event = ev::PlayerEggThrowEvent {
            player,
            egg: api::EntityRef {
                id: p.id as i32,
                kind: "minecraft:egg".into(),
                location: self.plugin_location([at.x, at.y, at.z]),
                player: None,
            },
            hatching,
            num_hatches: hatches,
            hatching_type: chicken,
        };
        self.plugin_fire(None, &mut event);
        let kind = if event.hatching_type.contains(':') {
            event.hatching_type
        } else {
            format!("minecraft:{}", event.hatching_type)
        };
        (event.hatching, event.num_hatches, kind)
    }

    // -----------------------------------------------------------------------
    // A player's packets and client.

    /// A player's packet after `plugin_packet`: the events of this group.
    /// True when a plugin cancelled it (the game then drops it).
    pub(super) fn plugin_world_packet(&mut self, net: &mut Network, conn: u32, packet: &S) -> bool {
        if self.plugins.is_none() {
            return false;
        }
        match packet {
            S::Interact(p) if self.plugins_listen::<ev::PlayerInteractAtEntityEvent>() => {
                let Some(player) = Self::plugin_player_of(net, conn) else {
                    return false;
                };
                let event = ev::PlayerInteractAtEntityEvent {
                    player,
                    entity: self.plugin_entity(Some(net), p.id),
                    hand: if p.hand == 1 {
                        ev::Hand::Off
                    } else {
                        ev::Hand::Main
                    },
                    position: p.location,
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                self.plugin_resync.push(conn);
                true
            }
            S::Punch(_) if self.plugins_listen::<ev::PlayerAnimationEvent>() => {
                let Some(player) = Self::plugin_player_of(net, conn) else {
                    return false;
                };
                let event = ev::PlayerAnimationEvent {
                    player,
                    animation: ev::PlayerAnimationType::ArmSwing,
                    cancelled: false,
                };
                !self.plugin_allows(Some(net), event)
            }
            _ => false,
        }
    }
    /// Once a tick: `PlayerLocaleChangeEvent` and `PlayerChangedMainHandEvent`
    /// for what the players' clients told since, and `PlayerCommandSendEvent`
    /// for each command tree sent (a plugin's removals send it again).
    pub(super) fn plugin_watch_world(&mut self, net: &mut Network) {
        let locale = self.plugins_listen::<ev::PlayerLocaleChangeEvent>();
        let hand = self.plugins_listen::<ev::PlayerChangedMainHandEvent>();
        let commands = self.plugins_listen::<ev::PlayerCommandSendEvent>();
        if !locale && !hand && !commands {
            self.plugin_world.clients.clear();
            return;
        }
        let key = (net.hub.mod_commands_key(), net.hub.extra_commands.len());
        let mut seen = HashMap::new();
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            if !g.placed {
                continue;
            }
            let now = Client {
                language: g.info.language.clone(),
                main_hand: g.info.main_hand,
                commands: (g.permission, key.0, key.1),
            };
            let (uuid, conn) = (g.uuid, g.conn);
            let before = self.plugin_world.clients.get(&uuid).cloned();
            seen.insert(uuid, now.clone());
            if before.as_ref() == Some(&now) {
                continue;
            }
            let player = crate::game::Server::plugin_player(g);
            if let Some(b) = &before {
                if locale && b.language != now.language {
                    let mut event = ev::PlayerLocaleChangeEvent {
                        player: player.clone(),
                        locale: now.language.clone(),
                    };
                    self.plugin_fire(Some(net), &mut event);
                }
                if hand && b.main_hand != now.main_hand {
                    let mut event = ev::PlayerChangedMainHandEvent {
                        player: player.clone(),
                        main_hand: if now.main_hand == 0 {
                            ev::MainHand::Left
                        } else {
                            ev::MainHand::Right
                        },
                    };
                    self.plugin_fire(Some(net), &mut event);
                }
            }
            if commands && before.is_none_or(|b| b.commands != now.commands) {
                let mut packet = net.hub.commands_packet(i);
                let names: Vec<String> = packet
                    .nodes
                    .get(packet.root as usize)
                    .map(|root| {
                        root.children
                            .iter()
                            .filter_map(|&c| packet.nodes.get(c as usize))
                            .map(|n| n.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let mut event = ev::PlayerCommandSendEvent {
                    player,
                    commands: names.clone(),
                };
                self.plugin_fire(Some(net), &mut event);
                if event.commands != names {
                    let keep: HashSet<&str> = event.commands.iter().map(String::as_str).collect();
                    let nodes = packet.nodes.clone();
                    if let Some(root) = packet.nodes.get_mut(packet.root as usize) {
                        root.children.retain(|&c| {
                            nodes
                                .get(c as usize)
                                .is_some_and(|n| keep.contains(n.name.as_str()))
                        });
                    }
                    net.hub.send_to(conn, &C::Commands(packet));
                }
            }
        }
        self.plugin_world.clients = seen;
    }
    /// The server is about to send the player of `conn` their velocity `v`:
    /// `PlayerVelocityEvent`. What to send, or None when cancelled.
    pub(super) fn plugin_velocity(&mut self, net: &Network, conn: u32, v: Vec3) -> Option<Vec3> {
        if !self.plugins_listen::<ev::PlayerVelocityEvent>() {
            return Some(v);
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return Some(v);
        };
        let mut event = ev::PlayerVelocityEvent {
            player,
            velocity: [v.x, v.y, v.z],
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        let [x, y, z] = event.velocity;
        (!event.cancelled).then(|| Vec3::new(x, y, z))
    }
    /// The player of `conn` took an orb: `PlayerExpCooldownChangeEvent` for
    /// the `cooldown` that follows. The cooldown to keep.
    pub(super) fn plugin_exp_cooldown(&mut self, net: &Network, conn: u32, cooldown: u32) -> u32 {
        if !self.plugins_listen::<ev::PlayerExpCooldownChangeEvent>() {
            return cooldown;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return cooldown;
        };
        let mut event = ev::PlayerExpCooldownChangeEvent {
            player,
            new_cooldown: cooldown,
            reason: ev::ExpCooldownReason::PickupOrb,
        };
        self.plugin_fire(Some(net), &mut event);
        event.new_cooldown
    }
    /// The completions of `buffer` for the player of `conn`:
    /// `AsyncTabCompleteEvent`, then (unless it handled them)
    /// `TabCompleteEvent`. A cancelled one leaves none.
    pub(super) fn plugin_tab_complete(
        &mut self,
        net: &Network,
        conn: u32,
        buffer: &str,
        range: (usize, usize),
        suggestions: Vec<crate::hc::command::Suggestion>,
    ) -> Vec<crate::hc::command::Suggestion> {
        let asks = self.plugins_listen::<ev::AsyncTabCompleteEvent>();
        let tab = self.plugins_listen::<ev::TabCompleteEvent>();
        if !asks && !tab {
            return suggestions;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return suggestions;
        };
        let texts: Vec<String> = suggestions.iter().map(|s| s.text.clone()).collect();
        let mut completions = texts.clone();
        let mut handled = false;
        if asks {
            let mut event = ev::AsyncTabCompleteEvent {
                sender: player.clone(),
                buffer: buffer.to_owned(),
                completions,
                handled: false,
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut event);
            if event.cancelled {
                return Vec::new();
            }
            completions = event.completions;
            handled = event.handled;
        }
        if tab && !handled {
            let mut event = ev::TabCompleteEvent {
                sender: player,
                buffer: buffer.to_owned(),
                completions,
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut event);
            if event.cancelled {
                return Vec::new();
            }
            completions = event.completions;
        }
        if completions == texts {
            return suggestions;
        }
        completions
            .into_iter()
            .map(|text| match suggestions.iter().find(|s| s.text == text) {
                Some(s) => s.clone(),
                None => crate::hc::command::Suggestion {
                    range,
                    text,
                    tooltip: None,
                    int_value: None,
                },
            })
            .collect()
    }
    /// The console's `say`: `BroadcastMessageEvent`. The line to send, or
    /// None when cancelled.
    pub(super) fn plugin_broadcast(&mut self, message: String) -> Option<String> {
        if !self.plugins_listen::<ev::BroadcastMessageEvent>() {
            return Some(message);
        }
        let mut event = ev::BroadcastMessageEvent {
            message,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        (!event.cancelled).then_some(event.message)
    }
    /// The player of `conn` joined, before they are placed:
    /// `AsyncPlayerPreLoginEvent`. False when a plugin turned them away
    /// (`plugin_turn_away` then kicks them once placed).
    pub(super) fn plugin_pre_login(&mut self, net: &Network, conn: u32) -> bool {
        if !self.plugins_listen::<ev::AsyncPlayerPreLoginEvent>() {
            return true;
        }
        let Some(g) = net.hub.player(conn) else {
            return true;
        };
        let mut event = ev::AsyncPlayerPreLoginEvent {
            name: g.name.clone(),
            uuid: g.uuid,
            address: g.address.ip().to_string(),
            allowed: true,
            kick_message: String::new(),
        };
        self.plugin_fire(Some(net), &mut event);
        if event.allowed {
            return true;
        }
        let reason = if event.kick_message.is_empty() {
            "You are not allowed on this server".to_owned()
        } else {
            event.kick_message
        };
        self.plugin_world.turned_away.insert(conn, reason);
        false
    }
    /// Kick the player of `conn` `plugin_pre_login` turned away.
    pub(super) fn plugin_turn_away(&mut self, net: &mut Network, conn: u32) {
        let reason = self
            .plugin_world
            .turned_away
            .remove(&conn)
            .unwrap_or_default();
        net.hub.kick(
            conn,
            crate::hc::server_net::data::component_nbt(
                &crate::hc::text::Text::legacy(&reason).component(),
            ),
        );
    }
    /// The editor of the sign at `pos` (`front`) is about to open for the
    /// player of `conn`: `PlayerSignOpenEvent`. False when cancelled.
    pub(super) fn plugin_sign_open(
        &mut self,
        net: &Network,
        conn: u32,
        pos: [i32; 3],
        front: bool,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerSignOpenEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::PlayerSignOpenEvent {
            sign: self.plugin_block_in(&player.location.world, pos),
            player,
            front,
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }

    // -----------------------------------------------------------------------
    // Chunks and levels.

    /// After the level's chunks streamed: `ChunkUnloadEvent` for each chunk
    /// gone, `ChunkLoadEvent` (and `ChunkPopulateEvent` for a new one) for
    /// each one come.
    pub(super) fn plugin_watch_chunks(&mut self) {
        let listens = self.plugins_listen::<ev::ChunkLoadEvent>()
            || self.plugins_listen::<ev::ChunkUnloadEvent>()
            || self.plugins_listen::<ev::ChunkPopulateEvent>();
        let dim = self.dim;
        if !listens {
            self.plugin_world.chunks.remove(&dim);
            return;
        }
        let revision = self.world.chunks.membership_revision();
        if self
            .plugin_world
            .chunks
            .get(&dim)
            .is_some_and(|(r, _)| *r == revision)
        {
            return;
        }
        let now: HashSet<(i32, i32)> = self.world.chunks.keys().copied().collect();
        let before = self
            .plugin_world
            .chunks
            .insert(dim, (revision, now.clone()))
            .map(|(_, set)| set)
            .unwrap_or_default();
        let world = dim.id().to_owned();
        let mut gone: Vec<_> = before.difference(&now).copied().collect();
        gone.sort_unstable();
        for (x, z) in gone {
            let mut event = ev::ChunkUnloadEvent {
                world: world.clone(),
                x,
                z,
            };
            self.plugin_fire(None, &mut event);
        }
        let mut came: Vec<(i32, i32, bool)> = now
            .difference(&before)
            .map(|&(x, z)| {
                (
                    x,
                    z,
                    self.world.chunks.get(&(x, z)).is_some_and(|c| c.fresh),
                )
            })
            .collect();
        came.sort_unstable();
        for (x, z, fresh) in came {
            let mut event = ev::ChunkLoadEvent {
                world: world.clone(),
                x,
                z,
                is_new_chunk: fresh,
            };
            self.plugin_fire(None, &mut event);
            if fresh {
                let mut event = ev::ChunkPopulateEvent {
                    world: world.clone(),
                    x,
                    z,
                };
                self.plugin_fire(None, &mut event);
            }
        }
    }
    /// The level of `dim` was opened: `WorldInitEvent`, then `WorldLoadEvent`.
    pub(super) fn plugin_world_opened(&mut self, dim: Dim) {
        let world = dim.id().to_owned();
        if self.plugins_listen::<ev::WorldInitEvent>() {
            let mut event = ev::WorldInitEvent {
                world: world.clone(),
            };
            self.plugin_fire(None, &mut event);
        }
        if self.plugins_listen::<ev::WorldLoadEvent>() {
            let mut event = ev::WorldLoadEvent { world };
            self.plugin_fire(None, &mut event);
        }
    }
    /// The idle level of `dim` is about to close: `WorldUnloadEvent`. False
    /// when cancelled.
    pub(super) fn plugin_world_unload(&mut self, dim: Dim) -> bool {
        if !self.plugins_listen::<ev::WorldUnloadEvent>() {
            return true;
        }
        let event = ev::WorldUnloadEvent {
            world: dim.id().to_owned(),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if allowed {
            self.plugin_world.chunks.remove(&dim);
            self.plugin_world.raids.remove(&dim);
        }
        allowed
    }

    // -----------------------------------------------------------------------
    // Growth, portals, the spawn and time.

    /// The blocks set since the level's edit log stood at `from` (on lit
    /// chunks): position, old and new state.
    fn plugin_edits_since(&self, from: usize) -> Vec<([i32; 3], u32, u32)> {
        self.world
            .light_edits
            .get(from..)
            .unwrap_or_default()
            .iter()
            .map(|&((x, y, z), old, new)| ([x, y, z], old, new))
            .collect()
    }
    /// Put back what `edits` set at the positions `undo` keeps.
    fn plugin_undo(&mut self, edits: &[([i32; 3], u32, u32)], undo: impl Fn([i32; 3]) -> bool) {
        for &(pos, old, _) in edits.iter().rev() {
            if undo(pos) {
                let _ = self.world.set(pos[0], pos[1], pos[2], BlockStateId(old));
            }
        }
    }
    /// What each edited position holds now, as plugins see it.
    fn plugin_edited_blocks(&self, edits: &[([i32; 3], u32, u32)]) -> Vec<api::Block> {
        let mut seen = HashSet::new();
        edits
            .iter()
            .filter(|(pos, ..)| seen.insert(*pos))
            .map(|(pos, ..)| self.plugin_block(*pos))
            .collect()
    }
    /// Bone meal is about to be used at `pos`: when it is a sapling, a
    /// mushroom or a fungus, the edit log's mark and the plant's block for
    /// `plugin_grow_end`.
    pub(super) fn plugin_grow_begin(&self, pos: [i32; 3]) -> Option<(usize, String)> {
        if !self.plugins_listen::<ev::StructureGrowEvent>() {
            return None;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let name = self.res.facts().get(state).name.to_string();
        let grows = name.ends_with("_sapling")
            || name.ends_with("_mushroom")
            || name.ends_with("_fungus")
            || name.ends_with("azalea")
            || name == "minecraft:mangrove_propagule";
        grows.then_some((self.world.light_edits.len(), name))
    }
    /// After the bone meal: `StructureGrowEvent` when the plant at `pos`
    /// grew into more than itself. A plugin's removals are taken back; a
    /// cancelled growth is taken back whole.
    pub(super) fn plugin_grow_end(&mut self, begun: Option<(usize, String)>, pos: [i32; 3]) {
        let Some((from, species)) = begun else {
            return;
        };
        let edits = self.plugin_edits_since(from);
        if !edits.iter().any(|(p, ..)| *p != pos) {
            return;
        }
        let blocks = self.plugin_edited_blocks(&edits);
        let mut event = ev::StructureGrowEvent {
            location: Location::new(
                self.dim.id(),
                f64::from(pos[0]),
                f64::from(pos[1]),
                f64::from(pos[2]),
            ),
            species,
            bonemeal: true,
            player: self.plugin_acting(),
            blocks: blocks.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            self.plugin_undo(&edits, |_| true);
        } else if event.blocks.len() != blocks.len() {
            let kept: HashSet<[i32; 3]> = event
                .blocks
                .iter()
                .map(|b| [b.pos.x, b.pos.y, b.pos.z])
                .collect();
            self.plugin_undo(&edits, |p| !kept.contains(&p));
        }
    }
    /// Fire is about to light the portal of `blocks` (`state` each):
    /// `PortalCreateEvent` (FIRE). False when cancelled.
    pub(super) fn plugin_portal_light(&mut self, blocks: &[[i32; 3]], state: BlockStateId) -> bool {
        if !self.plugins_listen::<ev::PortalCreateEvent>() {
            return true;
        }
        let world = self.dim.id().to_owned();
        let state = self.state_string(state);
        let event = ev::PortalCreateEvent {
            blocks: blocks
                .iter()
                .map(|p| api::Block {
                    world: world.clone(),
                    pos: BlockPos::new(p[0], p[1], p[2]),
                    state: state.clone(),
                })
                .collect(),
            world,
            entity: self.plugin_turn_entity(),
            reason: ev::PortalCreateReason::Fire,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// Where the edit log stands before a portal is made for an arrival.
    pub(super) fn plugin_portal_pair_begin(&self) -> Option<usize> {
        self.plugins_listen::<ev::PortalCreateEvent>()
            .then_some(self.world.light_edits.len())
    }
    /// A portal was made for an arrival: `PortalCreateEvent` (NETHER_PAIR).
    /// False when cancelled (it is taken back).
    pub(super) fn plugin_portal_pair_end(&mut self, from: Option<usize>) -> bool {
        let Some(from) = from else {
            return true;
        };
        let edits = self.plugin_edits_since(from);
        if edits.is_empty() {
            return true;
        }
        let event = ev::PortalCreateEvent {
            world: self.dim.id().to_owned(),
            blocks: self.plugin_edited_blocks(&edits),
            entity: None,
            reason: ev::PortalCreateReason::NetherPair,
            cancelled: false,
        };
        if self.plugin_allows(None, event) {
            return true;
        }
        self.plugin_undo(&edits, |_| true);
        false
    }
    /// `/setworldspawn` moved the world spawn from `old` to `new`:
    /// `SpawnChangeEvent`.
    pub(super) fn plugin_spawn_change(&mut self, old: Option<[i32; 3]>, new: [i32; 3]) {
        if !self.plugins_listen::<ev::SpawnChangeEvent>() {
            return;
        }
        let at =
            |p: [i32; 3]| self.plugin_location([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
        let mut event = ev::SpawnChangeEvent {
            world: self.dim.id().to_owned(),
            previous_location: old.map(at),
            location: at(new),
        };
        self.plugin_fire(None, &mut event);
    }
    /// The time of day is about to jump by `amount`: `TimeSkipEvent`. The
    /// jump to make, or None when cancelled.
    pub(super) fn plugin_time_skip(
        &mut self,
        reason: ev::TimeSkipReason,
        amount: i64,
    ) -> Option<i64> {
        if !self.plugins_listen::<ev::TimeSkipEvent>() {
            return Some(amount);
        }
        let mut event = ev::TimeSkipEvent {
            world: self.dim.id().to_owned(),
            skip_reason: reason,
            skip_amount: amount,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        (!event.cancelled).then_some(event.skip_amount)
    }

    // -----------------------------------------------------------------------
    // Raids.

    /// The Raid Omen of the player whose turn it is is about to start or
    /// strengthen a raid at `center` (`existing`, when there is one there):
    /// `RaidTriggerEvent`. False when cancelled.
    pub(super) fn plugin_raid_trigger(
        &mut self,
        center: [i32; 3],
        existing: Option<(i32, &raid::Raid)>,
    ) -> bool {
        if !self.plugins_listen::<ev::RaidTriggerEvent>() {
            return true;
        }
        let raid = existing.map_or_else(
            || ev::RaidInfo {
                center: BlockPos::new(center[0], center[1], center[2]),
                ..Default::default()
            },
            |(id, r)| raid_info(id, r),
        );
        let event = ev::RaidTriggerEvent {
            raid,
            world: self.dim.id().to_owned(),
            player: self.plugin_acting(),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// A wave of the raid `id` came: `RaidSpawnWaveEvent`.
    pub(super) fn plugin_raid_wave(
        &mut self,
        id: i32,
        r: Option<&raid::Raid>,
        leader: Option<u32>,
        raiders: &[u32],
    ) {
        if !self.plugins_listen::<ev::RaidSpawnWaveEvent>() {
            return;
        }
        let Some(r) = r else {
            return;
        };
        let mut event = ev::RaidSpawnWaveEvent {
            raid: raid_info(id, r),
            world: self.dim.id().to_owned(),
            leader: leader.map(|m| self.plugin_entity(None, m as i32)),
            raiders: raiders
                .iter()
                .map(|&m| self.plugin_entity(None, m as i32))
                .collect(),
        };
        self.plugin_fire(None, &mut event);
    }
    /// The raid `id` was won by `heroes`: `RaidFinishEvent`.
    pub(super) fn plugin_raid_finish(
        &mut self,
        id: i32,
        r: Option<&raid::Raid>,
        heroes: &[[i32; 4]],
    ) {
        if !self.plugins_listen::<ev::RaidFinishEvent>() {
            return;
        }
        let Some(r) = r else {
            return;
        };
        let winners = heroes
            .iter()
            .map(|h| crate::hc::server_net::playerdata::ints_uuid(*h))
            .filter_map(|uuid| self.plugin_seen.iter().find(|p| p.uuid == uuid).cloned())
            .collect();
        let mut event = ev::RaidFinishEvent {
            raid: raid_info(id, r),
            world: self.dim.id().to_owned(),
            winners,
        };
        self.plugin_fire(None, &mut event);
    }
    /// After the raids ticked: `RaidStopEvent` for each raid of the level
    /// that stopped or is gone since the last look.
    pub(super) fn plugin_watch_raids(&mut self, raids: Vec<(i32, &raid::Raid)>) {
        let dim = self.dim;
        if !self.plugins_listen::<ev::RaidStopEvent>() {
            self.plugin_world.raids.remove(&dim);
            return;
        }
        let peaceful = self.world_data.difficulty == 0;
        let mut stopped = Vec::new();
        let mut running = HashMap::new();
        for (id, r) in raids {
            if r.is_stopped() {
                let reason = if peaceful {
                    ev::RaidStopReason::Peace
                } else if r.ticks_active >= 48_000 {
                    ev::RaidStopReason::Timeout
                } else if r.post_raid_ticks > 0 || r.celebration_ticks > 0 {
                    ev::RaidStopReason::Finished
                } else if r.groups_spawned == 0 && r.active {
                    ev::RaidStopReason::NotInVillage
                } else {
                    ev::RaidStopReason::Other
                };
                stopped.push((id, raid_info(id, r), reason));
            } else {
                running.insert(id, raid_info(id, r));
            }
        }
        let before = self
            .plugin_world
            .raids
            .insert(dim, running.clone())
            .unwrap_or_default();
        let world = dim.id().to_owned();
        let mut ended: Vec<(ev::RaidInfo, ev::RaidStopReason)> = stopped
            .into_iter()
            .filter(|(id, ..)| before.contains_key(id))
            .map(|(_, info, reason)| (info, reason))
            .collect();
        // Gone without being seen stopped: the `raids` rule turned off.
        let seen: HashSet<i32> = ended.iter().map(|e| e.0.id).collect();
        ended.extend(
            before
                .into_iter()
                .filter(|(id, _)| !running.contains_key(id) && !seen.contains(id))
                .map(|(_, info)| (info, ev::RaidStopReason::Other)),
        );
        for (raid, reason) in ended {
            let mut event = ev::RaidStopEvent {
                raid,
                world: world.clone(),
                reason,
            };
            self.plugin_fire(None, &mut event);
        }
    }

    // -----------------------------------------------------------------------
    // Game events and loot.

    /// A game event `event` at `pos` (`ctx` its source) is about to reach
    /// `radius` blocks: `GenericGameEvent`. Its reach, or None when cancelled.
    pub(super) fn plugin_game_event(
        &mut self,
        event: &str,
        pos: [f64; 3],
        ctx: &crate::hc::sculk::Context,
        radius: i32,
    ) -> Option<i32> {
        if !self.plugins_listen::<ev::GenericGameEvent>() {
            return Some(radius);
        }
        let mut e = ev::GenericGameEvent {
            event: event.to_owned(),
            location: self.plugin_location(pos),
            entity: ctx.source.and_then(|w| self.plugin_who_entity(w)),
            radius,
            cancelled: false,
        };
        self.plugin_fire(None, &mut e);
        (!e.cancelled).then_some(e.radius)
    }
    /// The player of `conn` is opening the containers of `slots`: for each
    /// one still holding its loot table's roll, seen for the first time,
    /// `LootGenerateEvent`. A plugin's loot replaces the roll; cancelled,
    /// the container is empty.
    pub(super) fn plugin_container_loot(&mut self, net: &Network, conn: u32, slots: &Slots) {
        if !self.plugins_listen::<ev::LootGenerateEvent>() {
            return;
        }
        let keys = match slots {
            Slots::Block(a) => vec![*a],
            Slots::Double(a, b) => vec![*a, *b],
            _ => return,
        };
        let player = Self::plugin_player_of(net, conn);
        for key in keys {
            let table = self
                .world
                .chunks
                .get(&(key.0 >> 4, key.2 >> 4))
                .and_then(|c| c.saved.as_ref())
                .and_then(|s| s.rolled_loot.iter().find(|r| r.pos == key))
                .map(|r| r.table.clone());
            let Some(table) = table else {
                continue;
            };
            if !self.plugin_world.looted.insert((self.dim, key)) {
                continue;
            }
            let Some(crate::hc::containers::BlockEntity::Chest(items)) =
                self.world.block_entities.get(&key)
            else {
                continue;
            };
            let old: Vec<ItemStack> = items.iter().flatten().cloned().collect();
            let loot: Vec<api::ItemStack> = old.iter().map(stack_ref).collect();
            let mut event = ev::LootGenerateEvent {
                world: self.dim.id().to_owned(),
                loot_table: table,
                block: Some(self.plugin_block([key.0, key.1, key.2])),
                entity: None,
                player: player.clone(),
                loot: loot.clone(),
                cancelled: false,
            };
            self.plugin_fire(Some(net), &mut event);
            if !event.cancelled && event.loot == loot {
                continue;
            }
            let new = if event.cancelled {
                Vec::new()
            } else {
                stacks_from(&event.loot, &old)
            };
            if let Some(crate::hc::containers::BlockEntity::Chest(items)) =
                self.world.block_entities.get_mut(&key)
            {
                let size = items.len();
                let mut new = new.into_iter();
                for slot in items.iter_mut().take(size) {
                    *slot = new.next();
                }
            }
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        }
    }
    /// The mob `m` died and dropped `drops`: `LootGenerateEvent` for its
    /// items. The drops to make (the experience stays).
    pub(super) fn plugin_mob_loot(&mut self, m: &Mob, drops: Vec<drops::Drop>) -> Vec<drops::Drop> {
        if !self.plugins_listen::<ev::LootGenerateEvent>() {
            return drops;
        }
        let old: Vec<horizon_world::loot::LootStack> = drops
            .iter()
            .filter_map(|d| match d {
                drops::Drop::Item(s) => Some(s.clone()),
                drops::Drop::Orb(_) => None,
            })
            .collect();
        let loot: Vec<api::ItemStack> = old
            .iter()
            .map(|s| api::ItemStack::new(&s.item, s.count.max(0) as u32))
            .collect();
        let kind = m.kind.id.to_string();
        let mut event = ev::LootGenerateEvent {
            world: self.dim.id().to_owned(),
            loot_table: format!(
                "minecraft:entities/{}",
                kind.trim_start_matches("minecraft:")
            ),
            block: None,
            entity: Some(self.plugin_entity(None, m.id as i32)),
            player: self.plugin_acting(),
            loot: loot.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if !event.cancelled && event.loot == loot {
            return drops;
        }
        let orbs = drops
            .into_iter()
            .filter(|d| matches!(d, drops::Drop::Orb(_)));
        if event.cancelled {
            return orbs.collect();
        }
        event
            .loot
            .iter()
            .filter(|s| s.count > 0)
            .map(|s| {
                let mut stack = old
                    .iter()
                    .find(|o| o.item == s.item)
                    .cloned()
                    .unwrap_or_else(|| horizon_world::loot::LootStack {
                        item: s.item.clone(),
                        count: 1,
                        damage: 0,
                        enchantments: Vec::new(),
                        potion: None,
                        map: false,
                        exploration: None,
                        ominous_amplifier: None,
                        stew_effects: Vec::new(),
                        names: Vec::new(),
                    });
                stack.count = s.count as i32;
                drops::Drop::Item(stack)
            })
            .chain(orbs)
            .collect()
    }
}
