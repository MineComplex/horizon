//! What the level asks a server's plugins before a block changes (Bukkit's
//! block events: `TNTPrimeEvent`, `NotePlayEvent`, `BellRingEvent`,
//! `BlockIgniteEvent`, `SpongeAbsorbEvent`, `MoistureChangeEvent`,
//! `CauldronLevelChangeEvent`) and what it tells them after
//! (`BlockRedstoneEvent`). A level whose plugins listen
//! (`LevelAccess::plugins_ask`) takes the change as a [`PluginAsk`] and
//! holds it back; [`Behaviours::answer_plugin_ask`] makes it once they let it
//! (as the plugins left it). A level that asks nothing changes as vanilla.
use super::{
    Behaviours, LevelAccess, Sim, Ticks,
    classes::{Class, Interface},
    update::rel,
};
use crate::registry::BlockStateId;
use horizon_core::{geometry::Direction, position::BlockPos};

/// What kind of change a [`PluginAsk`] is, for `LevelAccess::plugins_ask`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PluginAskKind {
    TntPrime,
    NotePlay,
    BellRing,
    Ignite,
    SpongeAbsorb,
    Moisture,
    Cauldron,
    Redstone,
}
impl PluginAskKind {
    /// Its bit in a set of kinds.
    pub const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// Why the level primes TNT (`TNTPrimeEvent.PrimeCause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TntCause {
    /// A redstone signal reached it (`TntBlock.onPlace`, `neighborChanged`).
    Redstone,
    /// Fire burnt it away.
    Fire,
    /// A player broke unstable TNT.
    BlockBreak,
}

/// Why the level sets a block on fire (`BlockIgniteEvent.IgniteCause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IgniteCause {
    /// `LavaFluid.randomTick`.
    Lava,
    /// `FireBlock.tick` spreading.
    Spread,
}

/// A change held back for the plugins, or a report.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginAsk {
    /// TNT at `pos` is about to be primed (a redstone prime also removes it).
    TntPrime { pos: BlockPos, cause: TntCause },
    /// `NoteBlock.playNote` of the note block at `pos`.
    NotePlay { pos: BlockPos, state: BlockStateId },
    /// `BellBlock.attemptToRing`: the bell at `pos`, struck towards `direction`.
    BellRing {
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
    },
    /// Fire (`fire`) is about to appear at `pos`, from `source` (the lava or
    /// the fire it spreads from).
    Ignite {
        pos: BlockPos,
        cause: IgniteCause,
        source: BlockPos,
        fire: BlockStateId,
    },
    /// The sponge at `pos` is about to soak up the water of `blocks`.
    SpongeAbsorb {
        pos: BlockPos,
        blocks: Vec<BlockPos>,
    },
    /// Farmland at `pos` is about to dry or get wet: `state` into `into`.
    Moisture {
        pos: BlockPos,
        state: BlockStateId,
        into: BlockStateId,
    },
    /// A cauldron fills by itself (rain, snow, a dripping stalactite):
    /// `state` into `into`, with the level event `sound` after.
    Cauldron {
        pos: BlockPos,
        state: BlockStateId,
        into: BlockStateId,
        sound: Option<i32>,
    },
    /// A report, never held: a redstone component's power went from `old`
    /// to `new` (wire, lever, button, torch, lamp).
    Redstone { pos: BlockPos, old: i32, new: i32 },
}
impl PluginAsk {
    pub fn kind(&self) -> PluginAskKind {
        match self {
            Self::TntPrime { .. } => PluginAskKind::TntPrime,
            Self::NotePlay { .. } => PluginAskKind::NotePlay,
            Self::BellRing { .. } => PluginAskKind::BellRing,
            Self::Ignite { .. } => PluginAskKind::Ignite,
            Self::SpongeAbsorb { .. } => PluginAskKind::SpongeAbsorb,
            Self::Moisture { .. } => PluginAskKind::Moisture,
            Self::Cauldron { .. } => PluginAskKind::Cauldron,
            Self::Redstone { .. } => PluginAskKind::Redstone,
        }
    }
    /// The block it is about.
    pub fn pos(&self) -> BlockPos {
        match *self {
            Self::TntPrime { pos, .. }
            | Self::NotePlay { pos, .. }
            | Self::BellRing { pos, .. }
            | Self::Ignite { pos, .. }
            | Self::SpongeAbsorb { pos, .. }
            | Self::Moisture { pos, .. }
            | Self::Cauldron { pos, .. }
            | Self::Redstone { pos, .. } => pos,
        }
    }
}

impl Behaviours {
    /// Make a change held back as a [`PluginAsk`] (the plugins let it), if
    /// the block it names is still there.
    pub fn answer_plugin_ask<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        ask: PluginAsk,
    ) {
        self.sim(level, ticks).answer_plugin_ask(ask);
    }
}

