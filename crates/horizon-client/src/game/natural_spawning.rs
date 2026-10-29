//! The game's side of `NaturalSpawner` (`mobs::spawn_selection`): the level
//! it reads, the spawning chunks and caps of `ServerChunkCache.tickChunks`,
//! and the mobs it adds (finalized with the game's `finalizeSpawn`).
use super::{Dim, Game};
use horizon_client::{
    biome::{BiomeInfo, SpawnEntry},
    blocks::BlockFacts,
    collisions::{SceneCollisions, no_collision_empty},
    mobs::{
        Mob, MobWorld, Species,
        data::{self, MobData},
        seen_grid::SeenGrid,
        spawn_rules::{Dimension, SpawnLevel},
        spawn_selection::{
            self, Category, Heightmap, NaturalLevel, SpawnCost, StructureAt, StructureSpawns,
        },
    },
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::registry::BlockStateId;
use std::collections::HashMap;

/// A player in the level in place, as spawning and despawning see it.
pub(super) struct LevelPlayer {
    pub(super) position: Vec3,
    pub(super) bounds: Aabb,
    pub(super) alive: bool,
    pub(super) creative: bool,
    pub(super) spectator: bool,
}
impl LevelPlayer {
    fn distance_sqr(&self, x: f64, y: f64, z: f64) -> f64 {
        (self.position.x - x).powi(2)
            + (self.position.y - y).powi(2)
            + (self.position.z - z).powi(2)
    }
}

/// The game level as natural spawning reads it.
pub(super) struct GameLevel<'g, 'a> {
    pub(super) game: &'g crate::game::Sim<'a>,
    pub(super) w: MobWorld<'g>,
    /// Every player in the level ([`Game::level_players`]).
    pub(super) players: Vec<LevelPlayer>,
    /// Where the non-spectators among them are (`getNearestPlayer`).
    pub(super) nearest: Vec<[f64; 3]>,
    /// Boxes of the mobs this pass created, not yet in the game's list.
    pub(super) pending: std::cell::RefCell<Vec<Aabb>>,
    /// The types and boxes of what a spawner created this tick.
    pub(super) pending_ids: std::cell::RefCell<Vec<(String, Aabb)>>,
    /// The game's mobs by box, built once a pass has asked more than
    /// [`SCANNED_QUERIES`] box queries (the mobs do not change while it
    /// holds the game); the first ones scan, as the build costs a few scans.
    pub(super) boxes: std::cell::OnceCell<MobBoxes>,
    pub(super) box_queries: std::cell::Cell<u32>,
}
const SCANNED_QUERIES: u32 = 8;
/// A mob not removed: its index in `Game::mobs`, box, alive, and whether
/// it is a living shulker.
type MobBox = (usize, Aabb, bool, bool);
fn mob_box(i: usize, m: &Mob) -> MobBox {
    let alive = m.alive();
    (
        i,
        m.bounds(),
        alive,
        alive && m.species() == Species::Shulker,
    )
}
/// The boxes of the game's mobs not removed, and a grid over them.
pub(super) struct MobBoxes {
    entries: Vec<MobBox>,
    grid: SeenGrid,
}
impl MobBoxes {
    fn new(mobs: &[Mob]) -> Self {
        let entries: Vec<_> = mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| !m.removed)
            .map(|(i, m)| mob_box(i, m))
            .collect();
        let grid = SeenGrid::new(entries.iter().map(|e| e.1));
        Self { entries, grid }
    }
    /// The entries whose boxes meet `area` (and possibly a few more).
    fn near(&self, area: Aabb) -> impl Iterator<Item = MobBox> + '_ {
        let mut near = Vec::new();
        let indexed = self.grid.candidates(area, &mut near);
        let all = (!indexed).then(|| self.entries.iter());
        near.into_iter()
            .map(|i| self.entries[i as usize])
            .chain(all.into_iter().flatten().copied())
    }
}
impl GameLevel<'_, '_> {
    /// The mobs not removed whose boxes may meet `area` (callers test them).
    fn mobs_near(&self, area: Aabb) -> Box<dyn Iterator<Item = MobBox> + '_> {
        let asked = self.box_queries.get() + 1;
        self.box_queries.set(asked);
        if asked <= SCANNED_QUERIES && self.boxes.get().is_none() {
            return Box::new(
                self.game
                    .mobs
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| !m.removed)
                    .map(|(i, m)| mob_box(i, m)),
            );
        }
        Box::new(
            self.boxes
                .get_or_init(|| MobBoxes::new(&self.game.mobs))
                .near(area),
        )
    }
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<&BiomeInfo> {
        let chunk = self.game.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(self.game.res.biomes().get(chunk.biome(x, y, z)))
    }
}
impl SpawnLevel for GameLevel<'_, '_> {
    fn state(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.w.state(x, y, z)
    }
    fn facts(&self) -> &BlockFacts {
        self.w.facts
    }
    fn mob_data(&self) -> &MobData {
        self.w.data
    }
    fn block_tag(&self, tag: &str, block: &str) -> bool {
        self.w.block_tag(tag, block)
    }
    fn biome_tag(&self, tag: &str, x: i32, y: i32, z: i32) -> bool {
        self.w.biome_tag(tag, x, y, z)
    }
    fn sky_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.sky_light(x, y, z)
    }
    fn block_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.block_light(x, y, z)
    }
    fn sky_darken(&self) -> i32 {
        SpawnLevel::sky_darken(&self.w)
    }
    fn thundering(&self) -> bool {
        self.w.thundering
    }
    fn sea_level(&self) -> i32 {
        self.w.sea_level()
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.w.world_surface(x, z)
    }
    fn peaceful(&self) -> bool {
        self.w.peaceful()
    }
    fn dimension(&self) -> Dimension {
        self.w.dimension()
    }
    fn surface_slime_chance(&self, x: i32, y: i32, z: i32) -> f32 {
        self.w.surface_slime_chance(x, y, z)
    }
    fn world_seed(&self) -> i64 {
        self.w.world_seed
    }
    /// `getNearestPlayer(x, y, z, distance, true)`, over every player.
    fn player_near(&self, x: f64, y: f64, z: f64, distance: f64) -> bool {
        self.players.iter().any(|p| {
            p.alive && !p.creative && !p.spectator && p.distance_sqr(x, y, z) < distance * distance
        })
    }
}
impl NaturalLevel for GameLevel<'_, '_> {
    fn min_y(&self) -> i32 {
        horizon_client::chunk::MIN_Y
    }
    fn max_y(&self) -> i32 {
        horizon_client::chunk::MIN_Y + horizon_client::chunk::HEIGHT - 1
    }
    fn ambient_light(&self) -> f32 {
        match self.game.dim {
            Dim::Nether => 0.1,
            Dim::End => 0.25,
            _ => 0.0,
        }
    }
    fn has_ceiling(&self) -> bool {
        self.game.dim == Dim::Nether
    }
    fn mob_type(&self, entity: &str) -> Option<(Species, f32, f32, Category)> {
        let t = self.game.res.mob_types().get(entity)?;
        Some((
            t.species,
            t.width as f32,
            t.height as f32,
            Category::from_name(&t.category),
        ))
    }
    fn biome_spawns(&self, x: i32, y: i32, z: i32) -> Option<&HashMap<String, Vec<SpawnEntry>>> {
        self.biome(x, y, z).map(|b| &b.spawns)
    }
    fn spawn_cost(&self, entity: &str, x: i32, y: i32, z: i32) -> Option<SpawnCost> {
        self.biome(x, y, z)?.spawn_costs.get(entity).copied()
    }
    fn creature_probability(&self, x: i32, y: i32, z: i32) -> f32 {
        self.biome(x, y, z).map_or(0.1, |b| b.creature_probability)
    }
    fn structures_at(
        &self,
        x: i32,
        y: i32,
        z: i32,
        wanted: &dyn Fn(&str) -> bool,
    ) -> Vec<StructureAt> {
        self.game
            .structures_reaching_where([x, y, z], wanted)
            .into_iter()
            .map(|(id, in_piece)| StructureAt {
                id,
                in_piece,
                in_bounds: true,
            })
            .collect()
    }
    fn structure_spawns(&self) -> &StructureSpawns {
        StructureSpawns::shared(&self.game.options.resources)
    }
    fn nearest_player_positions(&self) -> &[[f64; 3]] {
        &self.nearest
    }
    fn respawn(&self) -> Option<[i32; 3]> {
        // The world spawn point lives in the overworld.
        if self.game.dim != Dim::Overworld {
            return None;
        }
        self.game.level.spawn.map(|s| {
            [
                s[0].floor() as i32,
                s[1].floor() as i32,
                s[2].floor() as i32,
            ]
        })
    }
    fn can_spawn_in_chunk(&self, cx: i32, cz: i32) -> bool {
        self.game.world.chunks.contains_key(&(cx, cz))
    }
    fn no_collision(&self, bounds: Aabb) -> bool {
        let mut scene = SceneCollisions {
            world: &self.game.world,
            binding: self.w.collision,
            subject: None,
        };
        // getEntityCollisions(null, box): living shulkers are solid.
        let grown = bounds.inflate(Vec3::new(1e-7, 1e-7, 1e-7));
        no_collision_empty(&mut scene, bounds)
            && !self
                .mobs_near(grown)
                .any(|(_, b, _, shulker)| shulker && b.intersects(grown))
            && !self
                .pending_ids
                .borrow()
                .iter()
                .any(|(id, b)| id == "minecraft:shulker" && b.intersects(grown))
    }
    fn unobstructed(&self, bounds: Aabb) -> bool {
        let player = self
            .players
            .iter()
            .any(|p| p.alive && !p.spectator && p.bounds.intersects(bounds));
        !player
            && !self.pending.borrow().iter().any(|b| b.intersects(bounds))
            && !self
                .mobs_near(bounds)
                .any(|(_, b, alive, _)| alive && b.intersects(bounds))
    }
    fn spawn_mobs(&self) -> bool {
        self.game.world_data.rules.bool("spawn_mobs")
    }
    /// WORLD_SURFACE (every spawn attempt's) is the chunk's kept heightmap.
    /// The others are scanned down the column: sections of air alone are
    /// skipped (air counts for no heightmap). An unloaded column reads as air.
    fn heightmap(&self, kind: Heightmap, x: i32, z: i32) -> i32 {
        let world = self.w.world;
        if kind == Heightmap::WorldSurface {
            return self.w.world_surface(x, z);
        }
        world
            .chunks
            .get(&(x >> 4, z >> 4))
            .and_then(|chunk| {
                chunk.highest(world.air.0, |y| {
                    spawn_selection::heightmap_blocks(self, kind, x, y, z)
                })
            })
            .map_or(horizon_client::chunk::MIN_Y, |y| y + 1)
    }
    fn pathfindable_land(&self, x: i32, y: i32, z: i32) -> bool {
        self.w.data.state(self.state(x, y, z)).flags & data::LAND != 0
    }
}
impl horizon_client::mobs::spawner::SpawnerLevel for GameLevel<'_, '_> {
    /// `Level.hasNearbyAlivePlayer`, over every player.
    fn alive_player_near(&self, x: f64, y: f64, z: f64, range: f64) -> bool {
        self.players.iter().any(|p| {
            p.alive && !p.spectator && (range < 0.0 || p.distance_sqr(x, y, z) < range * range)
        })
    }
    fn spawner_blocks_work(&self) -> bool {
        self.game.world_data.rules.bool("spawner_blocks_work")
    }
    fn entity_exists(&self, id: &str) -> bool {
        self.game.res.mob_types().contains_key(id) || self.game.entity_type_exists(id)
    }
    fn count_entities(&self, id: &str, bounds: Aabb) -> usize {
        let mobs = self
            .mobs_near(bounds)
            .filter(|&(i, b, ..)| self.game.mobs[i].kind.id == id && b.intersects(bounds))
            .count();
        let pending = self
            .pending_ids
            .borrow()
            .iter()
            .filter(|(k, b)| k == id && b.intersects(bounds))
            .count();
        mobs + pending
    }
}
impl<'a> Game<'a> {}
impl<'a> crate::game::Sim<'a> {
    pub(super) fn natural_level(&self) -> GameLevel<'_, 'a> {
        let players = self.level_players();
        let nearest = players
            .iter()
            .filter(|p| !p.spectator)
            .map(|p| [p.position.x, p.position.y, p.position.z])
            .collect();
        GameLevel {
            game: self,
            w: self.mob_world_base(&[]),
            players,
            nearest,
            pending: Default::default(),
            pending_ids: Default::default(),
            boxes: Default::default(),
            box_queries: Default::default(),
        }
    }
    /// Every player in the level in place: the local player, and the LAN
    /// guests or server players in it.
    pub(super) fn level_players(&self) -> Vec<LevelPlayer> {
        let mut players: Vec<LevelPlayer> = (Vec::new)();
        if let Some(p) = self.player_view() {
            players.insert(
                0,
                LevelPlayer {
                    position: p.position,
                    bounds: p.bounds,
                    alive: p.alive,
                    creative: p.creative,
                    spectator: p.spectator,
                },
            );
        }
        players
    }
}
