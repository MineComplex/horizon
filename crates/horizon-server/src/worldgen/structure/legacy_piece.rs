//! The shared half of the piece-based ("legacy") structures: `StructurePiece`'s fields, its
//! orientation transform and block helpers (`placeBlock`, `generateBox` and its variants,
//! `fillColumnDown`, `createChest`...), and [`LegacyPiece`], the enum of every piece kind that
//! places its blocks in code. Each kind lives in its own module (`scattered`, `mineshaft`,
//! `stronghold`, `fortress`, `monument`); this module saves, loads and places them by piece id.
//!
//! Paper's `CraftBukkit` hooks route chests, dispensers and spawners through block states with a
//! snapshot of the block entity's NBT: the block is written (`UPDATE_CLIENTS`), then the entity's
//! NBT (`LootTable`, `LootTableSeed`, `SpawnData`). The world has no block entities or entities
//! yet, so a placement collects them in [`PieceOutput`], as template placement does
//! ([`super::place::PlacedTemplate`]).

use super::fortress::FortressPiece;
use super::mineshaft::MineshaftPiece;
use super::monument::MonumentBuilding;
use super::scattered::{BuriedTreasurePiece, DesertPyramidPiece, JungleTemplePiece, SwampHutPiece};
use super::start::StructurePiece;
use super::stronghold::StrongholdPiece;
use crate::block::flags::SupportType;
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::{Compound, JavaString, Tag};
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::placement::{PlacementWorld, WorldError};
use crate::worldgen::structure::BiomeSet;

pub type Result<T> = std::result::Result<T, WorldError>;

/// `Block.UPDATE_CLIENTS`, the flags `placeBlock` writes with.
pub const UPDATE_CLIENTS: i32 = 2;

/// `net.minecraft.world.level.levelgen.structure.BoundingBox`, with its fields public: pieces
/// move and grow theirs. Boxes are never normalised (vanilla only logs an inverted one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl BoundingBox {
    pub const fn new(min: [i32; 3], max: [i32; 3]) -> Self {
        Self { min, max }
    }
    /// `BoundingBox.move(dx, dy, dz)` on a copy.
    pub fn moved(self, d: [i32; 3]) -> Self {
        Self {
            min: [0, 1, 2].map(|i| self.min[i].wrapping_add(d[i])),
            max: [0, 1, 2].map(|i| self.max[i].wrapping_add(d[i])),
        }
    }
    /// `isInside(pos)`.
    pub fn is_inside(&self, p: [i32; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
    /// `intersects(other)`.
    pub fn intersects(&self, other: &Self) -> bool {
        (0..3).all(|i| self.max[i] >= other.min[i] && self.min[i] <= other.max[i])
    }
    /// `intersects(minX, minZ, maxX, maxZ)`: the columns only.
    pub fn intersects_columns(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max[0] >= min_x && self.min[0] <= max_x && self.max[2] >= min_z && self.min[2] <= max_z
    }
    /// `encapsulate(other)`.
    pub fn encapsulate(&mut self, other: &Self) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(other.min[i]);
            self.max[i] = self.max[i].max(other.max[i]);
        }
    }
    /// `getXSpan`, `getYSpan`, `getZSpan`.
    pub fn span(&self) -> [i32; 3] {
        [0, 1, 2].map(|i| self.max[i] - self.min[i] + 1)
    }
    /// `getCenter`.
    pub fn center(&self) -> [i32; 3] {
        [0, 1, 2].map(|i| self.min[i] + (self.max[i] - self.min[i] + 1) / 2)
    }
    /// `BoundingBox.fromCorners`.
    pub fn from_corners(a: [i32; 3], b: [i32; 3]) -> Self {
        Self {
            min: [0, 1, 2].map(|i| a[i].min(b[i])),
            max: [0, 1, 2].map(|i| a[i].max(b[i])),
        }
    }
    pub fn to_bounds(self) -> StructureBounds {
        StructureBounds::from_corners(self.min, self.max)
    }
    pub fn from_bounds(bounds: StructureBounds) -> Self {
        Self::new(bounds.minimum(), bounds.maximum())
    }
    /// `BoundingBox.CODEC`.
    pub fn tag(&self) -> Tag {
        Tag::IntArray(vec![
            self.min[0],
            self.min[1],
            self.min[2],
            self.max[0],
            self.max[1],
            self.max[2],
        ])
    }
    pub fn read(tag: Option<&Tag>) -> Option<Self> {
        match tag? {
            // `new BoundingBox(...)` fixes inverted bounds axis by axis.
            Tag::IntArray(v) if v.len() == 6 => Some(Self::new(
                [v[0].min(v[3]), v[1].min(v[4]), v[2].min(v[5])],
                [v[0].max(v[3]), v[1].max(v[4]), v[2].max(v[5])],
            )),
            _ => None,
        }
    }
}