impl<L: LevelAccess + ?Sized> Sim<'_, L> {
    /// Hold back a change of `kind` for the plugins (true), or leave it to
    /// the caller to make now (false).
    pub(crate) fn held_for_plugins(
        &mut self,
        kind: PluginAskKind,
        ask: impl FnOnce(&mut Self) -> PluginAsk,
    ) -> bool {
        if !self.level.plugins_ask(kind) {
            return false;
        }
        let ask = ask(self);
        self.level.hold_for_plugins(ask);
        true
    }
    /// `BlockRedstoneEvent`: tell the plugins a component's power changed.
    pub(crate) fn report_redstone(&mut self, pos: BlockPos, old: i32, new: i32) {
        if old != new && self.level.plugins_ask(PluginAskKind::Redstone) {
            self.level
                .hold_for_plugins(PluginAsk::Redstone { pos, old, new });
        }
    }
    fn answer_plugin_ask(&mut self, ask: PluginAsk) {
        let b = self.b;
        match ask {
            PluginAsk::TntPrime { pos, cause } => {
                if cause == TntCause::Redstone {
                    // TntBlock.prime then removeBlock, if it is still TNT.
                    if b.instance_of(self.state(pos), Class::Tnt) {
                        self.prime_tnt(pos);
                        self.remove_block(pos, false);
                    }
                } else {
                    self.prime_tnt(pos);
                }
            }
            PluginAsk::NotePlay { pos, .. } => {
                let state = self.state(pos);
                if b.data(state).is(Class::Note) {
                    self.note_play_now(pos, state);
                }
            }
            PluginAsk::BellRing { pos, direction, .. } => {
                let state = self.state(pos);
                if b.data(state).is(Class::Bell) {
                    self.bell_ring_now(pos, state, direction);
                }
            }
            PluginAsk::Ignite {
                pos,
                cause,
                source,
                fire,
            } => {
                if !b.is_air(self.state(pos)) {
                    return;
                }
                match cause {
                    IgniteCause::Lava => {
                        self.set_block_and_update(pos, fire);
                    }
                    IgniteCause::Spread => {
                        self.changed(super::BlockChange::Spread { source }, &[pos], |s| {
                            s.set_block_and_update(pos, fire);
                        });
                    }
                }
            }
            PluginAsk::SpongeAbsorb { pos, blocks } => {
                if !b.instance_of(self.state(pos), Class::Sponge) {
                    return;
                }
                for at in blocks {
                    self.sponge_soak(at);
                }
                self.sponge_turn_wet(pos);
            }
            PluginAsk::Moisture { pos, state, into } => {
                if self.state(pos) == state {
                    self.set_block(pos, into, 2);
                }
            }
            PluginAsk::Cauldron {
                pos,
                state,
                into,
                sound,
            } => {
                if self.state(pos) == state {
                    self.set_block_and_update(pos, into);
                    self.game_event(pos, "minecraft:block_change");
                    if let Some(id) = sound {
                        self.level_event(id, pos, 0);
                    }
                }
            }
            PluginAsk::Redstone { .. } => {}
        }
    }
    /// The blocks `SpongeBlock.removeWaterBreadthFirstSearch` would soak up
    /// from the sponge at `start`, without soaking them (Paper asks the
    /// plugins with this list): none when it soaks up nothing.
    pub(crate) fn sponge_preview(&self, start: BlockPos) -> Vec<BlockPos> {
        use std::collections::{HashSet, VecDeque};
        let b = self.b;
        let mut queue = VecDeque::from([(start, 0)]);
        let mut visited = HashSet::new();
        let mut found = Vec::new();
        let mut count = 0;
        while let Some((pos, depth)) = queue.pop_front() {
            if !visited.insert(pos) {
                continue;
            }
            if pos != start {
                if !b.is_water_fluid(self.fluid_at(pos).kind()) || !self.sponge_takes(pos) {
                    continue;
                }
                found.push(pos);
            }
            count += 1;
            if count >= 65 {
                break;
            }
            if depth < 6 {
                for direction in Direction::ALL {
                    queue.push_back((rel(pos, direction), depth + 1));
                }
            }
        }
        found
    }
    /// Whether the sponge soaks up the water of the block at `pos`: a
    /// liquid, a waterlogged block, kelp or seagrass.
    fn sponge_takes(&self, pos: BlockPos) -> bool {
        let b = self.b;
        let state = self.state(pos);
        if b.instance_of(state, Class::Liquid) {
            return true;
        }
        if b.implements(state, Interface::SimpleWaterlogged)
            && b.table.has(state, b.props.waterlogged)
        {
            return b.is_waterlogged(state);
        }
        let n = b.named;
        let block = b.block(state);
        block == n.kelp || block == n.kelp_plant || block == n.seagrass || block == n.tall_seagrass
    }
    /// The sponge soaks up the water of the block at `pos`, as
    /// `removeWaterBreadthFirstSearch` does, if it still holds some.
    fn sponge_soak(&mut self, pos: BlockPos) {
        let b = self.b;
        let state = self.state(pos);
        if !b.is_water_fluid(self.fluid_at(pos).kind()) || self.pickup_block(pos, state) {
            return;
        }
        if b.instance_of(state, Class::Liquid) {
            self.set_block_and_update(pos, b.air());
            return;
        }
        let n = b.named;
        let block = b.block(state);
        if block == n.kelp
            || block == n.kelp_plant
            || block == n.seagrass
            || block == n.tall_seagrass
        {
            self.drop_resources(state, pos);
            self.set_block_and_update(pos, b.air());
        }
    }
}
