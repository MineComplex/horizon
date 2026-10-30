//! Where the server fires the plugins' blocks events (docs/plugins.md):
//! TNT being lit, note blocks and bells, fire appearing, beds and anchors
//! exploding, sponges, farmland, cauldrons, flowing fluids, redstone, blocks
//! of two parts placed, dispensers shearing and campfires cooking.
//!
//! The level's own changes (horizon-world's block behaviour) come as
//! `PluginAsk`s: a level whose plugins listen holds the change back
//! (`LevelAccess::hold_for_plugins`), and `plugin_block_ask` lets it happen
//! once they did not cancel it.
use crate::hc::{Result, items::ItemStack};
use horizon_core::position::BlockPos as Pos;
use horizon_server_api::{self as api, BlockPos, event as ev};
use horizon_world::{
    level::{PluginAsk, PluginAskKind},
    registry::BlockStateId,
};

/// What these events keep between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The level's changes held back for the plugins, in the order their
    /// `plugin_ask` level events come.
    pub(super) asked: Vec<PluginAsk>,
    /// The bed or anchor about to explode, as it was (`BlockExplodeEvent`).
    exploding: Option<api::Block>,
}

/// The inventory as it was before a cauldron was used, should a plugin
/// cancel `CauldronLevelChangeEvent`.
pub(super) struct HandBefore {
    inventory: Vec<Option<ItemStack>>,
    offhand: Option<ItemStack>,
    held: Option<ItemStack>,
}