/// `StructurePiece.makeBoundingBox`.
pub fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    direction: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    if matches!(direction, Direction::North | Direction::South) {
        BoundingBox::new([x, y, z], [x + width - 1, y + height - 1, z + depth - 1])
    } else {
        BoundingBox::new([x, y, z], [x + depth - 1, y + height - 1, z + width - 1])
    }
}

pub const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `Direction.Plane.HORIZONTAL.getRandomDirection`.
pub fn random_horizontal(random: &mut impl RandomSource) -> Direction {
    HORIZONTAL[random.next_int_bound(4).unwrap_or(0) as usize]
}

/// `Direction.get2DDataValue`.
pub fn data_2d(d: Direction) -> i32 {
    match d {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
        Direction::Down | Direction::Up => -1,
    }
}

/// `Direction.from2DDataValue`.
pub fn from_2d(value: i32) -> Direction {
    [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ][value.rem_euclid(4) as usize]
}

pub fn direction_name(d: Direction) -> &'static str {
    match d {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

pub fn direction_by_name(name: &str) -> Option<Direction> {
    Direction::ALL
        .into_iter()
        .find(|d| direction_name(*d) == name)
}

pub fn offset(pos: [i32; 3], d: Direction, n: i32) -> [i32; 3] {
    let s = d.normal();
    [pos[0] + s[0] * n, pos[1] + s[1] * n, pos[2] + s[2] * n]
}

/// `Direction.getClockWise`.
pub fn clockwise(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}

/// `Direction.getCounterClockWise`.
pub fn counter_clockwise(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::West,
        Direction::West => Direction::South,
        Direction::South => Direction::East,
        Direction::East => Direction::North,
        other => other,
    }
}

/// The random draws a piece makes while it is placed (`RandomSource`, object safe).
pub trait PieceRandom {
    fn int(&mut self, bound: i32) -> i32;
    fn boolean(&mut self) -> bool;
    fn float(&mut self) -> f32;
    fn double(&mut self) -> f64;
    fn long(&mut self) -> i64;
}

impl<R: RandomSource> PieceRandom for R {
    fn int(&mut self, bound: i32) -> i32 {
        self.next_int_bound(bound).unwrap_or(0)
    }
    fn boolean(&mut self) -> bool {
        self.next_boolean()
    }
    fn float(&mut self) -> f32 {
        self.next_float()
    }
    fn double(&mut self) -> f64 {
        self.next_double()
    }
    fn long(&mut self) -> i64 {
        self.next_long()
    }
}

/// The level as a piece reads and writes it (object safe over [`FeatureWorld`]).
pub trait PieceWorld {
    fn blocks(&self) -> &BlockStates;
    fn data(&self) -> &BlockStateData;
    fn block(&mut self, pos: [i32; 3]) -> Result<BlockState>;
    fn set(&mut self, pos: [i32; 3], state: BlockState, flags: i32) -> Result<bool>;
    fn height(&mut self, kind: Heightmap, x: i32, z: i32) -> Result<i32>;
    fn min_y(&self) -> i32;
    /// `LevelHeightAccessor.getMaxY()`.
    fn max_y(&self) -> i32;
    /// `WorldGenLevel.getSeaLevel()`.
    fn sea_level(&self) -> i32;
    fn mark_postprocessing(&mut self, pos: [i32; 3]) -> Result<()>;
    fn fluid_tick(&mut self, pos: [i32; 3], fluid: u8, delay: i32) -> Result<()>;
    /// `WorldGenLevel.getRandom().nextBoolean()`.
    fn level_boolean(&mut self) -> Result<bool>;
    /// `WorldGenLevel.getRandom().nextFloat()`.
    fn level_float(&mut self) -> Result<f32>;
    /// `WorldGenLevel.getBiome(pos)`: a `worldgen/biome` registry id.
    fn biome(&mut self, pos: [i32; 3]) -> Result<u32>;
}

