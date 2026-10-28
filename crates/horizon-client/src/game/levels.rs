//! A server's levels, one per dimension (`MinecraftServer.levels`).
//!
//! The game's tick code reads one level: the one in place, held in `Game`'s
//! own fields (the world, its streaming and save, block ticks, entities).
//! A server (the dedicated server and single player's server thread) keeps
//! the other dimensions' levels here and ticks each one with players or forced
//! chunks by swapping it into place (`enter_level`), the way `as_guest` swaps a player
//! in, so the tick code runs unchanged, one level at a time. The in-frame
//! game holds one level, which follows its player (`enter_dimension`).
//!
//! Shared by every level, as vanilla's derived levels share the
//! Overworld's level data: the game time, the day time and the weather, the
//! game rules, the scoreboard, the maps and the entities waiting to arrive
//! through a portal.

use super::{
    Dim, ExperienceOrb, FallingBlock, Game, ItemEntity, Mob, boats, commands,
    creaking::CreakingState, dragon, fireworks, lan::LanHost, lightning, machines, minecarts,
    ominous_item_spawner, persist, raids::Raiding, sculk::SculkState, tnt, trident,
};
use horizon_client::{
    Result, block_entity_anim::BlockEntityAnims, random_tick_sections::RandomTickSections,
    save::SaveDir, streaming::Streaming, tick_chunk_membership::TickChunkMembership,
    ticket_levels::ChunkTickets, vanillagen::VanillaTerrain, world::World,
};
use horizon_world::level::{LevelEvent, Ticks};
use std::{
    collections::HashMap,
    sync::Arc};


/// A level that is not in place: every per-level field of `Game`.
pub(super) struct Parked {
    world: World,
    streaming: Streaming,
    locator: Option<Arc<VanillaTerrain>>,
    save: Arc<SaveDir>,
    block_ticks: Ticks,
    tick_chunks: TickChunkMembership,
    tickets: ChunkTickets,
    random_tick_sections: RandomTickSections,
    level_events: Vec<LevelEvent>,
    falling: Vec<FallingBlock>,
    paintings: Vec<horizon_client::entities::Painting>,
    item_frames: Vec<horizon_client::entities::ItemFrame>,
    armor_stands: Vec<horizon_client::entities::ArmorStand>,
    cushions: Vec<horizon_client::entities::Cushion>,
    others: Vec<commands::OtherEntity>,
    pub(super) boats: Vec<boats::Boat>,
    pub(super) carts: Vec<minecarts::Minecart>,
    jukebox_sounds: std::collections::HashSet<(i32, i32, i32)>,
    bolts: Vec<lightning::Bolt>,
    crystals: Vec<dragon::Crystal>,
    dragon: Option<dragon::Dragon>,
    dragon_fireballs: Vec<dragon::DragonFireball>,
    breath_clouds: Vec<dragon::Breath>,
    tridents: Vec<trident::ThrownTrident>,
    rockets: Vec<fireworks::Rocket>,
    ominous_spawners: Vec<ominous_item_spawner::OminousItemSpawner>,
    tnts: Vec<tnt::PrimedTnt>,
    hoppers: HashMap<(i32, i32, i32), machines::HopperState>,
    pub(super) mobs: Vec<Mob>,
    /// The entity ids of the level's fence knots (`knot_ids`).
    knot_ids: HashMap<[i32; 3], u32>,
    projectiles: Vec<horizon_client::projectiles::Projectile>,
    dropped: Vec<ItemEntity>,
    orbs: Vec<ExperienceOrb>,
    persist: persist::EntityMemory,
    lectern_pulses: Vec<((i32, i32, i32), i32)>,
    lid_openers: HashMap<(i32, i32, i32), i32>,
    lan_events: Vec<(i32, [i32; 3], i32, Option<u32>)>,
    lan_block_events: Vec<([i32; 3], i32, i32, u32)>,
    lan_entity_events: Vec<(i32, i8, [f64; 3])>,
    lan_critical_hits: Vec<(Option<u32>, i32, bool)>,
    /// Only the per-level parts of these are used (`swap_level`).
    anims: BlockEntityAnims,
    raiding: Raiding,
    sculk: SculkState,
    creaking: CreakingState,
    /// Server ticks since a player was last in the level.
    idle: u32,
}

/// Which passes of the server tick the level in place runs: the first
/// reads the network (`Hub::poll`), the last ends the tick for every player
/// (`Hub::end_tick`). A tick with one level is both.
#[derive(Clone, Copy)]
pub(super) struct Pass {
    pub first: bool,
    pub last: bool,
}