impl crate::game::Server<'_> {
    /// The `PluginAskKind`s the level asks the plugins about first.
    pub(super) fn plugin_block_asks(&self) -> u32 {
        if self.plugins.is_none() || self.plugin_reverting {
            return 0;
        }
        let mut asks = 0;
        let mut ask = |listens: bool, kind: PluginAskKind| {
            if listens {
                asks |= kind.bit();
            }
        };
        ask(
            self.plugins_listen::<ev::TntPrimeEvent>(),
            PluginAskKind::TntPrime,
        );
        ask(
            self.plugins_listen::<ev::NotePlayEvent>(),
            PluginAskKind::NotePlay,
        );
        ask(
            self.plugins_listen::<ev::BellRingEvent>(),
            PluginAskKind::BellRing,
        );
        ask(
            self.plugins_listen::<ev::BlockIgniteEvent>(),
            PluginAskKind::Ignite,
        );
        ask(
            self.plugins_listen::<ev::SpongeAbsorbEvent>(),
            PluginAskKind::SpongeAbsorb,
        );
        ask(
            self.plugins_listen::<ev::MoistureChangeEvent>(),
            PluginAskKind::Moisture,
        );
        ask(
            self.plugins_listen::<ev::CauldronLevelChangeEvent>(),
            PluginAskKind::Cauldron,
        );
        ask(
            self.plugins_listen::<ev::BlockRedstoneEvent>(),
            PluginAskKind::Redstone,
        );
        asks
    }
    /// The entity of the player whose turn it is.
    fn plugin_acting_entity(&self) -> Option<api::EntityRef> {
        let player = self.plugin_acting()?;
        let id = self
            .net_acting
            .and_then(|conn| self.network.as_ref()?.hub.player(conn))
            .map(|g| g.entity_id)
            .or_else(|| {
                self.plugin_seen_ids
                    .iter()
                    .find(|(_, uuid)| *uuid == player.uuid)
                    .map(|(id, _)| *id)
            })
            .unwrap_or(-1);
        Some(api::EntityRef::of_player(id, player))
    }
    /// The block state `text` names (`minecraft:farmland[moisture=3]`).
    fn plugin_resolve_state(&self, text: &str) -> Option<BlockStateId> {
        let (name, props) = match text.split_once('[') {
            Some((name, rest)) => (name, rest.trim_end_matches(']')),
            None => (text, ""),
        };
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        let mut properties = std::collections::BTreeMap::new();
        for pair in props.split(',').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=')?;
            properties.insert(k.trim().to_owned(), v.trim().to_owned());
        }
        let registry = self.pack.assets.registry;
        let default = registry.default_state(&name)?;
        let mut state = registry.state(default)?.clone();
        state.properties.extend(properties);
        registry.resolve(&state).ok()
    }
    /// A changed `new_state` of an event, when it still names a block of the
    /// kind `accepts`.
    fn plugin_new_state(
        &self,
        given: &str,
        was: &str,
        accepts: impl Fn(&str) -> bool,
    ) -> Option<BlockStateId> {
        if given == was {
            return None;
        }
        let id = self.plugin_resolve_state(given)?;
        let name = self.pack.assets.registry.state(id)?.name.clone();
        accepts(&name).then_some(id)
    }

    // -----------------------------------------------------------------------
    // The level's changes, held back.

    /// The `plugin_ask` level event at `pos`: the change the level held back
    /// for it, fired as its event, and made unless a plugin cancelled it.
    pub(super) fn plugin_block_ask(&mut self, pos: Pos) -> Result<()> {
        let asked = &mut self.plugin_blocks.asked;
        let Some(i) = asked.iter().position(|a| a.pos() == pos) else {
            return Ok(());
        };
        let mut ask = asked.remove(i);
        // Asks whose level event never came (another level's, a fixture's
        // cleared events) don't pile up.
        if asked.len() > 256 {
            asked.drain(..asked.len() - 256);
        }
        let block = self.plugin_level_block(pos);
        let allowed = match &mut ask {
            PluginAsk::Redstone { old, new, .. } => {
                let mut event = ev::BlockRedstoneEvent {
                    block,
                    old_current: *old,
                    new_current: *new,
                };
                self.plugin_fire(None, &mut event);
                return Ok(());
            }
            PluginAsk::TntPrime { cause, .. } => {
                use horizon_world::level::TntCause;
                let (cause, priming_entity) = match cause {
                    TntCause::Redstone => (ev::PrimeCause::Redstone, None),
                    TntCause::Fire => (ev::PrimeCause::Fire, None),
                    TntCause::BlockBreak => {
                        (ev::PrimeCause::BlockBreak, self.plugin_acting_entity())
                    }
                };
                self.plugin_allows(
                    None,
                    ev::TntPrimeEvent {
                        block,
                        cause,
                        priming_entity,
                        priming_block: None,
                        cancelled: false,
                    },
                )
            }
            PluginAsk::NotePlay { state, .. } => {
                let props = self
                    .pack
                    .assets
                    .registry
                    .state(*state)
                    .map(|s| s.properties.clone())
                    .unwrap_or_default();
                let event = ev::NotePlayEvent {
                    block,
                    instrument: props.get("instrument").cloned().unwrap_or_default(),
                    note: props.get("note").and_then(|n| n.parse().ok()).unwrap_or(0),
                    cancelled: false,
                };
                self.plugin_allows(None, event)
            }
            PluginAsk::BellRing { direction, .. } => {
                let event = ev::BellRingEvent {
                    block,
                    direction: ev::BlockFace::from_id(*direction as i32),
                    entity: self.plugin_acting_entity(),
                    cancelled: false,
                };
                self.plugin_allows(None, event)
            }
            PluginAsk::Ignite { cause, source, .. } => {
                use horizon_world::level::IgniteCause;
                let cause = match cause {
                    IgniteCause::Lava => ev::IgniteCause::Lava,
                    IgniteCause::Spread => ev::IgniteCause::Spread,
                };
                let event = ev::BlockIgniteEvent {
                    block,
                    cause,
                    igniting_entity: None,
                    igniting_block: Some(self.plugin_level_block(*source)),
                    cancelled: false,
                };
                self.plugin_allows(None, event)
            }
            PluginAsk::SpongeAbsorb { blocks, .. } => {
                let mut event = ev::SpongeAbsorbEvent {
                    block,
                    blocks: blocks
                        .iter()
                        .map(|p| BlockPos::new(p.x, p.y, p.z))
                        .collect(),
                    cancelled: false,
                };
                self.plugin_fire(None, &mut event);
                let keep: std::collections::HashSet<(i32, i32, i32)> =
                    event.blocks.iter().map(|b| (b.x, b.y, b.z)).collect();
                blocks.retain(|p| keep.contains(&(p.x, p.y, p.z)));
                !event.cancelled
            }
            PluginAsk::Moisture { into, .. } => {
                let was = self.state_string(*into);
                let mut event = ev::MoistureChangeEvent {
                    block,
                    new_state: was.clone(),
                    cancelled: false,
                };
                self.plugin_fire(None, &mut event);
                if let Some(id) =
                    self.plugin_new_state(&event.new_state, &was, |n| n == "minecraft:farmland")
                {
                    *into = id;
                }
                !event.cancelled
            }
            PluginAsk::Cauldron { into, .. } => {
                let was = self.state_string(*into);
                let mut event = ev::CauldronLevelChangeEvent {
                    block,
                    entity: None,
                    reason: ev::CauldronChangeReason::NaturalFill,
                    new_state: was.clone(),
                    cancelled: false,
                };
                self.plugin_fire(None, &mut event);
                if let Some(id) =
                    self.plugin_new_state(&event.new_state, &was, |n| n.ends_with("cauldron"))
                {
                    *into = id;
                }
                !event.cancelled
            }
        };
        if allowed {
            self.with_level(|b, level, ticks| b.answer_plugin_ask(level, ticks, ask));
            self.process_level_events()?;
        }
        Ok(())
    }
    /// The next fluid tick's change of level: `FluidLevelChangeEvent`, and
    /// the tick learns what the plugins made of it.
    pub(super) fn plugin_fluid_level(&mut self) {
        if !self.plugins_listen::<ev::FluidLevelChangeEvent>() {
            return;
        }
        let Some((pos, old, new)) =
            self.with_level(|b, level, ticks| b.scheduled_fluid_level(level, ticks))
        else {
            return;
        };
        let mut block = self.plugin_level_block(pos);
        block.state = self.state_string(old);
        let was = self.state_string(new);
        let mut event = ev::FluidLevelChangeEvent {
            block,
            new_state: was.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        let answer = if event.cancelled {
            None
        } else {
            match self.plugin_new_state(&event.new_state, &was, |_| true) {
                Some(id) => Some(id),
                None => return,
            }
        };
        self.with_level(|b, _, ticks| b.answer_fluid_level(ticks, pos, answer));
    }

    // -----------------------------------------------------------------------
    // TNT and fire.

    /// TNT at `pos` about to be lit by `cause`: `TNTPrimeEvent`. False when
    /// a plugin cancelled it. A player's cause names the player whose turn
    /// it is; `entity` names another priming entity, `from` a priming block.
    pub(super) fn plugin_tnt_prime(
        &mut self,
        pos: [i32; 3],
        cause: ev::PrimeCause,
        entity: Option<u32>,
        from: Option<[i32; 3]>,
    ) -> bool {
        if !self.plugins_listen::<ev::TntPrimeEvent>() {
            return true;
        }
        let priming_entity = match entity {
            Some(id) => Some(self.plugin_entity(None, id as i32)),
            None if cause == ev::PrimeCause::Player => self.plugin_acting_entity(),
            None => None,
        };
        let event = ev::TntPrimeEvent {
            block: self.plugin_block(pos),
            cause,
            priming_entity,
            priming_block: from.map(|p| self.plugin_block(p)),
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed && cause == ev::PrimeCause::Player {
            self.plugin_resync_acting();
            self.plugin_resend_acting(pos);
        }
        allowed
    }
    /// Fire about to appear at `pos` (or a campfire or candle lit there):
    /// `BlockIgniteEvent`. False when a plugin cancelled it. `entity` names
    /// the igniting entity, None the player whose turn it is when
    /// `player_lit` (their client gets the block and their hand again).
    pub(super) fn plugin_ignite(
        &mut self,
        pos: [i32; 3],
        cause: ev::IgniteCause,
        entity: Option<u32>,
        player_lit: bool,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockIgniteEvent>() {
            return true;
        }
        let igniting_entity = match entity {
            Some(id) => Some(self.plugin_entity(None, id as i32)),
            None if player_lit => self.plugin_acting_entity(),
            None => None,
        };
        let event = ev::BlockIgniteEvent {
            block: self.plugin_block(pos),
            cause,
            igniting_entity,
            igniting_block: None,
            cancelled: false,
        };
        let allowed = self.plugin_allows(None, event);
        if !allowed && player_lit {
            self.plugin_resync_acting();
            self.plugin_resend_acting(pos);
        }
        allowed
    }

    // -----------------------------------------------------------------------
    // Beds and anchors exploding.

    /// The bed or anchor at `pos` is about to go and explode
    /// (`BlockExplodeEvent` names it as it was).
    pub(super) fn plugin_note_exploding_block(&mut self, pos: [i32; 3]) {
        if self.plugins_listen::<ev::BlockExplodeEvent>() {
            self.plugin_blocks.exploding = Some(self.plugin_block(pos));
        }
    }
    /// The explosion about to destroy `blocks`, when a bed or an anchor
    /// explodes: `BlockExplodeEvent`. None for any other explosion; else
    /// false when cancelled, and `blocks` keeps only those the plugins left.
    pub(super) fn plugin_block_explode(&mut self, blocks: &mut Vec<[i32; 3]>) -> Option<bool> {
        let block = self.plugin_blocks.exploding.take()?;
        let mut event = ev::BlockExplodeEvent {
            block,
            blocks: blocks
                .iter()
                .map(|p| BlockPos::new(p[0], p[1], p[2]))
                .collect(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            return Some(false);
        }
        let keep: std::collections::HashSet<[i32; 3]> =
            event.blocks.iter().map(|b| [b.x, b.y, b.z]).collect();
        blocks.retain(|p| keep.contains(p));
        Some(true)
    }

    // -----------------------------------------------------------------------
    // Blocks of two parts.

    /// The blocks a placement at `clicked` (on `face`) may touch, as they
    /// are, for `BlockMultiPlaceEvent`; None when no plugin listens.
    pub(super) fn plugin_multi_place_before(
        &self,
        clicked: [i32; 3],
        face: [i32; 3],
    ) -> Option<Vec<([i32; 3], BlockStateId)>> {
        if !self.plugins_listen::<ev::BlockMultiPlaceEvent>() {
            return None;
        }
        let next = [
            clicked[0] + face[0],
            clicked[1] + face[1],
            clicked[2] + face[2],
        ];
        let mut around = Vec::new();
        for c in [clicked, next] {
            for d in [
                [0, 0, 0],
                [0, -1, 0],
                [0, 1, 0],
                [0, 0, -1],
                [0, 0, 1],
                [-1, 0, 0],
                [1, 0, 0],
            ] {
                let p = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
                if !around.iter().any(|(q, _)| *q == p) {
                    around.push((p, self.world.block(p[0], p[1], p[2])));
                }
            }
        }
        Some(around)
    }
    /// The player whose turn it is placed `state` at `pos` with its other
    /// parts (the blocks next to it of the same kind that it set):
    /// `BlockMultiPlaceEvent`. False when a plugin cancelled it; every part
    /// is then put back (`before`: from `plugin_multi_place_before`).
    pub(super) fn plugin_multi_place(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        before: Option<Vec<([i32; 3], BlockStateId)>>,
        item: &ItemStack,
    ) -> bool {
        let Some(before) = before else {
            return true;
        };
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let kind = self.res.facts().get(state).name.clone();
        let replaced_here = before
            .iter()
            .find(|(p, _)| *p == pos)
            .map_or(self.world.air, |(_, s)| *s);
        let parts: Vec<([i32; 3], BlockStateId)> = before
            .into_iter()
            .filter(|(p, old)| {
                let d = [p[0] - pos[0], p[1] - pos[1], p[2] - pos[2]];
                let now = self.world.block(p[0], p[1], p[2]);
                d.iter().map(|v| v.abs()).sum::<i32>() == 1
                    && now != *old
                    && self.res.facts().get(now).name == kind
            })
            .collect();
        if parts.is_empty() {
            return true;
        }
        let mut blocks = vec![self.plugin_block(pos)];
        let mut replaced = vec![api::Block {
            state: self.state_string(replaced_here),
            ..self.plugin_block(pos)
        }];
        for &(p, old) in &parts {
            blocks.push(self.plugin_block(p));
            replaced.push(api::Block {
                state: self.state_string(old),
                ..self.plugin_block(p)
            });
        }
        let event = ev::BlockMultiPlaceEvent {
            player,
            world: self.dim.id().to_owned(),
            blocks,
            replaced,
            item: Some(super::plugin_events::stack_ref(item)),
            cancelled: false,
        };
        if self.plugin_allows(None, event) {
            return true;
        }
        for (p, old) in parts.into_iter().chain([(pos, replaced_here)]) {
            let at = Pos::new(p[0], p[1], p[2]);
            self.with_level(|b, level, ticks| b.set_block(level, ticks, at, old, 3));
            self.plugin_resend_acting(p);
        }
        self.plugin_resync_acting();
        false
    }

    // -----------------------------------------------------------------------
    // Cauldrons.

    /// The inventory before the player uses a cauldron, when a plugin
    /// listens to `CauldronLevelChangeEvent`.
    pub(super) fn plugin_cauldron_before(&self) -> Option<HandBefore> {
        if !self.plugins_listen::<ev::CauldronLevelChangeEvent>() {
            return None;
        }
        Some(HandBefore {
            inventory: self.inventory.clone(),
            offhand: self.offhand.clone(),
            held: self.held().cloned(),
        })
    }
    /// The player whose turn it is used the cauldron at `pos`, which is
    /// about to become `new`: `CauldronLevelChangeEvent`. False when a
    /// plugin cancelled it: the hand is as it was. A plugin may change `new`.
    pub(super) fn plugin_cauldron_use(
        &mut self,
        pos: [i32; 3],
        before: Option<HandBefore>,
        new: &mut Option<BlockStateId>,
    ) -> bool {
        let (Some(before), Some(into)) = (before, *new) else {
            return true;
        };
        use ev::CauldronChangeReason as R;
        let held = before.held.as_ref().map_or("", |s| &*s.item);
        let reason = match held {
            "minecraft:bucket" => R::BucketFill,
            "minecraft:glass_bottle" => R::BottleFill,
            "minecraft:potion" => R::BottleEmpty,
            h if h.ends_with("_bucket") => R::BucketEmpty,
            h if h.ends_with("_banner") => R::BannerWash,
            h if h.ends_with("shulker_box") => R::ShulkerWash,
            h if h.starts_with("minecraft:leather_") || h == "minecraft:wolf_armor" => R::ArmorWash,
            _ => R::Unknown,
        };
        let was = self.state_string(into);
        let mut event = ev::CauldronLevelChangeEvent {
            block: self.plugin_block(pos),
            entity: self.plugin_acting_entity(),
            reason,
            new_state: was.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            self.inventory = before.inventory;
            self.offhand = before.offhand;
            self.plugin_resync_acting();
            self.plugin_resend_acting(pos);
            return false;
        }
        if let Some(id) = self.plugin_new_state(&event.new_state, &was, |n| n.ends_with("cauldron"))
        {
            *new = Some(id);
        }
        true
    }
    /// A burning player puts out their fire in the cauldron at `pos`, which
    /// is about to become `new`: `CauldronLevelChangeEvent` (`EXTINGUISH`).
    /// False when a plugin cancelled it; a plugin may change `new`.
    pub(super) fn plugin_cauldron_extinguish(
        &mut self,
        pos: [i32; 3],
        new: &mut BlockStateId,
    ) -> bool {
        if !self.plugins_listen::<ev::CauldronLevelChangeEvent>() {
            return true;
        }
        let was = self.state_string(*new);
        let mut event = ev::CauldronLevelChangeEvent {
            block: self.plugin_block(pos),
            entity: self.plugin_acting_entity(),
            reason: ev::CauldronChangeReason::Extinguish,
            new_state: was.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            return false;
        }
        if let Some(id) = self.plugin_new_state(&event.new_state, &was, |n| n.ends_with("cauldron"))
        {
            *new = id;
        }
        true
    }

    // -----------------------------------------------------------------------
    // Dispensers and campfires.

    /// The dispenser at `dispenser` is about to shear the mob at `index`
    /// (when it is ready to be): `BlockShearEntityEvent`. False when a plugin
    /// cancelled it.
    pub(super) fn plugin_dispenser_shear(
        &mut self,
        dispenser: (i32, i32, i32),
        index: usize,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockShearEntityEvent>() {
            return true;
        }
        use crate::hc::mobs::{Species, sulfur_cube};
        let m = &self.mobs[index];
        let ready = match m.species() {
            Species::SulfurCube => sulfur_cube::has_body(m),
            Species::Sheep => !m.extra.sheared && !m.is_baby(),
            Species::Bogged | Species::SnowGolem => !m.extra.sheared,
            Species::CopperGolem => self.golem_shearable(index),
            Species::Mooshroom => !m.is_baby(),
            _ => false,
        };
        if !ready {
            return true;
        }
        let event = ev::BlockShearEntityEvent {
            block: self.plugin_block([dispenser.0, dispenser.1, dispenser.2]),
            entity: self.plugin_entity(None, m.id as i32),
            tool: api::ItemStack::new("minecraft:shears", 1),
            cancelled: false,
        };
        self.plugin_allows(None, event)
    }
    /// The campfire at `pos` finished cooking `item` (from `slot`) into
    /// `result`: `BlockCookEvent`. What it drops, or None when a plugin
    /// cancelled it (the item goes back on the fire).
    pub(super) fn plugin_campfire_cook(
        &mut self,
        pos: (i32, i32, i32),
        slot: usize,
        item: &ItemStack,
        result: ItemStack,
    ) -> Option<ItemStack> {
        if !self.plugins_listen::<ev::BlockCookEvent>() {
            return Some(result);
        }
        let was = super::plugin_events::stack_ref(&result);
        let mut event = ev::BlockCookEvent {
            block: self.plugin_block([pos.0, pos.1, pos.2]),
            source: super::plugin_events::stack_ref(item),
            result: was.clone(),
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            if let Some(crate::hc::containers::BlockEntity::Campfire(c)) =
                self.world.block_entities.get_mut(&pos)
            {
                c.items[slot] = Some(item.clone());
            }
            return None;
        }
        if event.result == was {
            return Some(result);
        }
        Some(ItemStack::new(&event.result.item, event.result.count))
    }
}