impl<W: FeatureWorld> PieceWorld for W {
    fn blocks(&self) -> &BlockStates {
        PlacementWorld::blocks(self)
    }
    fn data(&self) -> &BlockStateData {
        self.state_data()
    }
    fn block(&mut self, pos: [i32; 3]) -> Result<BlockState> {
        self.block_state(BlockPos::new(pos[0], pos[1], pos[2]))
    }
    fn set(&mut self, pos: [i32; 3], state: BlockState, flags: i32) -> Result<bool> {
        self.set_block(BlockPos::new(pos[0], pos[1], pos[2]), state, flags)
    }
    fn height(&mut self, kind: Heightmap, x: i32, z: i32) -> Result<i32> {
        PlacementWorld::height(self, kind, x, z)
    }
    fn min_y(&self) -> i32 {
        PlacementWorld::min_y(self)
    }
    fn sea_level(&self) -> i32 {
        self.generation().sea_level
    }
    fn max_y(&self) -> i32 {
        let level = self.level_generation();
        level.min_y + level.height - 1
    }
    fn mark_postprocessing(&mut self, pos: [i32; 3]) -> Result<()> {
        FeatureWorld::mark_postprocessing(self, BlockPos::new(pos[0], pos[1], pos[2]))
    }
    fn fluid_tick(&mut self, pos: [i32; 3], fluid: u8, delay: i32) -> Result<()> {
        self.schedule_fluid_tick(BlockPos::new(pos[0], pos[1], pos[2]), fluid, delay)
    }
    fn level_boolean(&mut self) -> Result<bool> {
        FeatureWorld::level_boolean(self)
    }
    fn level_float(&mut self) -> Result<f32> {
        FeatureWorld::level_float(self)
    }
    fn biome(&mut self, pos: [i32; 3]) -> Result<u32> {
        PlacementWorld::biome(self, BlockPos::new(pos[0], pos[1], pos[2]))
    }
}

/// What a placement makes besides blocks, until the world has block entities and entities.
#[derive(Debug, Default)]
pub struct PieceOutput {
    /// Block entity NBT by position, in the order it was set (a later one replaces an earlier).
    pub block_entities: Vec<(BlockPos, Compound)>,
    /// Entities to add, as their NBT (`id`, `Pos`, `Rotation`...).
    pub entities: Vec<Compound>,
}

/// Everything a piece needs while it is placed into one chunk (`postProcess`'s arguments).
pub struct PieceContext<'a> {
    pub world: &'a mut dyn PieceWorld,
    pub random: &'a mut dyn PieceRandom,
    pub out: &'a mut PieceOutput,
    pub chunk_box: BoundingBox,
    pub chunk: ChunkPos,
    pub reference: BlockPos,
    /// `WorldGenLevel.getSeed()`.
    pub seed: i64,
    /// `BiomeTags.MINESHAFT_BLOCKING`.
    pub mineshaft_blocking: &'a BiomeSet,
}

/// What a piece reads of the level besides its blocks.
#[derive(Clone, Copy)]
pub struct PieceLevel<'a> {
    pub seed: i64,
    pub mineshaft_blocking: &'a BiomeSet,
}

fn namespaced(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains(':') {
        name.into()
    } else {
        format!("minecraft:{name}").into()
    }
}

