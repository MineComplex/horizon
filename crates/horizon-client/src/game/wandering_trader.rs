//! `WanderingTraderSpawner`: every 1200 ticks the spawn delay counts down;
//! when it runs out a trader may appear near the player (or the nearest
//! bell), bringing two llamas on leads, and wanders off after two days.
use super::{ Game, creatures::Creatures};
use horizon_client::save::write_dat;
use horizon_world::nbt::Tag;

const TICK_DELAY: i32 = 1200;
const SPAWN_DELAY: i32 = 24000;
const MIN_CHANCE: i32 = 25;

/// The spawner's clock and the `WanderingTraderData` saved with the world.
pub(super) struct Spawner {
    tick_delay: i32,
    spawn_delay: i32,
    chance: i32,
    loaded: bool,
    dirty: bool,
}
impl Default for Spawner {
    fn default() -> Self {
        Self {
            tick_delay: TICK_DELAY,
            spawn_delay: SPAWN_DELAY,
            chance: MIN_CHANCE,
            loaded: false,
            dirty: false,
        }
    }
}
impl Creatures {
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    fn trader_path(&self) -> std::path::PathBuf {
        self.world_root
            .join("data")
            .join("minecraft")
            .join("wandering_trader.dat")
    }
    pub(super) fn save_wandering_trader(&mut self) {
        if !self.creatures.trader.loaded || !self.creatures.trader.dirty {
            return;
        }
        let t = &self.creatures.trader;
        let tag = Tag::compound([
            (
                "data",
                Tag::compound([
                    ("spawn_delay", Tag::Int(t.spawn_delay)),
                    ("spawn_chance", Tag::Int(t.chance)),
                ]),
            ),
            ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ]);
        if write_dat(&self.trader_path(), tag).is_ok() {
            self.creatures.trader.dirty = false;
        }
    }
}
