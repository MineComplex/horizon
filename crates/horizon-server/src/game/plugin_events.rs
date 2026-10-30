//! Where the server fires the plugins' events beyond the first ones in
//! `plugin_host.rs` (docs/plugins.md): a player's packets before the game
//! handles them, what changes about a player from one tick to the next, the
//! player's own actions (teleports, eating, beds, buckets, bone meal, menus,
//! pickups), mobs coming and going, explosions, and the blocks that change by
//! themselves (pistons, fluids, growth, decay). A cancelled action is undone
//! on the player's client: the block, the slot or the menu is sent again.
use super::server_net::Network;
use crate::hc::items::ItemStack;
use horizon_core::{geometry::Vec3, position::BlockPos as Pos};
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};
use horizon_server_api::{
    self as api, BlockPos, Location, PlayerRef,
    event::{self as ev, Event},
};

/// What the plugins last saw of a player, for the events of a change
/// between two ticks.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Watch {
    sneaking: bool,
    sprinting: bool,
    flying: bool,
    bed: Option<[i32; 3]>,
    world: String,
    level: i32,
    points: i32,
    food: i32,
    health: f32,
    vehicle: Option<u32>,
}

/// A game stack as plugins see it.
pub(super) fn stack_ref(s: &ItemStack) -> api::ItemStack {
    api::ItemStack::new(&s.item, s.count)
}

fn hand_of(hand: i32) -> ev::Hand {
    if hand == 1 {
        ev::Hand::Off
    } else {
        ev::Hand::Main
    }
}