impl PieceContext<'_> {
    /// The default state of a block (air for a name the registry lacks).
    pub fn state(&self, name: &str) -> BlockState {
        let blocks = self.world.blocks();
        match blocks.block_by_name(&namespaced(name)) {
            Some(block) => blocks.default_state(block),
            None => blocks.default_state(blocks.block_by_name("minecraft:air").expect("air")),
        }
    }
    /// A block's state with these property values (the others default).
    pub fn state_with(&self, name: &str, properties: &[(&str, &str)]) -> BlockState {
        let blocks = self.world.blocks();
        let Some(block) = blocks.block_by_name(&namespaced(name)) else {
            return self.state("air");
        };
        blocks
            .state_with(block, properties.iter().copied())
            .unwrap_or_else(|| blocks.default_state(block))
    }
    pub fn name(&self, state: BlockState) -> &str {
        let blocks = self.world.blocks();
        blocks.block_name(blocks.block(state))
    }
    pub fn is_air(&self, state: BlockState) -> bool {
        self.world.data().is_air(state)
    }
    /// `BlockState.liquid()`.
    pub fn is_liquid(&self, state: BlockState) -> bool {
        self.world
            .data()
            .has(state, crate::block::flags::FLAG_LIQUID)
    }
    pub fn is_solid_render(&self, state: BlockState) -> bool {
        self.world.data().is_solid_render(state)
    }
    pub fn is_block(&mut self, pos: [i32; 3], name: &str) -> Result<bool> {
        let state = self.world.block(pos)?;
        Ok(self.name(state) == namespaced(name))
    }
    pub fn property(&self, state: BlockState, key: &str) -> Option<&str> {
        self.world.blocks().value(state, key)
    }
    pub fn with_property(&self, state: BlockState, key: &str, value: &str) -> BlockState {
        self.world
            .blocks()
            .with_property(state, key, value)
            .unwrap_or(state)
    }
    /// `BlockState.isFaceSturdy(level, pos, face)` with `SupportType.FULL`.
    pub fn face_sturdy(&mut self, pos: [i32; 3], face: Direction) -> bool {
        self.world
            .block(pos)
            .is_ok_and(|s| self.world.data().is_face_sturdy(s, face, SupportType::Full))
    }
    /// `BlockState.isFaceSturdy(level, pos, face, SupportType.CENTER)`.
    pub fn center_sturdy(&mut self, pos: [i32; 3], face: Direction) -> bool {
        self.world.block(pos).is_ok_and(|s| {
            self.world
                .data()
                .is_face_sturdy(s, face, SupportType::Center)
        })
    }
    /// `StructurePiece.isReplaceableByStructures`.
    pub fn replaceable_by_structures(&self, state: BlockState) -> bool {
        self.is_air(state)
            || self.is_liquid(state)
            || matches!(
                self.name(state),
                "minecraft:glow_lichen" | "minecraft:seagrass" | "minecraft:tall_seagrass"
            )
    }
    /// A block entity with a loot table and a seed drawn from the piece's random
    /// (`setLootTable(table, random.nextLong())`).
    pub fn loot_table(&mut self, pos: [i32; 3], kind: &str, table: &str) {
        let seed = self.random.long();
        self.out.block_entities.push((
            BlockPos::new(pos[0], pos[1], pos[2]),
            Tag::compound([
                ("id", Tag::String(kind.into())),
                ("LootTable", Tag::String(table.into())),
                ("LootTableSeed", Tag::Long(seed)),
            ])
            .into_compound(),
        ));
    }
    /// `placeCraftSpawner`: a spawner of `entity` (`SpawnData`, no potentials).
    pub fn spawner(&mut self, pos: [i32; 3], entity: &str) -> Result<()> {
        let state = self.state("spawner");
        self.world.set(pos, state, UPDATE_CLIENTS)?;
        let spawn_data = Tag::compound([(
            "entity",
            Tag::compound([("id", Tag::String(entity.into()))]),
        )]);
        self.out.block_entities.push((
            BlockPos::new(pos[0], pos[1], pos[2]),
            Tag::compound([
                ("id", Tag::String("minecraft:mob_spawner".into())),
                ("SpawnData", spawn_data),
            ])
            .into_compound(),
        ));
        Ok(())
    }
}

/// `Tag::compound(..)` as a [`Compound`].
trait IntoCompound {
    fn into_compound(self) -> Compound;
}

impl IntoCompound for Tag {
    fn into_compound(self) -> Compound {
        match self {
            Tag::Compound(c) => c,
            _ => Compound::new(),
        }
    }
}

/// `StructurePiece`'s own state: the box, the orientation (which fixes the mirror and rotation of
/// the blocks it places) and the generation depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceBase {
    pub bounding_box: BoundingBox,
    pub orientation: Option<Direction>,
    pub gen_depth: i32,
    /// Blocks `placeBlock` leaves alone (`canBeReplaced` false): not saved, set by the piece kind.
    pub keep: &'static [&'static str],
}

impl PieceBase {
    pub fn new(bounding_box: BoundingBox, orientation: Option<Direction>, gen_depth: i32) -> Self {
        Self {
            bounding_box,
            orientation,
            gen_depth,
            keep: &[],
        }
    }