impl Default for Pass {
    fn default() -> Self {
        Self {
            first: true,
            last: true,
        }
    }
}

/// The levels not in place, by dimension.
#[derive(Default)]
pub(super) struct Levels {
    parked: HashMap<Dim, Parked>,
}

impl Levels {

    /// The levels that are not in place, by dimension.
    pub(super) fn parked_dims(&self) -> Vec<Dim> {
        self.parked.keys().copied().collect()
    }
}

impl Game<'_> {
}
impl crate::game::Sim<'_> {
    /// A new level for `dim`, opened from the save (`MinecraftServer.createLevels`).
    fn open_level(&self, dim: Dim) -> Result<Parked> {
        let (streaming, save, locator) = super::world_loading::make_streaming(
            &self.res,
            &self.pack,
            &self.options,
            &self.settings,
            &self.world_root,
            dim,
            self.level.seed,
        )?;
        let mut world = World::new(self.res.facts().air, self.level.seed);
        world.fast_lava = dim == Dim::Nether;
        Ok(Parked {
            world,
            streaming,
            locator,
            save,
            block_ticks: Ticks::new(self.level.seed as i32),
            tick_chunks: Default::default(),
            tickets: Default::default(),
            random_tick_sections: Default::default(),
            level_events: Vec::new(),
            falling: Vec::new(),
            paintings: Vec::new(),
            item_frames: Vec::new(),
            armor_stands: Vec::new(),
            cushions: Vec::new(),
            others: Vec::new(),
            boats: Vec::new(),
            carts: Vec::new(),
            jukebox_sounds: std::collections::HashSet::new(),
            bolts: Vec::new(),
            crystals: Vec::new(),
            dragon: None,
            dragon_fireballs: Vec::new(),
            breath_clouds: Vec::new(),
            tridents: Vec::new(),
            rockets: Vec::new(),
            ominous_spawners: Vec::new(),
            tnts: Vec::new(),
            hoppers: HashMap::new(),
            mobs: Vec::new(),
            knot_ids: HashMap::new(),
            projectiles: Vec::new(),
            dropped: Vec::new(),
            orbs: Vec::new(),
            persist: Default::default(),
            lectern_pulses: Vec::new(),
            lid_openers: HashMap::new(),
            lan_events: Vec::new(),
            lan_block_events: Vec::new(),
            lan_entity_events: Vec::new(),
            lan_critical_hits: Vec::new(),
            anims: Default::default(),
            raiding: Default::default(),
            sculk: Default::default(),
            creaking: Default::default(),
            idle: 0,
        })
    }
    /// Exchange the level in place with `p`.
    fn swap_level(&mut self, p: &mut Parked) {
        use std::mem::swap;
        swap(&mut self.world, &mut p.world);
        swap(&mut self.streaming, &mut p.streaming);
        let mut locator = self.end.locator();
        swap(&mut locator, &mut p.locator);
        self.end.set_locator(locator);
        self.share_spawn_conditions();
        swap(&mut self.save, &mut p.save);
        swap(&mut self.block_ticks, &mut p.block_ticks);
        swap(&mut self.tick_chunks, &mut p.tick_chunks);
        swap(&mut self.tickets, &mut p.tickets);
        swap(&mut self.random_tick_sections, &mut p.random_tick_sections);
        swap(&mut self.level_events, &mut p.level_events);
        swap(&mut self.falling, &mut p.falling);
        swap(&mut self.paintings, &mut p.paintings);
        swap(&mut self.item_frames, &mut p.item_frames);
        swap(&mut self.armor_stands, &mut p.armor_stands);
        swap(&mut self.cushions, &mut p.cushions);
        swap(&mut self.others, &mut p.others);
        swap(&mut self.boats, &mut p.boats);
        swap(&mut self.carts, &mut p.carts);
        swap(&mut self.jukebox_sounds, &mut p.jukebox_sounds);
        swap(&mut self.bolts, &mut p.bolts);
        swap(&mut self.crystals, &mut p.crystals);
        swap(&mut self.dragon, &mut p.dragon);
        swap(&mut self.dragon_fireballs, &mut p.dragon_fireballs);
        swap(&mut self.breath_clouds, &mut p.breath_clouds);
        swap(&mut self.tridents, &mut p.tridents);
        swap(&mut self.rockets, &mut p.rockets);
        swap(&mut self.ominous_spawners, &mut p.ominous_spawners);
        swap(&mut self.tnts, &mut p.tnts);
        swap(&mut self.hoppers, &mut p.hoppers);
        swap(&mut self.mobs, &mut p.mobs);
        swap(&mut self.knot_ids, &mut p.knot_ids);
        swap(&mut self.creatures.projectiles, &mut p.projectiles);
        swap(&mut self.dropped, &mut p.dropped);
        swap(&mut self.orbs, &mut p.orbs);
        swap(&mut self.persist, &mut p.persist);
        swap(&mut self.lectern_pulses, &mut p.lectern_pulses);
        swap(&mut self.lid_openers, &mut p.lid_openers);
        swap(&mut self.lan_events, &mut p.lan_events);
        swap(&mut self.lan_block_events, &mut p.lan_block_events);
        swap(&mut self.lan_entity_events, &mut p.lan_entity_events);
        swap(&mut self.lan_critical_hits, &mut p.lan_critical_hits);
        let (a, b) = (&mut self.be_anims, &mut p.anims);
        swap(&mut a.chests, &mut b.chests);
        swap(&mut a.books, &mut b.books);
        swap(&mut a.shulkers, &mut b.shulkers);
        swap(&mut a.bells, &mut b.bells);
        swap(&mut a.conduits, &mut b.conduits);
        swap(&mut a.skulls, &mut b.skulls);
        swap(&mut a.pots, &mut b.pots);
        swap(&mut a.gateways, &mut b.gateways);
        swap(&mut a.gateway_servers, &mut b.gateway_servers);
        swap(&mut a.conduit_servers, &mut b.conduit_servers);
        self.raiding.swap_level(&mut p.raiding);
        self.sculk.swap_level(&mut p.sculk);
        self.creaking.swap_level(&mut p.creaking);
        // What only the client draws starts over.
        self.particles.clear();
        self.pickups.clear();
    }
    /// Put `target`'s level in place, opening it first if it is not loaded.
    /// The level that was in place is kept with the others; the shared
    /// clock and weather go with the swap. `host` is the hub when the caller
    /// holds it (`self.lan` is taken during the network tick).
    pub(super) fn enter_level(&mut self, host: Option<&mut LanHost>, target: Dim) -> Result<()> {
        // The in-frame game keeps its one level (`enter_dimension`).
        if self.options.execution_role.is_client() {
            return Ok(());
        }
        if target != self.dim {
            let (mut p, opened) = match self.levels.parked.remove(&target) {
                Some(p) => (p, false),
                None => (self.open_level(target)?, true),
            };
            p.world.game_time = self.world.game_time;
            p.world.day_time = self.world.day_time;
            p.world.weather = self.world.weather;
            // The game rules and the difficulty are the server's, not a
            // level's: natural regeneration holds in every dimension.
            p.world.health_rules = self.world.health_rules;
            // The sculk tables' watched blocks are built once per game.
            p.world.watched = std::mem::take(&mut self.world.watched);
            self.swap_level(&mut p);
            p.idle = 0;
            let left = std::mem::replace(&mut self.dim, target);
            self.levels.parked.insert(left, p);
            if opened {
                // The new streaming's radius and meshing options.
                self.apply_streaming_settings();
            }
        }
        let host = match host {
            Some(h) => Some(h),
            None => self.lan.as_mut(),
        };
        if let Some(h) = host {
            h.hub.set_dimension(target.id());
        }
        Ok(())
    }
    /// Put `dim`'s level in place for a command whose source is there
    /// (`execute in`, `execute at` a player in another level), opening it if
    /// nobody is in it; returns the level to put back afterwards. Only a
    /// server holds other levels.
    pub(super) fn enter_command_level(&mut self, dim: Dim) -> Option<Dim> {
        if self.options.execution_role.is_client() || dim == self.dim {
            return None;
        }
        let back = self.dim;
        match self.enter_level(None, dim) {
            Ok(()) => Some(back),
            Err(e) => {
                log::error!("could not open {}: {e}", dim.id());
                None
            }
        }
    }
    /// Save every parked level (`MinecraftServer.saveAllChunks`); the level
    /// in place is the caller's to save.
    pub(super) fn save_parked_levels(&mut self, wait: bool) -> Result<()> {
        if self.levels.parked.is_empty() {
            return Ok(());
        }
        let back = self.dim;
        let dims: Vec<Dim> = self.levels.parked.keys().copied().collect();
        for dim in dims {
            self.enter_level(None, dim)?;
            self.save_level(wait)?;
        }
        self.enter_level(None, back)
    }
}