impl crate::game::Server<'_> {
    /// The block at `pos` of `world` (its state when it is this level's).
    pub(super) fn plugin_block_in(&self, world: &str, pos: [i32; 3]) -> api::Block {
        let state = if world == self.dim.id() {
            self.state_string(self.world.block(pos[0], pos[1], pos[2]))
        } else {
            String::new()
        };
        api::Block {
            world: world.to_owned(),
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            state,
        }
    }
    /// The block at `pos` of this level.
    pub(super) fn plugin_block(&self, pos: [i32; 3]) -> api::Block {
        self.plugin_block_in(self.dim.id(), pos)
    }
    pub(super) fn plugin_level_block(&self, pos: Pos) -> api::Block {
        self.plugin_block([pos.x, pos.y, pos.z])
    }
    /// The player of `conn`, from the hub in hand.
    pub(super) fn plugin_player_of(net: &Network, conn: u32) -> Option<PlayerRef> {
        net.hub.player(conn).map(crate::game::Server::plugin_player)
    }
    /// The player whose turn it is gets their inventory again (a cancelled
    /// action their client predicted).
    pub(super) fn plugin_resync_acting(&mut self) {
        if let Some(conn) = self.net_acting {
            self.plugin_resync.push(conn);
        }
    }
    /// `callEvent` for an event that may be cancelled: whether it went ahead.
    pub(super) fn plugin_allows<E: Event>(&mut self, net: Option<&Network>, mut event: E) -> bool {
        self.plugin_fire(net, &mut event);
        !event.cancelled()
    }

    // -----------------------------------------------------------------------
    // A player's packets.

    /// A player's packet, before the game handles it: the events it means.
    /// True when a plugin cancelled it (the game then drops it). What a
    /// plugin changed in it (a sign's lines) is written back.
    pub(super) fn plugin_packet(&mut self, net: &mut Network, conn: u32, packet: &mut S) -> bool {
        if self.plugins.is_none() {
            return false;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return false;
        };
        let world = player.location.world.clone();
        match packet {
            S::SetCarriedItem(p) if self.plugins_listen::<ev::PlayerItemHeldEvent>() => {
                let previous = net.hub.player(conn).map_or(0, |g| g.selected) as u8;
                let new_slot = p.slot.clamp(0, 8) as u8;
                if previous == new_slot {
                    return false;
                }
                let event = ev::PlayerItemHeldEvent {
                    player,
                    previous_slot: previous,
                    new_slot,
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                net.hub.send_to(
                    conn,
                    &C::SetHeldSlot(play::SetHeldSlot {
                        slot: i32::from(previous),
                    }),
                );
                true
            }
            S::Interact(p) if self.plugins_listen::<ev::PlayerInteractEntityEvent>() => {
                let event = ev::PlayerInteractEntityEvent {
                    player,
                    entity: self.plugin_entity(Some(net), p.id),
                    hand: hand_of(p.hand),
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                self.plugin_resync.push(conn);
                true
            }
            // SWAP_ITEM_WITH_OFFHAND.
            S::PlayerAction(a)
                if a.action == 7 && self.plugins_listen::<ev::PlayerSwapHandItemsEvent>() =>
            {
                let Some(g) = net.hub.player(conn) else {
                    return false;
                };
                let held = g.inventory.get(g.selected).cloned().flatten();
                let event = ev::PlayerSwapHandItemsEvent {
                    player,
                    main_hand_item: g.offhand.as_ref().map(stack_ref),
                    off_hand_item: held.as_ref().map(stack_ref),
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                self.plugin_resync.push(conn);
                true
            }
            // START_DESTROY_BLOCK: the dig begins.
            S::PlayerAction(a)
                if a.action == 0 && self.plugins_listen::<ev::BlockDamageEvent>() =>
            {
                let held = net
                    .hub
                    .player(conn)
                    .and_then(|g| g.inventory.get(g.selected).cloned().flatten());
                let event = ev::BlockDamageEvent {
                    player,
                    block: self.plugin_block_in(&world, a.pos),
                    item: held.as_ref().map(stack_ref),
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                net.hub.ack(conn, a.sequence);
                let state = self.world.block(a.pos[0], a.pos[1], a.pos[2]);
                self.plugin_resend.push((conn, a.pos, state.0 as i32));
                true
            }
            // The result slot of a crafting grid: CraftItemEvent.
            S::ContainerClick(p)
                if p.slot == 0
                    && self.plugins_listen::<ev::CraftItemEvent>()
                    && matches!(
                        net.hub.menu_view(conn).0.as_str(),
                        "minecraft:crafting" | "minecraft:inventory"
                    )
                    && net.hub.menu_slot_item(conn, &self.world, 0).is_some() =>
            {
                let (kind, _, size) = net.hub.menu_view(conn);
                let Some(result) = net.hub.menu_slot_item(conn, &self.world, 0) else {
                    return false;
                };
                let event = ev::CraftItemEvent {
                    player,
                    view: ev::InventoryView { kind, size },
                    result: stack_ref(&result),
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                net.hub.resend_menu(conn, &self.world);
                true
            }
            S::ContainerClick(p) if self.plugins_listen::<ev::InventoryClickEvent>() => {
                let (kind, _, size) = net.hub.menu_view(conn);
                let current = (p.slot >= 0)
                    .then(|| net.hub.menu_slot_item(conn, &self.world, p.slot as usize))
                    .flatten();
                let cursor = net.hub.player(conn).and_then(|g| g.carried.clone());
                let event = ev::InventoryClickEvent {
                    player,
                    view: ev::InventoryView { kind, size },
                    slot: i32::from(p.slot),
                    button: p.button.max(0) as u8,
                    click: ev::ClickType::from_id(p.input),
                    current: current.as_ref().map(stack_ref),
                    cursor: cursor.as_ref().map(stack_ref),
                    cancelled: false,
                };
                if self.plugin_allows(Some(net), event) {
                    return false;
                }
                net.hub.resend_menu(conn, &self.world);
                true
            }
            S::ContainerClose(_) if self.plugins_listen::<ev::InventoryCloseEvent>() => {
                let (kind, _, size) = net.hub.menu_view(conn);
                let mut event = ev::InventoryCloseEvent {
                    player,
                    view: ev::InventoryView { kind, size },
                };
                self.plugin_fire(Some(net), &mut event);
                false
            }
            S::SignUpdate(p) if self.plugins_listen::<ev::SignChangeEvent>() => {
                let mut event = ev::SignChangeEvent {
                    player,
                    block: self.plugin_block_in(&world, p.pos),
                    lines: p.lines.to_vec(),
                    front: p.front,
                    cancelled: false,
                };
                self.plugin_fire(Some(net), &mut event);
                if event.cancelled {
                    // The client shows the text it typed: the sign goes back.
                    let state = self.world.block(p.pos[0], p.pos[1], p.pos[2]);
                    self.plugin_resend.push((conn, p.pos, state.0 as i32));
                    return true;
                }
                for (k, line) in event.lines.into_iter().take(4).enumerate() {
                    p.lines[k] = line;
                }
                false
            }
            S::ResourcePack(p) if self.plugins_listen::<ev::PlayerResourcePackStatusEvent>() => {
                let mut event = ev::PlayerResourcePackStatusEvent {
                    player,
                    status: ev::ResourcePackStatus::from_id(p.action),
                };
                self.plugin_fire(Some(net), &mut event);
                false
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // What changed about the players since the last tick.

    fn plugins_watch_players(&self) -> bool {
        self.plugins_listen::<ev::PlayerToggleSneakEvent>()
            || self.plugins_listen::<ev::PlayerToggleSprintEvent>()
            || self.plugins_listen::<ev::PlayerToggleFlightEvent>()
            || self.plugins_listen::<ev::PlayerBedLeaveEvent>()
            || self.plugins_listen::<ev::PlayerChangedWorldEvent>()
            || self.plugins_listen::<ev::PlayerLevelChangeEvent>()
            || self.plugins_listen::<ev::PlayerExpChangeEvent>()
            || self.plugins_listen::<ev::FoodLevelChangeEvent>()
            || self.plugins_listen::<ev::EntityRegainHealthEvent>()
            || self.plugins_listen::<ev::VehicleEnterEvent>()
            || self.plugins_listen::<ev::VehicleExitEvent>()
    }
    /// Once a tick: `PlayerToggleSneakEvent`, `PlayerToggleSprintEvent`,
    /// `PlayerToggleFlightEvent` (cancelled: back as they were),
    /// `PlayerBedLeaveEvent`, `PlayerChangedWorldEvent` and
    /// `PlayerLevelChangeEvent` for what changed since the last.
    pub(super) fn plugin_watch(&mut self, net: &mut Network) {
        if !self.plugins_watch_players() {
            self.plugin_watched.clear();
            return;
        }
        let live: std::collections::HashSet<u32> = net
            .hub
            .players
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.conn)
            .collect();
        self.plugin_watched.retain(|c, _| live.contains(c));
        for i in 0..net.hub.players.len() {
            let g = &net.hub.players[i];
            if !g.placed {
                continue;
            }
            let conn = g.conn;
            let now = Watch {
                sneaking: g.sneaking,
                sprinting: g.sprinting,
                flying: g.flying,
                bed: g.bed,
                world: g.dimension.clone(),
                level: g.xp_level,
                points: g.xp_total,
                food: g.food,
                health: g.health,
                vehicle: g.vehicle,
            };
            let Some(before) = self.plugin_watched.insert(conn, now.clone()) else {
                continue;
            };
            if before == now {
                continue;
            }
            let player = crate::game::Server::plugin_player(g);
            if before.sneaking != now.sneaking {
                let mut e = ev::PlayerToggleSneakEvent {
                    player: player.clone(),
                    sneaking: now.sneaking,
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if before.sprinting != now.sprinting {
                let mut e = ev::PlayerToggleSprintEvent {
                    player: player.clone(),
                    sprinting: now.sprinting,
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if before.flying != now.flying {
                let e = ev::PlayerToggleFlightEvent {
                    player: player.clone(),
                    flying: now.flying,
                    cancelled: false,
                };
                if !self.plugin_allows(Some(net), e) {
                    let g = &mut net.hub.players[i];
                    g.flying = before.flying;
                    let flags = g.ability_flags();
                    g.send(&C::PlayerAbilities(play::PlayerAbilities {
                        flags,
                        flying_speed: 0.05,
                        walking_speed: 0.1,
                    }));
                    if let Some(w) = self.plugin_watched.get_mut(&conn) {
                        w.flying = before.flying;
                    }
                }
            }
            if let Some(bed) = before.bed
                && now.bed.is_none()
            {
                let mut e = ev::PlayerBedLeaveEvent {
                    player: player.clone(),
                    bed: self.plugin_block_in(&before.world, bed),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if before.world != now.world {
                let mut e = ev::PlayerChangedWorldEvent {
                    player: player.clone(),
                    from: before.world.clone(),
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if before.food != now.food {
                let e = ev::FoodLevelChangeEvent {
                    player: player.clone(),
                    food_level: now.food,
                    cancelled: false,
                };
                if !self.plugin_allows(Some(net), e) {
                    net.hub.players[i].food = before.food;
                    if let Some(w) = self.plugin_watched.get_mut(&conn) {
                        w.food = before.food;
                    }
                }
            }
            if now.health > before.health && before.health > 0.0 {
                let entity_id = net.hub.players[i].entity_id;
                let e = ev::EntityRegainHealthEvent {
                    entity: api::EntityRef::of_player(entity_id, player.clone()),
                    amount: f64::from(now.health - before.health),
                    cancelled: false,
                };
                if !self.plugin_allows(Some(net), e) {
                    net.hub.players[i].health = before.health;
                    if let Some(w) = self.plugin_watched.get_mut(&conn) {
                        w.health = before.health;
                    }
                }
            }
            if before.vehicle != now.vehicle {
                if let Some(id) = before.vehicle {
                    let mut e = ev::VehicleExitEvent {
                        vehicle: self.plugin_entity(Some(net), id as i32),
                        exited: player.clone(),
                    };
                    self.plugin_fire(Some(net), &mut e);
                }
                if let Some(id) = now.vehicle {
                    let mut e = ev::VehicleEnterEvent {
                        vehicle: self.plugin_entity(Some(net), id as i32),
                        entered: player.clone(),
                    };
                    self.plugin_fire(Some(net), &mut e);
                }
            }
            if before.points != now.points {
                let mut e = ev::PlayerExpChangeEvent {
                    player: player.clone(),
                    amount: now.points - before.points,
                };
                self.plugin_fire(Some(net), &mut e);
            }
            if before.level != now.level {
                let mut e = ev::PlayerLevelChangeEvent {
                    player,
                    old_level: before.level,
                    new_level: now.level,
                };
                self.plugin_fire(Some(net), &mut e);
            }
        }
    }

    /// `PlayerLoginEvent` for a player just placed: false when a plugin
    /// turned them away (they are kicked with its message).
    pub(super) fn plugin_login(&mut self, net: &mut Network, conn: u32) -> bool {
        if !self.plugins_listen::<ev::PlayerLoginEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let mut event = ev::PlayerLoginEvent {
            player,
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
        net.hub.kick(
            conn,
            crate::hc::server_net::data::component_nbt(
                &crate::hc::text::Text::legacy(&reason).component(),
            ),
        );
        false
    }

    // -----------------------------------------------------------------------
    // The player whose turn it is.

    /// The player whose turn it is is about to be moved to `to` in their
    /// level: `PlayerTeleportEvent`. Where to put them (a plugin may change
    /// it), or None when cancelled.
    pub(super) fn plugin_teleport(
        &mut self,
        to: Vec3,
        rotation: Option<[f32; 2]>,
        cause: ev::TeleportCause,
    ) -> Option<(Vec3, Option<[f32; 2]>)> {
        if !self.plugins_listen::<ev::PlayerTeleportEvent>() {
            return Some((to, rotation));
        }
        let Some(player) = self.plugin_acting() else {
            return Some((to, rotation));
        };
        let from = player.location.clone();
        let [yaw, pitch] = rotation.unwrap_or([from.yaw, from.pitch]);
        let target = Location {
            world: from.world.clone(),
            x: to.x,
            y: to.y,
            z: to.z,
            yaw,
            pitch,
        };
        let mut event = ev::PlayerTeleportEvent {
            player,
            from,
            to: target.clone(),
            cause,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            None
        } else if event.to != target {
            Some((
                Vec3::new(event.to.x, event.to.y, event.to.z),
                Some([event.to.yaw, event.to.pitch]),
            ))
        } else {
            Some((to, rotation))
        }
    }
    /// The player whose turn it is finished eating or drinking `stack`:
    /// `PlayerItemConsumeEvent`. False when cancelled.
    pub(super) fn plugin_consume(&mut self, stack: &ItemStack) -> bool {
        if !self.plugins_listen::<ev::PlayerItemConsumeEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerItemConsumeEvent {
            player,
            item: stack_ref(stack),
            cancelled: false,
        };
        if self.plugin_allows(None, event) {
            return true;
        }
        self.plugin_resync_acting();
        false
    }
    /// The player whose turn it is is about to lie down in the bed at
    /// `head`: `PlayerBedEnterEvent`. False when cancelled.
    pub(super) fn plugin_bed_enter(&mut self, head: [i32; 3]) -> bool {
        if !self.plugins_listen::<ev::PlayerBedEnterEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerBedEnterEvent {
            player,
            bed: self.plugin_block(head),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The player whose turn it is is about to fill the held `bucket` from
    /// `pos` (`fill`) or empty it into `pos`: `PlayerBucketFillEvent` /
    /// `PlayerBucketEmptyEvent`. False when cancelled (the block and the
    /// bucket go to their client again).
    pub(super) fn plugin_bucket(&mut self, fill: bool, pos: [i32; 3], bucket: &ItemStack) -> bool {
        let listens = if fill {
            self.plugins_listen::<ev::PlayerBucketFillEvent>()
        } else {
            self.plugins_listen::<ev::PlayerBucketEmptyEvent>()
        };
        if !listens {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let (block, bucket) = (self.plugin_block(pos), stack_ref(bucket));
        let allowed = if fill {
            self.plugin_allows(
                None,
                ev::PlayerBucketFillEvent {
                    player,
                    block,
                    bucket,
                    cancelled: false,
                },
            )
        } else {
            self.plugin_allows(
                None,
                ev::PlayerBucketEmptyEvent {
                    player,
                    block,
                    bucket,
                    cancelled: false,
                },
            )
        };
        if !allowed {
            self.plugin_resend_acting(pos);
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The block at `pos` goes to the client of the player whose turn it is
    /// again, as it is.
    pub(super) fn plugin_resend_acting(&mut self, pos: [i32; 3]) {
        if let Some(conn) = self.net_acting {
            let state = self.world.block(pos[0], pos[1], pos[2]);
            self.plugin_resend.push((conn, pos, state.0 as i32));
        }
    }
    /// Whether plugins hear of bone meal (`BlockFertilizeEvent`).
    pub(super) fn plugins_hear_fertilize(&self) -> bool {
        self.plugins_listen::<ev::BlockFertilizeEvent>()
    }
    /// Bone meal about to be used on `target` by the player whose turn it
    /// is, or a dispenser (`by_player` false): `BlockFertilizeEvent`. False
    /// when cancelled.
    pub(super) fn plugin_fertilize(&mut self, target: Pos, by_player: bool) -> bool {
        if !self.plugins_hear_fertilize() {
            return true;
        }
        let player = if by_player {
            self.plugin_acting()
        } else {
            None
        };
        let event = ev::BlockFertilizeEvent {
            player,
            block: self.plugin_level_block(target),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed && by_player {
            self.plugin_resync_acting();
        }
        allowed
    }

    /// The player whose turn it is completed `advancement`:
    /// `PlayerAdvancementDoneEvent`.
    pub(super) fn plugin_advancement_done(&mut self, advancement: &str) {
        if !self.plugins_listen::<ev::PlayerAdvancementDoneEvent>() {
            return;
        }
        let Some(player) = self.plugin_acting() else {
            return;
        };
        let mut event = ev::PlayerAdvancementDoneEvent {
            player,
            advancement: advancement.to_owned(),
        };
        self.plugin_fire(None, &mut event);
    }

    // -----------------------------------------------------------------------
    // Menus and items.

    /// The player of `conn` is about to open a `kind` menu with `size` slots
    /// of its own: `InventoryOpenEvent`. False when cancelled.
    pub(super) fn plugin_inventory_open(
        &mut self,
        net: &Network,
        conn: u32,
        kind: &str,
        size: usize,
    ) -> bool {
        if !self.plugins_listen::<ev::InventoryOpenEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::InventoryOpenEvent {
            player,
            view: ev::InventoryView {
                kind: kind.to_owned(),
                size: size as u32,
            },
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }
    /// The player of `conn` is about to pick up the dropped item `id`
    /// holding `stack`: `PlayerPickupItemEvent`. False when cancelled.
    pub(super) fn plugin_pickup(
        &mut self,
        net: &Network,
        conn: u32,
        id: u32,
        at: Vec3,
        stack: &ItemStack,
    ) -> bool {
        if !self.plugins_listen::<ev::PlayerPickupItemEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::PlayerPickupItemEvent {
            player,
            item: stack_ref(stack),
            entity: api::EntityRef {
                id: id as i32,
                kind: "minecraft:item".into(),
                location: Location::new(self.dim.id(), at.x, at.y, at.z),
                player: None,
            },
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }

    // -----------------------------------------------------------------------
    // Mobs and explosions.

    /// A mob about to come into the world: `CreatureSpawnEvent`. False when
    /// a plugin cancelled it.
    pub(super) fn plugin_spawn_allowed(
        &mut self,
        mob: &crate::hc::mobs::Mob,
        reason: ev::SpawnReason,
    ) -> bool {
        if !self.plugins_listen::<ev::CreatureSpawnEvent>() {
            return true;
        }
        let location = Location {
            world: self.dim.id().to_owned(),
            x: mob.position.x,
            y: mob.position.y,
            z: mob.position.z,
            yaw: mob.yaw,
            pitch: 0.0,
        };
        let event = ev::CreatureSpawnEvent {
            entity: api::EntityRef {
                id: mob.id as i32,
                kind: crate::hc::command::args::normalize(&mob.kind.id),
                location: location.clone(),
                player: None,
            },
            location,
            reason,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// A mob died: `EntityDeathEvent`, with the player who killed it (the
    /// connection `killer`).
    pub(super) fn plugin_mob_died(&mut self, mob: &crate::hc::mobs::Mob, killer: Option<u32>) {
        if !self.plugins_listen::<ev::EntityDeathEvent>() {
            return;
        }
        let location = Location {
            world: self.dim.id().to_owned(),
            x: mob.position.x,
            y: mob.position.y,
            z: mob.position.z,
            yaw: mob.yaw,
            pitch: 0.0,
        };
        let killer = killer.and_then(|conn| {
            self.network
                .as_ref()
                .and_then(|net| Self::plugin_player_of(net, conn))
        });
        let mut event = ev::EntityDeathEvent {
            entity: api::EntityRef {
                id: mob.id as i32,
                kind: crate::hc::command::args::normalize(&mob.kind.id),
                location: location.clone(),
                player: None,
            },
            location,
            killer,
        };
        self.plugin_fire(None, &mut event);
    }
    /// An explosion of `entity` about to happen at `center`, destroying
    /// `blocks`: `EntityExplodeEvent`. False when cancelled; `blocks` keeps
    /// only those the plugins left.
    pub(super) fn plugin_explosion(
        &mut self,
        center: [f64; 3],
        entity: Option<u32>,
        blocks: &mut Vec<[i32; 3]>,
    ) -> bool {
        if !self.plugins_listen::<ev::EntityExplodeEvent>() {
            return true;
        }
        let mut event = ev::EntityExplodeEvent {
            entity: entity.map(|id| self.plugin_entity(None, id as i32)),
            location: Location::new(self.dim.id(), center[0], center[1], center[2]),
            blocks: blocks
                .iter()
                .map(|p| BlockPos::new(p[0], p[1], p[2]))
                .collect(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            return false;
        }
        let keep: std::collections::HashSet<[i32; 3]> =
            event.blocks.iter().map(|b| [b.x, b.y, b.z]).collect();
        blocks.retain(|p| keep.contains(p));
        true
    }

    // -----------------------------------------------------------------------
    // Blocks that change by themselves.

    /// Plugins hear of blocks growing, spreading, forming, fading, decaying
    /// and burning (`LevelEvent::Changed`, `LevelEvent::Pending`).
    pub(super) fn plugins_hear_block_changes(&self) -> bool {
        !self.plugin_reverting
            && (self.plugins_listen::<ev::BlockGrowEvent>()
                || self.plugins_listen::<ev::BlockSpreadEvent>()
                || self.plugins_listen::<ev::BlockFormEvent>()
                || self.plugins_listen::<ev::BlockFadeEvent>()
                || self.plugins_listen::<ev::LeavesDecayEvent>()
                || self.plugins_listen::<ev::BlockBurnEvent>())
    }
    /// Plugins hear of pistons moving.
    pub(super) fn plugins_hear_pistons(&self) -> bool {
        self.plugins_listen::<ev::BlockPistonExtendEvent>()
            || self.plugins_listen::<ev::BlockPistonRetractEvent>()
    }
    /// Plugins hear of fluids flowing (`BlockFromToEvent`).
    pub(super) fn plugins_hear_flows(&self) -> bool {
        self.plugins_listen::<ev::BlockFromToEvent>()
    }
    /// A piston about to move: `BlockPistonExtendEvent` or
    /// `BlockPistonRetractEvent`. False when a plugin cancelled it.
    pub(super) fn plugin_piston(&mut self, mv: &horizon_world::level::PistonMove) -> bool {
        let block = self.plugin_level_block(mv.pos);
        let direction = ev::BlockFace::from_id(mv.direction as i32);
        let blocks: Vec<BlockPos> = mv
            .blocks
            .iter()
            .map(|p| BlockPos::new(p.x, p.y, p.z))
            .collect();
        if mv.extending {
            self.plugin_allows(
                None,
                ev::BlockPistonExtendEvent {
                    block,
                    direction,
                    sticky: mv.sticky,
                    blocks,
                    cancelled: false,
                },
            )
        } else {
            self.plugin_allows(
                None,
                ev::BlockPistonRetractEvent {
                    block,
                    direction,
                    sticky: mv.sticky,
                    blocks,
                    cancelled: false,
                },
            )
        }
    }
    /// A fluid about to flow: `BlockFromToEvent` for each block it flows
    /// into. The blocks the plugins kept it out of.
    pub(super) fn plugin_fluid_flow(&mut self, flow: &horizon_world::level::FluidFlow) -> Vec<Pos> {
        let from = self.plugin_level_block(flow.from);
        let ask = |game: &mut Self, to: Pos| {
            let d = [to.x - flow.from.x, to.y - flow.from.y, to.z - flow.from.z];
            let event = ev::BlockFromToEvent {
                block: from.clone(),
                to: game.plugin_level_block(to),
                face: ev::BlockFace::from_offset(d),
                cancelled: false,
            };
            !game.plugin_allows(None, event)
        };
        if let Some(down) = flow.down
            && ask(self, down)
        {
            // FlowingFluid.spread stops there: no flow to the sides.
            return vec![down];
        }
        let mut vetoes = Vec::new();
        for &to in &flow.sides {
            if ask(self, to) {
                vetoes.push(to);
            }
        }
        vetoes
    }
    /// A change the level held back for the plugins: `LeavesDecayEvent`,
    /// `BlockBurnEvent`, `BlockFadeEvent` (snow melting) or `BlockFormEvent`
    /// (lava meeting water). True when it may happen.
    pub(super) fn plugin_pending(&mut self, change: horizon_world::level::PendingChange) -> bool {
        use horizon_world::level::PendingChange as P;
        let (pos, _) = change.at();
        let block = self.plugin_level_block(pos);
        match change {
            P::LeavesDecay { .. } => self.plugin_allows(
                None,
                ev::LeavesDecayEvent {
                    block,
                    cancelled: false,
                },
            ),
            P::Burn { source, .. } => {
                let igniting = self.plugin_level_block(source);
                self.plugin_allows(
                    None,
                    ev::BlockBurnEvent {
                        block,
                        igniting,
                        cancelled: false,
                    },
                )
            }
            P::SnowMelt { .. } => self.plugin_allows(
                None,
                ev::BlockFadeEvent {
                    block,
                    new_state: "minecraft:air".into(),
                    cancelled: false,
                },
            ),
            P::Form { into, .. } => {
                let new_state = self.state_string(into);
                self.plugin_allows(
                    None,
                    ev::BlockFormEvent {
                        block,
                        new_state,
                        cancelled: false,
                    },
                )
            }
        }
    }
    /// Blocks changed by themselves: `BlockGrowEvent`, `BlockSpreadEvent`,
    /// `BlockFormEvent` or `BlockFadeEvent`. A cancelled change puts back the
    /// blocks it set (those nothing changed since).
    pub(super) fn plugin_block_changed(
        &mut self,
        cause: horizon_world::level::BlockChange,
        changes: &[(
            Pos,
            horizon_world::registry::BlockStateId,
            horizon_world::registry::BlockStateId,
        )],
    ) -> crate::hc::Result<()> {
        let Some(&(pos, old, new)) = changes.first() else {
            return Ok(());
        };
        let mut block = self.plugin_level_block(pos);
        block.state = self.state_string(old);
        let new_state = self.state_string(new);
        use horizon_world::level::BlockChange as B;
        let allowed = match cause {
            B::Grow => self.plugin_allows(
                None,
                ev::BlockGrowEvent {
                    block,
                    new_state,
                    cancelled: false,
                },
            ),
            B::Form => self.plugin_allows(
                None,
                ev::BlockFormEvent {
                    block,
                    new_state,
                    cancelled: false,
                },
            ),
            B::Fade => self.plugin_allows(
                None,
                ev::BlockFadeEvent {
                    block,
                    new_state,
                    cancelled: false,
                },
            ),
            B::Spread { source } => {
                let source = self.plugin_level_block(source);
                self.plugin_allows(
                    None,
                    ev::BlockSpreadEvent {
                        block,
                        source,
                        new_state,
                        cancelled: false,
                    },
                )
            }
        };
        if allowed {
            return Ok(());
        }
        // No events for the blocks put back.
        let was = std::mem::replace(&mut self.plugin_reverting, true);
        let mut result = Ok(());
        for &(pos, old, new) in changes.iter().rev() {
            if self.world.block(pos.x, pos.y, pos.z) == new
                && let Err(e) = self.level_set([pos.x, pos.y, pos.z], old)
            {
                result = Err(e);
            }
        }
        self.plugin_reverting = was;
        result
    }
}

/// What the plugins last saw of a mob: its target and whether it was tame.
pub(super) type MobWatch = (Option<crate::hc::mobs::Who>, bool);

impl crate::game::Server<'_> {
    /// Whether plugins hear of mobs' targets and taming.
    fn plugins_watch_mobs(&self) -> bool {
        self.plugins_listen::<ev::EntityTargetLivingEntityEvent>()
            || self.plugins_listen::<ev::EntityTameEvent>()
    }
    /// The entity a mob's `who` names: another mob, or the player it sees.
    pub(super) fn plugin_who(&self, mob: u32, who: crate::hc::mobs::Who) -> Option<api::EntityRef> {
        match who {
            crate::hc::mobs::Who::Mob(id) => Some(self.plugin_entity(None, id as i32)),
            crate::hc::mobs::Who::Player => {
                let net = self.network.as_ref()?;
                let conn = net.mob_players.get(&mob)?;
                let g = net.hub.player(*conn)?;
                Some(api::EntityRef::of_player(
                    g.entity_id,
                    crate::game::Server::plugin_player(g),
                ))
            }
        }
    }
    /// Once a tick, after the mobs ticked: `EntityTargetLivingEntityEvent`
    /// for each mob whose target changed and `EntityTameEvent` for each mob
    /// tamed; cancelled ones go back.
    pub(super) fn plugin_watch_mobs(&mut self) {
        if !self.plugins_watch_mobs() {
            self.plugin_mob_watch.clear();
            return;
        }
        let dim = self.dim;
        let live: std::collections::HashSet<u32> = self.mobs.iter().map(|m| m.id).collect();
        self.plugin_mob_watch
            .retain(|(d, id), _| *d != dim || live.contains(id));
        let now: Vec<(u32, MobWatch)> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| (m.id, (m.ai.target, m.extra.tame)))
            .collect();
        for (id, (target, tame)) in now {
            let Some((old_target, old_tame)) =
                self.plugin_mob_watch.insert((dim, id), (target, tame))
            else {
                continue;
            };
            if old_target != target {
                let event = ev::EntityTargetLivingEntityEvent {
                    entity: self.plugin_entity(None, id as i32),
                    target: target.and_then(|t| self.plugin_who(id, t)),
                    cancelled: false,
                };
                if !self.plugin_allows(None, event)
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    m.ai.target = old_target;
                    self.plugin_mob_watch.insert((dim, id), (old_target, tame));
                }
            }
            if tame && !old_tame {
                let owner = self
                    .mobs
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| m.extra.owner)
                    .map(crate::hc::server_net::playerdata::ints_uuid)
                    .and_then(|uuid| self.plugin_seen.iter().find(|p| p.uuid == uuid).cloned());
                let event = ev::EntityTameEvent {
                    entity: self.plugin_entity(None, id as i32),
                    owner,
                    cancelled: false,
                };
                if !self.plugin_allows(None, event)
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    crate::hc::mobs::tame::set_tame(m, false);
                    m.extra.owner = None;
                    let target = m.ai.target;
                    self.plugin_mob_watch.insert((dim, id), (target, false));
                }
            }
        }
    }
    /// The entity that shot or threw projectile `p`.
    pub(super) fn plugin_shooter(
        &self,
        p: &crate::hc::projectiles::Projectile,
    ) -> Option<api::EntityRef> {
        if let Some(conn) = p.player {
            let net = self.network.as_ref()?;
            let g = net.hub.player(conn)?;
            return Some(api::EntityRef::of_player(
                g.entity_id,
                crate::game::Server::plugin_player(g),
            ));
        }
        match p.owner? {
            crate::hc::mobs::Who::Mob(id) => Some(self.plugin_entity(None, id as i32)),
            crate::hc::mobs::Who::Player => None,
        }
    }
    /// Before the projectiles tick: `ProjectileLaunchEvent` for each one new
    /// since the last; cancelled ones are removed.
    pub(super) fn plugin_projectiles_launched(&mut self) {
        if !self.plugins_listen::<ev::ProjectileLaunchEvent>() {
            self.plugin_projectiles_seen.clear();
            return;
        }
        let dim = self.dim;
        let live: std::collections::HashSet<u32> =
            self.creatures.projectiles.iter().map(|p| p.id).collect();
        self.plugin_projectiles_seen
            .retain(|(d, id)| *d != dim || live.contains(id));
        let fresh: Vec<crate::hc::projectiles::Projectile> = self
            .creatures
            .projectiles
            .iter()
            .filter(|p| p.id != 0 && !self.plugin_projectiles_seen.contains(&(dim, p.id)))
            .cloned()
            .collect();
        let mut removed = Vec::new();
        for p in fresh {
            self.plugin_projectiles_seen.insert((dim, p.id));
            let event = ev::ProjectileLaunchEvent {
                entity: self.plugin_entity(None, p.id as i32),
                shooter: self.plugin_shooter(&p),
                cancelled: false,
            };
            if !self.plugin_allows(None, event) {
                removed.push(p.id);
            }
        }
        if !removed.is_empty() {
            self.creatures
                .projectiles
                .retain(|p| !removed.contains(&p.id));
        }
    }
    /// A projectile hit something: `ProjectileHitEvent`. False when a plugin
    /// cancelled what the hit does.
    pub(super) fn plugin_projectile_hit(
        &mut self,
        p: &crate::hc::projectiles::Projectile,
        hit: crate::hc::projectiles::Hit,
        victim: Option<u32>,
    ) -> bool {
        use crate::hc::projectiles::Hit;
        if !self.plugins_listen::<ev::ProjectileHitEvent>() || matches!(hit, Hit::None) {
            return true;
        }
        let (hit_block, hit_entity) = match hit {
            Hit::Block { pos, .. } => (Some(self.plugin_block(pos)), None),
            Hit::Entity { who, .. } => {
                let entity = match (victim, who) {
                    (Some(conn), _) => self.network.as_ref().and_then(|net| {
                        let g = net.hub.player(conn)?;
                        Some(api::EntityRef::of_player(
                            g.entity_id,
                            crate::game::Server::plugin_player(g),
                        ))
                    }),
                    (None, crate::hc::mobs::Who::Mob(id)) => {
                        Some(self.plugin_entity(None, id as i32))
                    }
                    (None, crate::hc::mobs::Who::Player) => None,
                };
                (None, entity)
            }
            Hit::None => (None, None),
        };
        let mut entity = self.plugin_entity(None, p.id as i32);
        if entity.kind.is_empty() {
            entity.kind = p.kind.id().to_owned();
            entity.location =
                Location::new(self.dim.id(), p.position.x, p.position.y, p.position.z);
        }
        let event = ev::ProjectileHitEvent {
            entity,
            shooter: self.plugin_shooter(p),
            hit_block,
            hit_entity,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// A dropped item about to come into the world at `at`: `ItemSpawnEvent`.
    /// False when cancelled.
    pub(super) fn plugin_item_spawn(&mut self, stack: &ItemStack, at: [f64; 3]) -> bool {
        if !self.plugins_listen::<ev::ItemSpawnEvent>() {
            return true;
        }
        let event = ev::ItemSpawnEvent {
            item: stack_ref(stack),
            location: Location::new(self.dim.id(), at[0], at[1], at[2]),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The dispenser or dropper at `pos` is about to dispense `stack`:
    /// `BlockDispenseEvent`. False when cancelled.
    pub(super) fn plugin_dispense(&mut self, pos: Pos, stack: &ItemStack) -> bool {
        if !self.plugins_listen::<ev::BlockDispenseEvent>() {
            return true;
        }
        let event = ev::BlockDispenseEvent {
            block: self.plugin_level_block(pos),
            item: stack_ref(stack),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The animals `parent` and `partner` are about to breed:
    /// `EntityBreedEvent`. False when cancelled.
    pub(super) fn plugin_breed(&mut self, parent: u32, partner: u32, by: Option<[i32; 4]>) -> bool {
        if !self.plugins_listen::<ev::EntityBreedEvent>() {
            return true;
        }
        let breeder = by
            .map(crate::hc::server_net::playerdata::ints_uuid)
            .and_then(|uuid| self.plugin_seen.iter().find(|p| p.uuid == uuid).cloned());
        let event = ev::EntityBreedEvent {
            mother: self.plugin_entity(None, parent as i32),
            father: self.plugin_entity(None, partner as i32),
            breeder,
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The player whose turn it is is about to shear the mob `id`:
    /// `PlayerShearEntityEvent`. False when cancelled.
    pub(super) fn plugin_shear(&mut self, id: u32) -> bool {
        if !self.plugins_listen::<ev::PlayerShearEntityEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerShearEntityEvent {
            player,
            entity: self.plugin_entity(None, id as i32),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The player whose turn it is casts or reels in their rod:
    /// `PlayerFishEvent`. False when cancelled.
    pub(super) fn plugin_fish(&mut self, state: ev::FishState, caught: Option<u32>) -> bool {
        if !self.plugins_listen::<ev::PlayerFishEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let event = ev::PlayerFishEvent {
            player,
            state,
            caught: caught.map(|id| self.plugin_entity(None, id as i32)),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed {
            self.plugin_resync_acting();
        }
        allowed
    }
    /// The player of `conn` is about to enchant `item` with the offer
    /// `button`: `EnchantItemEvent`. False when cancelled.
    pub(super) fn plugin_enchant(
        &mut self,
        net: &Network,
        conn: u32,
        item: &ItemStack,
        button: usize,
    ) -> bool {
        if !self.plugins_listen::<ev::EnchantItemEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::EnchantItemEvent {
            player,
            item: stack_ref(item),
            button: button as u8,
            level_cost: button as i32 + 1,
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }
    /// The player of `conn` saved a book: `PlayerEditBookEvent`. The pages
    /// and title to write, or None when cancelled.
    pub(super) fn plugin_edit_book(
        &mut self,
        net: &Network,
        conn: u32,
        pages: Vec<String>,
        title: Option<String>,
    ) -> Option<(Vec<String>, Option<String>)> {
        if !self.plugins_listen::<ev::PlayerEditBookEvent>() {
            return Some((pages, title));
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return Some((pages, title));
        };
        let mut event = ev::PlayerEditBookEvent {
            player,
            pages,
            title,
            cancelled: false,
        };
        self.plugin_fire(Some(net), &mut event);
        if event.cancelled {
            self.plugin_resync.push(conn);
            return None;
        }
        Some((event.pages, event.title))
    }
    /// The player of `conn` is about to go through a portal to `to`:
    /// `PlayerPortalEvent`. False when cancelled.
    pub(super) fn plugin_portal(&mut self, net: &Network, conn: u32, to: &str) -> bool {
        if !self.plugins_listen::<ev::PlayerPortalEvent>() {
            return true;
        }
        let Some(player) = Self::plugin_player_of(net, conn) else {
            return true;
        };
        let event = ev::PlayerPortalEvent {
            from: player.location.clone(),
            player,
            to_world: to.to_owned(),
            cancelled: false,
        };
        self.plugin_allows(Some(net), event)
    }
    /// Once a tick in each level: `WeatherChangeEvent` and
    /// `ThunderChangeEvent` for what the weather did since the last; a
    /// cancelled change is taken back.
    pub(super) fn plugin_watch_weather(&mut self) {
        let listens = self.plugins_listen::<ev::WeatherChangeEvent>()
            || self.plugins_listen::<ev::ThunderChangeEvent>();
        let dim = self.dim;
        let now = (self.world.weather.raining, self.world.weather.thundering);
        let Some(before) = self.plugin_weather.insert(dim, now) else {
            return;
        };
        if !listens || before == now {
            return;
        }
        let world = dim.id().to_owned();
        if before.0 != now.0 {
            let event = ev::WeatherChangeEvent {
                world: world.clone(),
                to_raining: now.0,
                cancelled: false,
            };
            if !self.plugin_allows(None, event) {
                self.world.weather.raining = before.0;
            }
        }
        if before.1 != now.1 {
            let event = ev::ThunderChangeEvent {
                world,
                to_thundering: now.1,
                cancelled: false,
            };
            if !self.plugin_allows(None, event) {
                self.world.weather.thundering = before.1;
            }
        }
        let kept = (self.world.weather.raining, self.world.weather.thundering);
        self.plugin_weather.insert(dim, kept);
    }
    /// The level is being saved: `WorldSaveEvent`.
    pub(super) fn plugin_world_save(&mut self) {
        if !self.plugins_listen::<ev::WorldSaveEvent>() {
            return;
        }
        let mut event = ev::WorldSaveEvent {
            world: self.dim.id().to_owned(),
        };
        self.plugin_fire(None, &mut event);
    }
    /// The server finished starting: `ServerLoadEvent`.
    pub(super) fn plugin_server_loaded(&mut self) {
        if !self.plugins_listen::<ev::ServerLoadEvent>() {
            return;
        }
        let mut event = ev::ServerLoadEvent;
        self.plugin_fire(None, &mut event);
    }
}