    pub fn with_keep(mut self, keep: &'static [&'static str]) -> Self {
        self.keep = keep;
        self
    }

    /// `StructurePiece(type, tag)`: `GD`, `BB` and `O` (`None` for a missing or invalid box).
    pub fn load(tag: &Compound) -> Option<Self> {
        let get = |key: &str| tag.get(&JavaString::from(key));
        let bounding_box = BoundingBox::read(get("BB"))?;
        let orientation = get("O").and_then(Tag::int_value).unwrap_or(0);
        Some(Self {
            bounding_box,
            orientation: (orientation != -1).then(|| from_2d(orientation)),
            gen_depth: get("GD").and_then(Tag::int_value).unwrap_or(0),
            keep: &[],
        })
    }

    /// `StructurePiece.createTag`'s own fields.
    pub fn tag(&self, id: &str) -> Compound {
        Tag::compound([
            ("id", Tag::String(id.into())),
            ("BB", self.bounding_box.tag()),
            ("O", Tag::Int(self.orientation.map_or(-1, data_2d))),
            ("GD", Tag::Int(self.gen_depth)),
        ])
        .into_compound()
    }

    /// `setOrientation`'s mirror and rotation.
    pub fn mirror_rotation(&self) -> (Mirror, Rotation) {
        match self.orientation {
            Some(Direction::South) => (Mirror::LeftRight, Rotation::None),
            Some(Direction::West) => (Mirror::LeftRight, Rotation::Clockwise90),
            Some(Direction::East) => (Mirror::None, Rotation::Clockwise90),
            _ => (Mirror::None, Rotation::None),
        }
    }

    pub fn world_x(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            Some(Direction::North | Direction::South) => self.bounding_box.min[0] + x,
            Some(Direction::West) => self.bounding_box.max[0] - z,
            Some(Direction::East) => self.bounding_box.min[0] + z,
            _ => x,
        }
    }
    pub fn world_y(&self, y: i32) -> i32 {
        if self.orientation.is_none() {
            y
        } else {
            y + self.bounding_box.min[1]
        }
    }
    pub fn world_z(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            Some(Direction::North) => self.bounding_box.max[2] - z,
            Some(Direction::South) => self.bounding_box.min[2] + z,
            Some(Direction::West | Direction::East) => self.bounding_box.min[2] + x,
            _ => z,
        }
    }
    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        [self.world_x(x, z), self.world_y(y), self.world_z(x, z)]
    }

    /// `placeBlock` with the default `canBeReplaced`.
    pub fn place(
        &self,
        c: &mut PieceContext<'_>,
        state: BlockState,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<()> {
        let replaceable = if self.keep.is_empty() {
            true
        } else {
            let current = self.get(c, x, y, z)?;
            !self.keep.contains(&c.name(current))
        };
        self.place_if(c, state, x, y, z, replaceable)
    }

    /// `placeBlock` where `canBeReplaced(x, y, z)` was already answered.
    pub fn place_if(
        &self,
        c: &mut PieceContext<'_>,
        state: BlockState,
        x: i32,
        y: i32,
        z: i32,
        replaceable: bool,
    ) -> Result<()> {
        let pos = self.world_pos(x, y, z);
        if !c.chunk_box.is_inside(pos) || !replaceable {
            return Ok(());
        }
        let (mirror, rotation) = self.mirror_rotation();
        let mut state = state;
        if mirror != Mirror::None {
            state = c.world.data().mirror(state, mirror);
        }
        if rotation != Rotation::None {
            state = c.world.data().rotate(state, rotation);
        }
        c.world.set(pos, state, UPDATE_CLIENTS)?;
        let fluid = c.world.data().fluid(state);
        if c.world.data().fluid_name(fluid.fluid) != "minecraft:empty" {
            c.world.fluid_tick(pos, fluid.fluid, 0)?;
        }
        if matches!(
            c.name(state),
            "minecraft:nether_brick_fence"
                | "minecraft:torch"
                | "minecraft:wall_torch"
                | "minecraft:oak_fence"
                | "minecraft:spruce_fence"
                | "minecraft:dark_oak_fence"
                | "minecraft:pale_oak_fence"
                | "minecraft:acacia_fence"
                | "minecraft:birch_fence"
                | "minecraft:jungle_fence"
                | "minecraft:ladder"
                | "minecraft:iron_bars"
        ) {
            c.world.mark_postprocessing(pos)?;
        }
        Ok(())
    }

    /// `getBlock`: air outside the chunk's box.
    pub fn get(&self, c: &mut PieceContext<'_>, x: i32, y: i32, z: i32) -> Result<BlockState> {
        let pos = self.world_pos(x, y, z);
        if c.chunk_box.is_inside(pos) {
            c.world.block(pos)
        } else {
            Ok(c.state("air"))
        }
    }

    /// `isInterior`.
    pub fn is_interior(&self, c: &mut PieceContext<'_>, x: i32, y: i32, z: i32) -> Result<bool> {
        let pos = self.world_pos(x, y + 1, z);
        if !c.chunk_box.is_inside(pos) {
            return Ok(false);
        }
        Ok(pos[1] < c.world.height(Heightmap::OceanFloorWg, pos[0], pos[2])?)
    }

    fn skipped(
        &self,
        c: &mut PieceContext<'_>,
        skip_air: bool,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<bool> {
        if !skip_air {
            return Ok(false);
        }
        let state = self.get(c, x, y, z)?;
        Ok(c.is_air(state))
    }

    /// `generateAirBox`.
    pub fn generate_air_box(
        &self,
        c: &mut PieceContext<'_>,
        [x0, y0, z0]: [i32; 3],
        [x1, y1, z1]: [i32; 3],
    ) -> Result<()> {
        let air = c.state("air");
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place(c, air, x, y, z)?;
                }
            }
        }
        Ok(())
    }

    /// `generateBox(level, chunkBB, x0.., edge, fill, skipAir)`.
    pub fn generate_box(
        &self,
        c: &mut PieceContext<'_>,
        [x0, y0, z0]: [i32; 3],
        [x1, y1, z1]: [i32; 3],
        edge: BlockState,
        fill: BlockState,
        skip_air: bool,
    ) -> Result<()> {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.skipped(c, skip_air, x, y, z)? {
                        continue;
                    }
                    let interior = y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1;
                    self.place(c, if interior { fill } else { edge }, x, y, z)?;
                }
            }
        }
        Ok(())
    }

    /// `generateBox` with a block selector: called as (context, x, y, z, is_edge).
    pub fn generate_box_selector(
        &self,
        c: &mut PieceContext<'_>,
        [x0, y0, z0]: [i32; 3],
        [x1, y1, z1]: [i32; 3],
        skip_air: bool,
        selector: &mut dyn FnMut(&mut PieceContext<'_>, i32, i32, i32, bool) -> BlockState,
    ) -> Result<()> {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.skipped(c, skip_air, x, y, z)? {
                        continue;
                    }
                    let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    let state = selector(c, x, y, z, edge);
                    self.place(c, state, x, y, z)?;
                }
            }
        }
        Ok(())
    }

    /// `generateMaybeBox`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_maybe_box(
        &self,
        c: &mut PieceContext<'_>,
        probability: f32,
        [x0, y0, z0]: [i32; 3],
        [x1, y1, z1]: [i32; 3],
        edge: BlockState,
        fill: BlockState,
        skip_air: bool,
        inside: bool,
    ) -> Result<()> {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if c.random.float() > probability {
                        continue;
                    }
                    if self.skipped(c, skip_air, x, y, z)? {
                        continue;
                    }
                    if inside && !self.is_interior(c, x, y, z)? {
                        continue;
                    }
                    let interior = y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1;
                    self.place(c, if interior { fill } else { edge }, x, y, z)?;
                }
            }
        }
        Ok(())
    }

    /// `maybeGenerateBlock`.
    pub fn maybe_block(
        &self,
        c: &mut PieceContext<'_>,
        probability: f32,
        x: i32,
        y: i32,
        z: i32,
        state: BlockState,
    ) -> Result<()> {
        if c.random.float() < probability {
            self.place(c, state, x, y, z)?;
        }
        Ok(())
    }

    /// `generateUpperHalfSphere`.
    pub fn upper_half_sphere(
        &self,
        c: &mut PieceContext<'_>,
        [x0, y0, z0]: [i32; 3],
        [x1, y1, z1]: [i32; 3],
        fill: BlockState,
        skip_air: bool,
    ) -> Result<()> {
        let dx = (x1 - x0 + 1) as f32;
        let dy = (y1 - y0 + 1) as f32;
        let dz = (z1 - z0 + 1) as f32;
        let cx = x0 as f32 + dx / 2.0;
        let cz = z0 as f32 + dz / 2.0;
        for y in y0..=y1 {
            let ny = (y - y0) as f32 / dy;
            for x in x0..=x1 {
                let nx = (x as f32 - cx) / (dx * 0.5);
                for z in z0..=z1 {
                    let nz = (z as f32 - cz) / (dz * 0.5);
                    if self.skipped(c, skip_air, x, y, z)? {
                        continue;
                    }
                    if nx * nx + ny * ny + nz * nz <= 1.05 {
                        self.place(c, fill, x, y, z)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// `fillColumnDown`.
    pub fn fill_column_down(
        &self,
        c: &mut PieceContext<'_>,
        state: BlockState,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<()> {
        let mut pos = self.world_pos(x, y, z);
        if !c.chunk_box.is_inside(pos) {
            return Ok(());
        }
        let min = c.world.min_y();
        loop {
            let current = c.world.block(pos)?;
            if !c.replaceable_by_structures(current) || pos[1] <= min + 1 {
                break;
            }
            c.world.set(pos, state, UPDATE_CLIENTS)?;
            pos[1] -= 1;
        }
        Ok(())
    }

    /// `createChest` at piece coordinates.
    pub fn create_chest(
        &self,
        c: &mut PieceContext<'_>,
        x: i32,
        y: i32,
        z: i32,
        table: &str,
    ) -> Result<bool> {
        create_chest(c, self.world_pos(x, y, z), table, None)
    }

    /// `createDispenser`.
    pub fn create_dispenser(
        &self,
        c: &mut PieceContext<'_>,
        x: i32,
        y: i32,
        z: i32,
        facing: Direction,
        table: &str,
    ) -> Result<bool> {
        let pos = self.world_pos(x, y, z);
        if !c.chunk_box.is_inside(pos) || c.is_block(pos, "minecraft:dispenser")? {
            return Ok(false);
        }
        let mut state = c.state_with("dispenser", &[("facing", direction_name(facing))]);
        let (mirror, rotation) = self.mirror_rotation();
        if mirror != Mirror::None {
            state = c.world.data().mirror(state, mirror);
        }
        if rotation != Rotation::None {
            state = c.world.data().rotate(state, rotation);
        }
        c.world.set(pos, state, UPDATE_CLIENTS)?;
        c.loot_table(pos, "minecraft:dispenser", table);
        Ok(true)
    }
}

/// `createChest` at an absolute position, with the state to use or `reorient`'s.
pub fn create_chest(
    c: &mut PieceContext<'_>,
    pos: [i32; 3],
    table: &str,
    state: Option<BlockState>,
) -> Result<bool> {
    if !c.chunk_box.is_inside(pos) || c.is_block(pos, "minecraft:chest")? {
        return Ok(false);
    }
    let state = match state {
        Some(s) => s,
        None => {
            let chest = c.state("chest");
            reorient(c, pos, chest)?
        }
    };
    c.world.set(pos, state, UPDATE_CLIENTS)?;
    c.loot_table(pos, "minecraft:chest", table);
    Ok(true)
}

/// `StructurePiece.reorient`.
fn reorient(c: &mut PieceContext<'_>, pos: [i32; 3], state: BlockState) -> Result<BlockState> {
    let mut solid: Option<Direction> = None;
    for d in HORIZONTAL {
        let n = c.world.block(offset(pos, d, 1))?;
        if c.name(n) == "minecraft:chest" {
            return Ok(state);
        }
        if c.is_solid_render(n) {
            if solid.is_some() {
                solid = None;
                break;
            }
            solid = Some(d);
        }
    }
    if let Some(d) = solid {
        return Ok(c.with_property(state, "facing", direction_name(d.opposite())));
    }
    let mut facing = c
        .property(state, "facing")
        .and_then(direction_by_name)
        .unwrap_or(Direction::North);
    let solid_at = |c: &mut PieceContext<'_>, d: Direction| -> Result<bool> {
        let s = c.world.block(offset(pos, d, 1))?;
        Ok(c.is_solid_render(s))
    };
    if solid_at(c, facing)? {
        facing = facing.opposite();
    }
    if solid_at(c, facing)? {
        facing = clockwise(facing);
    }
    if solid_at(c, facing)? {
        facing = facing.opposite();
    }
    Ok(c.with_property(state, "facing", direction_name(facing)))
}

/// Every piece kind that places blocks in code (vanilla's `StructurePiece` subclasses), in one
/// enum instead of a class per piece. Saved and loaded by piece id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyPiece {
    DesertPyramid(DesertPyramidPiece),
    JungleTemple(JungleTemplePiece),
    SwampHut(SwampHutPiece),
    BuriedTreasure(BuriedTreasurePiece),
    Mineshaft(MineshaftPiece),
    Fortress(FortressPiece),
    Stronghold(StrongholdPiece),
    Monument(MonumentBuilding),
}

impl LegacyPiece {
    /// `getBoundingBox`.
    pub fn bounding_box(&self) -> BoundingBox {
        match self {
            Self::DesertPyramid(p) => p.base.base.bounding_box,
            Self::JungleTemple(p) => p.base.base.bounding_box,
            Self::SwampHut(p) => p.base.base.bounding_box,
            Self::BuriedTreasure(p) => p.bounding_box(),
            Self::Mineshaft(p) => p.bounding_box(),
            Self::Fortress(p) => p.bounding_box(),
            Self::Stronghold(p) => p.bounding_box(),
            Self::Monument(p) => p.bounding_box(),
        }
    }

    /// `StructurePiece.createTag`.
    pub fn create_tag(&self) -> Compound {
        match self {
            Self::DesertPyramid(p) => p.create_tag(),
            Self::JungleTemple(p) => p.create_tag(),
            Self::SwampHut(p) => p.create_tag(),
            Self::BuriedTreasure(p) => p.create_tag(),
            Self::Mineshaft(p) => p.create_tag(),
            Self::Fortress(p) => p.create_tag(),
            Self::Stronghold(p) => p.create_tag(),
            Self::Monument(p) => p.create_tag(),
        }
    }

    /// `StructurePieceType` lookup by id, then the piece's constructor from its tag: `None` for
    /// an id this module does not make (kept as saved) or a tag that fails to load.
    pub fn load(id: &str, tag: &Compound) -> Option<Self> {
        Some(match id {
            "minecraft:tedp" => Self::DesertPyramid(DesertPyramidPiece::load(tag)?),
            "minecraft:tejp" => Self::JungleTemple(JungleTemplePiece::load(tag)?),
            "minecraft:tesh" => Self::SwampHut(SwampHutPiece::load(tag)?),
            "minecraft:btp" => Self::BuriedTreasure(BuriedTreasurePiece::load(tag)?),
            "minecraft:mscorridor"
            | "minecraft:mscrossing"
            | "minecraft:msroom"
            | "minecraft:msstairs" => Self::Mineshaft(MineshaftPiece::load(id, tag)?),
            _ if FortressPiece::handles(id) => Self::Fortress(FortressPiece::load(id, tag)?),
            MonumentBuilding::TYPE_ID => Self::Monument(MonumentBuilding::load(tag)?),
            _ if StrongholdPiece::handles(id) => Self::Stronghold(StrongholdPiece::load(id, tag)?),
            _ => return None,
        })
    }

    /// Whether `id` is a piece id of this module (loaded here, or dropped when it fails).
    pub fn handles(id: &str) -> bool {
        id == MonumentBuilding::TYPE_ID
            || FortressPiece::handles(id)
            || StrongholdPiece::handles(id)
            || matches!(
                id,
                "minecraft:tedp"
                    | "minecraft:tejp"
                    | "minecraft:tesh"
                    | "minecraft:btp"
                    | "minecraft:mscorridor"
                    | "minecraft:mscrossing"
                    | "minecraft:msroom"
                    | "minecraft:msstairs"
            )
    }

    /// `postProcess`.
    pub fn post_process(&mut self, c: &mut PieceContext<'_>) -> Result<()> {
        match self {
            Self::DesertPyramid(p) => p.post_process(c),
            Self::JungleTemple(p) => p.post_process(c),
            Self::SwampHut(p) => p.post_process(c),
            Self::BuriedTreasure(p) => p.post_process(c),
            Self::Mineshaft(p) => p.post_process(c),
            Self::Fortress(p) => p.post_process(c),
            Self::Stronghold(p) => p.post_process(c),
            Self::Monument(p) => p.post_process(c),
        }
    }
}

/// `StructureStart.placeInChunk` for the code-placed pieces among `pieces` (the others are
/// skipped): each piece meeting the chunk's box is post-processed in order, then the structure's
/// `afterPlace` runs. Pieces keep what placing changed (a settled height, chests placed); a
/// caller that wants it kept hands in the start's own pieces.
#[allow(clippy::too_many_arguments)]
pub fn place_pieces<W: FeatureWorld, R: RandomSource>(
    pieces: &mut [StructurePiece],
    structure_kind: &str,
    world: &mut W,
    random: &mut R,
    chunk_box: StructureBounds,
    reference: BlockPos,
    level: PieceLevel<'_>,
    out: &mut PieceOutput,
) -> Result<()> {
    let chunk_box = BoundingBox::from_bounds(chunk_box);
    let chunk = ChunkPos::new(chunk_box.min[0] >> 4, chunk_box.min[2] >> 4);
    let mut c = PieceContext {
        world,
        random,
        out,
        chunk_box,
        chunk,
        reference,
        seed: level.seed,
        mineshaft_blocking: level.mineshaft_blocking,
    };
    for piece in pieces.iter_mut() {
        if let StructurePiece::Legacy(piece) = piece
            && piece.bounding_box().intersects(&chunk_box)
        {
            piece.post_process(&mut c)?;
        }
    }
    if structure_kind == "minecraft:desert_pyramid" {
        let mut boxes = pieces
            .iter()
            .filter_map(StructurePiece::bounding_box)
            .map(BoundingBox::from_bounds);
        if let Some(first) = boxes.next() {
            let mut all = first;
            boxes.for_each(|b| all.encapsulate(&b));
            let center = all.center();
            for piece in pieces.iter() {
                if let StructurePiece::Legacy(LegacyPiece::DesertPyramid(piece)) = piece {
                    piece.after_place(&mut c, center)?;
                }
            }
        }
    }
    Ok(())
}

/// `StructurePiece`'s box for a piece placed at `foot` facing `direction` (`BoundingBox.orientBox`):
/// `offset` is relative to the foot, `size` is width, height, depth.
pub fn orient_box(
    [x, y, z]: [i32; 3],
    [ox, oy, oz]: [i32; 3],
    [w, h, d]: [i32; 3],
    direction: Direction,
) -> BoundingBox {
    match direction {
        Direction::North => BoundingBox::new(
            [x + ox, y + oy, z - d + 1 + oz],
            [x + w - 1 + ox, y + h - 1 + oy, z + oz],
        ),
        Direction::West => BoundingBox::new(
            [x - d + 1 + oz, y + oy, z + ox],
            [x + oz, y + h - 1 + oy, z + w - 1 + ox],
        ),
        Direction::East => BoundingBox::new(
            [x + oz, y + oy, z + ox],
            [x + d - 1 + oz, y + h - 1 + oy, z + w - 1 + ox],
        ),
        _ => BoundingBox::new(
            [x + ox, y + oy, z + oz],
            [x + w - 1 + ox, y + h - 1 + oy, z + d - 1 + oz],
        ),
    }
}
