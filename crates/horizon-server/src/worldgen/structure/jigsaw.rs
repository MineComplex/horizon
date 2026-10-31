//! Jigsaw structures: `JigsawStructure` (its codec and `findGenerationPoint`), `JigsawPlacement.addPieces`
//! (the start piece, then the priority queue of pieces whose jigsaws take children from their pools)
//! and `PoolElementStructurePiece` as plain owned data ([`JigsawPiece`]).
//!
//! Vanilla shares each region of free space (`MutableObject<VoxelShape>`) between the pieces placed
//! into it; here the free spaces are an arena owned by the assembly ([`Free`], by index). A free
//! space is a box minus the boxes placed in it, which is exactly the voxel shape vanilla builds
//! (`Shapes.join(ONLY_FIRST)` of integer boxes), tested the same way (`joinIsNotEmpty(ONLY_SECOND)`
//! of the 0.25-deflated box: inside the container and meeting no placed box).
//!
//! Heights (`ChunkGenerator.getFirstFreeHeight`) and biome checks come from a [`JigsawWorld`]; random
//! draws are vanilla's, in its order. Ported from Horizon's `horizon-world::worldgen::structure::jigsaw`
//! without its modpack placements.

use super::place::LiquidSettings;
use super::pool::{
    ElementKind, PoolContext, PoolElementId, PoolId, Projection, TemplatePools, decode_liquid,
    template_of,
};
use super::template::{JointType, StructureTemplates, palette_at, transform};
use crate::block::BlockStates;
use crate::block::transform::{Mirror, Rotation};
use crate::core::geometry::Direction;
use crate::core::position::{BlockPos, ChunkPos};
use crate::util::random::{InvalidBound, LegacyRandom, PositionalRandom, RandomSource};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::codec::{
    DecodeError, Object, boolean, identifier, int_range, object, optional, required, string,
};
use crate::worldgen::density::{StructureBounds, TerrainAdjustment};
use crate::worldgen::placement::HeightProvider;
use serde_json::value::RawValue;

/// `PoolAliasBinding`.
#[derive(Clone, Debug, PartialEq)]
pub enum AliasBinding {
    Direct {
        alias: String,
        target: String,
    },
    Random {
        alias: String,
        targets: Vec<(String, i32)>,
    },
    RandomGroup(Vec<(Vec<AliasBinding>, i32)>),
}

/// A `minecraft:jigsaw` structure's own fields.
#[derive(Clone, Debug)]
pub struct JigsawStructure {
    pub start_pool: PoolId,
    pub start_jigsaw_name: Option<String>,
    pub max_depth: i32,
    pub start_height: HeightProvider,
    pub use_expansion_hack: bool,
    pub project_start_to_heightmap: Option<Heightmap>,
    /// `max_distance_from_center` (horizontal, vertical).
    pub max_distance: (i32, i32),
    pub pool_aliases: Vec<AliasBinding>,
    /// `dimension_padding` (bottom, top).
    pub dimension_padding: (i32, i32),
    pub liquid: LiquidSettings,
}

/// `JigsawJunction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
    pub delta_y: i32,
    pub dest_projection: Projection,
}

/// `PoolElementStructurePiece`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JigsawPiece {
    pub element: PoolElementId,
    pub position: BlockPos,
    pub ground_level_delta: i32,
    pub rotation: Rotation,
    pub bounding_box: StructureBounds,
    pub junctions: Vec<Junction>,
    pub liquid: LiquidSettings,
}

/// What the assembly asks the level: `getFirstFreeHeight` and the column biome check.
pub trait JigsawWorld {
    /// `ChunkGenerator.getFirstFreeHeight(x, z, heightmap, heightAccessor, randomState)`.
    fn first_free_height(&mut self, x: i32, z: i32, heightmap: Heightmap) -> i32;
    /// `GenerationContext.couldStructureExistInColumn(x, z, minY, maxY)`.
    fn could_exist_in_column(&mut self, x: i32, z: i32, min_y: i32, max_y: i32) -> bool;
}

/// The tables the assembly reads.
#[derive(Clone, Copy)]
pub struct JigsawTables<'a> {
    pub pools: &'a TemplatePools,
    pub templates: &'a StructureTemplates,
    pub blocks: &'a BlockStates,
}

fn heightmap(name: &str) -> Result<Heightmap, DecodeError> {
    (Heightmap::ALL.into_iter())
        .find(|h| h.key() == name)
        .ok_or(DecodeError::Field("heightmap"))
}

fn weighted<T>(
    raw: &RawValue,
    mut data: impl FnMut(&RawValue) -> Result<T, DecodeError>,
) -> Result<Vec<(T, i32)>, DecodeError> {
    let entries: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
    if entries.is_empty() {
        return Err(DecodeError::Field("weighted list"));
    }
    entries
        .iter()
        .map(|entry| {
            let fields = object(entry)?;
            Ok((
                data(required(&fields, "data")?)?,
                int_range(required(&fields, "weight")?, "weight", 0, i32::MAX)?,
            ))
        })
        .collect()
}

fn decode_alias(raw: &RawValue) -> Result<AliasBinding, DecodeError> {
    let fields = object(raw)?;
    let pool = |name: &'static str| identifier(&string(required(&fields, name)?)?);
    match identifier(&string(required(&fields, "type")?)?)?.as_str() {
        "minecraft:direct" => Ok(AliasBinding::Direct {
            alias: pool("alias")?,
            target: pool("target")?,
        }),
        "minecraft:random" => Ok(AliasBinding::Random {
            alias: pool("alias")?,
            targets: weighted(required(&fields, "targets")?, |r| identifier(&string(r)?))?,
        }),
        "minecraft:random_group" => Ok(AliasBinding::RandomGroup(weighted(
            required(&fields, "groups")?,
            |r| {
                let list: Vec<Box<RawValue>> = serde_json::from_str(r.get())?;
                list.iter().map(|b| decode_alias(b)).collect()
            },
        )?)),
        _ => Err(DecodeError::Field("pool alias type")),
    }
}

/// `either(int, record)`: an int is both values.
fn int_or_pair(
    raw: &RawValue,
    int: impl Fn(&RawValue) -> Result<i32, DecodeError>,
    pair: impl Fn(&Object) -> Result<(i32, i32), DecodeError>,
) -> Result<(i32, i32), DecodeError> {
    if raw.get().trim_start().starts_with('{') {
        pair(&object(raw)?)
    } else {
        int(raw).map(|v| (v, v))
    }
}

impl JigsawStructure {
    /// `JigsawStructure.CODEC` (with `verifyRange`).
    pub(crate) fn decode(
        fields: &Object,
        terrain_adaptation: TerrainAdjustment,
        pools: &mut TemplatePools,
        cx: &PoolContext<'_>,
    ) -> Result<Self, DecodeError> {
        let max_distance = int_or_pair(
            required(fields, "max_distance_from_center")?,
            |r| int_range(r, "max_distance_from_center", 1, 128),
            |f| {
                Ok((
                    int_range(required(f, "horizontal")?, "horizontal", 1, 128)?,
                    optional(f, "vertical")
                        .map_or(Ok(4064), |r| int_range(r, "vertical", 1, 4064))?,
                ))
            },
        )?;
        let padding = int_or_pair(
            optional(fields, "dimension_padding")
                .unwrap_or(RawValue::from_string("0".into())?.as_ref()),
            |r| int_range(r, "dimension_padding", 0, i32::MAX),
            |f| {
                // `lenientOptionalFieldOf`: a bad value is the default.
                let lenient = |name| {
                    optional(f, name)
                        .and_then(|r| int_range(r, "padding", 0, i32::MAX).ok())
                        .unwrap_or(0)
                };
                Ok((lenient("bottom"), lenient("top")))
            },
        )?;
        let aliases = match optional(fields, "pool_aliases") {
            None => Vec::new(),
            Some(raw) => {
                let list: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
                list.iter()
                    .map(|b| decode_alias(b))
                    .collect::<Result<_, _>>()?
            }
        };
        let structure = Self {
            start_pool: pools.holder(required(fields, "start_pool")?, cx)?,
            start_jigsaw_name: optional(fields, "start_jigsaw_name")
                .map(|r| identifier(&string(r)?))
                .transpose()?,
            max_depth: int_range(required(fields, "size")?, "size", 0, 20)?,
            start_height: HeightProvider::decode(required(fields, "start_height")?)?,
            use_expansion_hack: boolean(required(fields, "use_expansion_hack")?)?,
            project_start_to_heightmap: optional(fields, "project_start_to_heightmap")
                .map(|r| heightmap(&string(r)?))
                .transpose()?,
            max_distance,
            pool_aliases: aliases,
            dimension_padding: padding,
            liquid: optional(fields, "liquid_settings")
                .map(decode_liquid)
                .transpose()?
                .unwrap_or_default(),
        };
        let edge = if terrain_adaptation == TerrainAdjustment::None {
            0
        } else {
            12
        };
        if structure.max_distance.0 + edge > 128 {
            return Err(DecodeError::Registry(
                "Horizontal structure size including terrain adaptation must not exceed 128".into(),
            ));
        }
        Ok(structure)
    }
}

/// `WeightedList.getRandomOrThrow`.
fn pick<'a, T>(
    list: &'a [(T, i32)],
    random: &mut impl RandomSource,
) -> Result<&'a T, InvalidBound> {
    let total = list.iter().map(|(_, w)| *w).sum::<i32>();
    let mut selection = random.next_int_bound(total)?;
    for (value, weight) in list {
        selection -= weight;
        if selection < 0 {
            return Ok(value);
        }
    }
    Err(InvalidBound)
}

fn resolve_alias(
    binding: &AliasBinding,
    random: &mut impl RandomSource,
    out: &mut Vec<(String, String)>,
) -> Result<(), InvalidBound> {
    match binding {
        AliasBinding::Direct { alias, target } => out.push((alias.clone(), target.clone())),
        AliasBinding::Random { alias, targets } => {
            out.push((alias.clone(), pick(targets, random)?.clone()))
        }
        AliasBinding::RandomGroup(groups) => {
            for binding in pick(groups, random)? {
                resolve_alias(binding, random, out)?;
            }
        }
    }
    Ok(())
}

/// `PoolAliasLookup.create(bindings, pos, seed)`: alias to target pool names.
pub fn alias_lookup(
    bindings: &[AliasBinding],
    pos: BlockPos,
    seed: i64,
) -> Result<Vec<(String, String)>, InvalidBound> {
    let mut out = Vec::new();
    if bindings.is_empty() {
        return Ok(out);
    }
    let mut random = LegacyRandom::new(seed).fork_positional().at(pos);
    for binding in bindings {
        resolve_alias(binding, &mut random, &mut out)?;
    }
    Ok(out)
}

/// A jigsaw block of an element, moved and turned (`JigsawBlockInfo`, borrowed from the template).
#[derive(Clone, Copy, Debug)]
struct Connector<'t> {
    pos: BlockPos,
    front: Direction,
    top: Direction,
    joint: JointType,
    /// `None` for a feature element's (`canAttach` then takes any target).
    name: Option<&'t str>,
    pool: &'t str,
    target: &'t str,
    placement_priority: i32,
    selection_priority: i32,
}

fn direction(name: &str) -> Direction {
    match name {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => Direction::North,
    }
}

/// `JigsawBlock.canAttach`.
fn can_attach(source: &Connector<'_>, target: &Connector<'_>) -> bool {
    source.front == target.front.opposite()
        && (source.joint == JointType::Rollable || source.top == target.top)
        && target.name.is_none_or(|name| source.target == name)
}

fn step(direction: Direction) -> [i32; 3] {
    direction.normal()
}

fn inside(b: StructureBounds, p: BlockPos) -> bool {
    b.contains([p.x, p.y, p.z])
}

fn moved_y(b: StructureBounds, dy: i32) -> StructureBounds {
    let (min, max) = (b.minimum(), b.maximum());
    StructureBounds::from_corners([min[0], min[1] + dy, min[2]], [max[0], max[1] + dy, max[2]])
}

fn intersects(a: StructureBounds, b: StructureBounds) -> bool {
    let (amin, amax, bmin, bmax) = (a.minimum(), a.maximum(), b.minimum(), b.maximum());
    (0..3).all(|i| amax[i] >= bmin[i] && amin[i] <= bmax[i])
}

impl JigsawTables<'_> {
    /// `getShuffledJigsawBlocks(manager, position, rotation, random)`.
    fn shuffled_jigsaws(
        &self,
        element: PoolElementId,
        position: BlockPos,
        rotation: Rotation,
        random: &mut impl RandomSource,
    ) -> Result<Vec<Connector<'_>>, InvalidBound> {
        match &self.pools.element(element).kind {
            ElementKind::Empty => Ok(Vec::new()),
            ElementKind::List(children) => {
                self.shuffled_jigsaws(children[0], position, rotation, random)
            }
            ElementKind::Feature { .. } => Ok(vec![Connector {
                pos: position,
                front: Direction::Down,
                top: Direction::South,
                joint: JointType::Rollable,
                name: None,
                pool: super::pool::EMPTY_POOL,
                target: super::pool::EMPTY_POOL,
                placement_priority: 0,
                selection_priority: 0,
            }]),
            ElementKind::Single { location, .. } => {
                let template = template_of(self.templates, location);
                let Some(palette) = palette_at(template.palettes().len(), position) else {
                    return Ok(Vec::new());
                };
                let mut list: Vec<Connector<'_>> = template.palettes()[palette]
                    .jigsaws()
                    .iter()
                    .map(|jigsaw| {
                        let orientation = self
                            .blocks
                            .value(jigsaw.state, "orientation")
                            .unwrap_or("north_up");
                        let (front, top) = orientation.split_once('_').unwrap_or(("north", "up"));
                        let pos =
                            transform(jigsaw.pos, Mirror::None, rotation, BlockPos::default());
                        Connector {
                            pos: pos.offset(position.x, position.y, position.z),
                            front: rotation.rotate(direction(front)),
                            top: rotation.rotate(direction(top)),
                            joint: jigsaw.joint,
                            name: Some(jigsaw.name.as_str()),
                            pool: jigsaw.pool.as_str(),
                            target: jigsaw.target.as_str(),
                            placement_priority: jigsaw.placement_priority,
                            selection_priority: jigsaw.selection_priority,
                        }
                    })
                    .collect();
                shuffle(&mut list, random)?;
                list.sort_by_key(|c| std::cmp::Reverse(c.selection_priority));
                Ok(list)
            }
        }
    }

    fn bounding_box(
        &self,
        element: PoolElementId,
        position: BlockPos,
        rotation: Rotation,
    ) -> StructureBounds {
        self.pools
            .bounding_box(self.templates, element, position, rotation)
    }

    /// `getGroundLevelDelta`.
    fn ground_level_delta(&self, _element: PoolElementId) -> i32 {
        1
    }

    fn is_empty(&self, element: PoolElementId) -> bool {
        matches!(self.pools.element(element).kind, ElementKind::Empty)
    }
}

/// `Util.shuffle`.
pub(crate) fn shuffle<T>(
    values: &mut [T],
    random: &mut impl RandomSource,
) -> Result<(), InvalidBound> {
    for i in (2..=values.len()).rev() {
        let swap = random.next_int_bound(i as i32)? as usize;
        values.swap(i - 1, swap);
    }
    Ok(())
}

/// A free space: a box (`None`: an empty shape) minus the boxes placed in it.
#[derive(Debug)]
struct Free {
    container: Option<StructureBounds>,
    removed: Vec<StructureBounds>,
}

impl Free {
    fn fits(&self, b: StructureBounds) -> bool {
        self.container.is_some_and(|c| {
            (0..3).all(|i| b.minimum()[i] >= c.minimum()[i] && b.maximum()[i] <= c.maximum()[i])
        }) && !self.removed.iter().any(|&r| intersects(r, b))
    }
}

/// What `findGenerationPoint` needs of the generation context.
pub struct GenerationContext<'a, W> {
    pub tables: JigsawTables<'a>,
    pub world: &'a mut W,
    /// The level's height (`heightAccessor.getMinY`, `getHeight`).
    pub min_y: i32,
    pub height: i32,
    /// `new WorldGenerationContext(generator, heightAccessor)`.
    pub generation: WorldGenerationContext,
    pub seed: i64,
    pub chunk: ChunkPos,
}

/// A start: the stub's position and the pieces, the centre piece first.
#[derive(Clone, Debug, PartialEq)]
pub struct JigsawStart {
    pub position: BlockPos,
    pub pieces: Vec<JigsawPiece>,
}

/// `JigsawStructure.findGenerationPoint` followed by `isValidBiome` (`valid_biome` at the stub's
/// position) and the pieces builder. `random` is the context's (`setLargeFeatureSeed`).
pub fn generate<W: JigsawWorld>(
    structure: &JigsawStructure,
    cx: &mut GenerationContext<'_, W>,
    random: &mut impl RandomSource,
    valid_biome: impl FnOnce(&mut W, BlockPos) -> bool,
) -> Result<Option<JigsawStart>, InvalidBound> {
    let height = structure.start_height.sample(random, cx.generation)?;
    let start = BlockPos::new(cx.chunk.x << 4, height, cx.chunk.z << 4);
    let aliases = alias_lookup(&structure.pool_aliases, start, cx.seed)?;
    let tables = cx.tables;
    let alias = |name: &str| -> Option<PoolId> {
        let name = (aliases.iter().find(|(a, _)| a == name)).map_or(name, |(_, t)| t.as_str());
        tables.pools.id(name)
    };
    let center_rotation = Rotation::ALL[random.next_int_bound(4)? as usize];
    // `startPool.unwrapKey().flatMap(lookup).orElse(startPool.value())`.
    let start_name = tables.pools.pool(structure.start_pool).id.as_str();
    let start_pool = if start_name.starts_with("<inline") {
        structure.start_pool
    } else {
        alias(start_name).unwrap_or(structure.start_pool)
    };
    let pool = tables.pools.pool(start_pool);
    let center = if pool.templates.is_empty() {
        None
    } else {
        Some(pool.templates[random.next_int_bound(pool.templates.len() as i32)? as usize])
    };
    let Some(center) = center.filter(|&e| !tables.is_empty(e)) else {
        return Ok(None);
    };
    let anchor = match &structure.start_jigsaw_name {
        Some(name) => {
            let jigsaws = tables.shuffled_jigsaws(center, start, center_rotation, random)?;
            match jigsaws.iter().find(|j| j.name == Some(name.as_str())) {
                Some(j) => j.pos,
                None => {
                    tracing::error!(
                        "No starting jigsaw {name} found in start pool {}",
                        tables.pools.pool(structure.start_pool).id
                    );
                    return Ok(None);
                }
            }
        }
        None => start,
    };
    let local = BlockPos::new(anchor.x - start.x, anchor.y - start.y, anchor.z - start.z);
    let adjusted = BlockPos::new(start.x - local.x, start.y - local.y, start.z - local.z);
    let mut center_piece = JigsawPiece {
        element: center,
        position: adjusted,
        ground_level_delta: tables.ground_level_delta(center),
        rotation: center_rotation,
        bounding_box: tables.bounding_box(center, adjusted, center_rotation),
        junctions: Vec::new(),
        liquid: structure.liquid,
    };
    let b = center_piece.bounding_box;
    let center_x = (b.maximum()[0] + b.minimum()[0]) / 2;
    let center_z = (b.maximum()[2] + b.minimum()[2]) / 2;
    let max_y = cx.min_y + cx.height - 1;
    let bottom_y = match structure.project_start_to_heightmap {
        None => adjusted.y,
        Some(kind) => {
            if !cx
                .world
                .could_exist_in_column(center_x, center_z, cx.min_y, max_y)
            {
                return Ok(None);
            }
            start.y + cx.world.first_free_height(center_x, center_z, kind)
        }
    };
    let ground = b.minimum()[1] + center_piece.ground_level_delta;
    let dy = bottom_y - ground;
    center_piece.bounding_box = moved_y(center_piece.bounding_box, dy);
    center_piece.position = center_piece.position.offset(0, dy, 0);
    let (bottom, top) = structure.dimension_padding;
    if (bottom, top) != (0, 0) {
        let moved = center_piece.bounding_box;
        if moved.minimum()[1] < cx.min_y + bottom || moved.maximum()[1] > max_y - top {
            return Ok(None);
        }
    }
    let center_y = bottom_y + local.y;
    let position = BlockPos::new(center_x, center_y, center_z);
    if !valid_biome(cx.world, position) {
        return Ok(None);
    }
    let mut pieces = vec![center_piece];
    if structure.max_depth > 0 {
        let (h, v) = structure.max_distance;
        let y0 = (center_y - v).max(cx.min_y + bottom);
        let y1 = (center_y + v + 1).min(max_y + 1 - top);
        let container = (y0 != y1).then(|| {
            StructureBounds::from_corners(
                [center_x - h, y0.min(y1), center_z - h],
                [center_x + h, y0.max(y1) - 1, center_z + h],
            )
        });
        let mut placer = Placer {
            tables,
            alias: &alias,
            structure,
            world: &mut *cx.world,
            pieces: &mut pieces,
            frees: vec![Free {
                container,
                removed: vec![moved_y(b, dy)],
            }],
            queue: std::collections::BTreeMap::new(),
            sequence: 0,
        };
        placer.children(0, 0, 0, random)?;
        while let Some(state) = placer.next() {
            placer.children(state.piece, state.free, state.depth, random)?;
        }
    }
    Ok(Some(JigsawStart { position, pieces }))
}

/// `PieceState`: a piece of the list, its free space and depth.
#[derive(Clone, Copy, Debug)]
struct PieceState {
    piece: usize,
    free: usize,
    depth: i32,
}

struct Placer<'a, 'p, W> {
    tables: JigsawTables<'a>,
    alias: &'p dyn Fn(&str) -> Option<PoolId>,
    structure: &'a JigsawStructure,
    world: &'p mut W,
    pieces: &'p mut Vec<JigsawPiece>,
    frees: Vec<Free>,
    /// `SequencedPriorityIterator`: highest priority first, then insertion order.
    queue: std::collections::BTreeMap<(std::cmp::Reverse<i32>, u64), PieceState>,
    sequence: u64,
}

impl<W: JigsawWorld> Placer<'_, '_, W> {
    fn next(&mut self) -> Option<PieceState> {
        self.queue.pop_first().map(|(_, state)| state)
    }

    fn pool(&self, name: &str) -> Option<PoolId> {
        (self.alias)(name)
    }

    fn base_height(&mut self, cached: &mut i32, pos: BlockPos) -> i32 {
        if *cached == i32::MIN {
            *cached = self
                .world
                .first_free_height(pos.x, pos.z, Heightmap::WorldSurfaceWg);
        }
        *cached
    }

    /// `tryPlacingChildren`.
    fn children(
        &mut self,
        source: usize,
        context_free: usize,
        depth: i32,
        random: &mut impl RandomSource,
    ) -> Result<(), InvalidBound> {
        let tables = self.tables;
        let pools = tables.pools;
        let max_depth = self.structure.max_depth;
        let liquid = self.structure.liquid;
        let expansion_hack = self.structure.use_expansion_hack;
        let s = &self.pieces[source];
        let (source_element, source_position, source_rotation, source_box, source_ground) = (
            s.element,
            s.position,
            s.rotation,
            s.bounding_box,
            s.ground_level_delta,
        );
        let source_projection = pools.element(source_element).projection;
        let source_rigid = source_projection == Projection::Rigid;
        let mut source_free: Option<usize> = None;
        let source_y = source_box.minimum()[1];
        let jigsaws =
            tables.shuffled_jigsaws(source_element, source_position, source_rotation, random)?;
        'jigsaws: for jigsaw in &jigsaws {
            let [sx, sy, sz] = step(jigsaw.front);
            let target_pos = jigsaw.pos.offset(sx, sy, sz);
            let local_y = jigsaw.pos.y - source_y;
            let mut base_height = i32::MIN;
            let Some(target_pool) = self.pool(jigsaw.pool) else {
                tracing::warn!("Empty or non-existent pool: {}", jigsaw.pool);
                continue;
            };
            let target = pools.pool(target_pool);
            if target.templates.is_empty() && !pools.is_empty_pool(target_pool) {
                tracing::warn!("Empty or non-existent pool: {}", jigsaw.pool);
                continue;
            }
            let fallback = pools.pool(target.fallback);
            if fallback.templates.is_empty() && !pools.is_empty_pool(target.fallback) {
                tracing::warn!("Empty or non-existent fallback pool: {}", fallback.id);
                continue;
            }
            let children_free = if inside(source_box, target_pos) {
                *source_free.get_or_insert_with(|| {
                    self.frees.push(Free {
                        container: Some(source_box),
                        removed: Vec::new(),
                    });
                    self.frees.len() - 1
                })
            } else {
                context_free
            };
            let mut candidates: Vec<PoolElementId> = Vec::new();
            if depth != max_depth {
                let mut list = target.templates.clone();
                shuffle(&mut list, random)?;
                candidates.extend(list);
            }
            let mut list = fallback.templates.clone();
            shuffle(&mut list, random)?;
            candidates.extend(list);
            let placement_priority = jigsaw.placement_priority;
            for target_element in candidates {
                if tables.is_empty(target_element) {
                    break;
                }
                let mut rotations = Rotation::ALL;
                shuffle(&mut rotations, random)?;
                for rotation in rotations {
                    let target_jigsaws = tables.shuffled_jigsaws(
                        target_element,
                        BlockPos::default(),
                        rotation,
                        random,
                    )?;
                    let hack_box =
                        tables.bounding_box(target_element, BlockPos::default(), rotation);
                    let span = hack_box.maximum()[1] - hack_box.minimum()[1] + 1;
                    let expand_to = if expansion_hack && span <= 16 {
                        (target_jigsaws.iter())
                            .map(|tj| {
                                let [dx, dy, dz] = step(tj.front);
                                if !inside(hack_box, tj.pos.offset(dx, dy, dz)) {
                                    return 0;
                                }
                                let child = self.pool(tj.pool);
                                let size = child.map_or(0, |p| pools.pool(p).max_size);
                                let fallback = child
                                    .map_or(0, |p| pools.pool(pools.pool(p).fallback).max_size);
                                size.max(fallback)
                            })
                            .max()
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    for target_jigsaw in &target_jigsaws {
                        if !can_attach(jigsaw, target_jigsaw) {
                            continue;
                        }
                        let target_local = target_jigsaw.pos;
                        let raw_position = BlockPos::new(
                            target_pos.x - target_local.x,
                            target_pos.y - target_local.y,
                            target_pos.z - target_local.z,
                        );
                        let raw_box = tables.bounding_box(target_element, raw_position, rotation);
                        let raw_y = raw_box.minimum()[1];
                        let target_projection = pools.element(target_element).projection;
                        let target_rigid = target_projection == Projection::Rigid;
                        let target_local_y = target_local.y;
                        let delta_y = local_y - target_local_y + step(jigsaw.front)[1];
                        let target_y = if source_rigid && target_rigid {
                            source_y + delta_y
                        } else {
                            self.base_height(&mut base_height, jigsaw.pos) - target_local_y
                        };
                        let offset = target_y - raw_y;
                        let mut target_box = moved_y(raw_box, offset);
                        let target_position = raw_position.offset(0, offset, 0);
                        if expand_to > 0 {
                            let (min, max) = (target_box.minimum(), target_box.maximum());
                            let size = (expand_to + 1).max(max[1] - min[1]);
                            target_box = StructureBounds::from_corners(
                                min,
                                [max[0], max[1].max(min[1] + size), max[2]],
                            );
                        }
                        let free = &mut self.frees[children_free];
                        if !free.fits(target_box) {
                            continue;
                        }
                        free.removed.push(target_box);
                        let target_ground = if target_rigid {
                            source_ground - delta_y
                        } else {
                            tables.ground_level_delta(target_element)
                        };
                        let junction_y = if source_rigid {
                            source_y + local_y
                        } else if target_rigid {
                            target_y + target_local_y
                        } else {
                            self.base_height(&mut base_height, jigsaw.pos) + delta_y / 2
                        };
                        self.pieces[source].junctions.push(Junction {
                            source_x: target_pos.x,
                            source_ground_y: junction_y - local_y + source_ground,
                            source_z: target_pos.z,
                            delta_y,
                            dest_projection: target_projection,
                        });
                        self.pieces.push(JigsawPiece {
                            element: target_element,
                            position: target_position,
                            ground_level_delta: target_ground,
                            rotation,
                            bounding_box: target_box,
                            junctions: vec![Junction {
                                source_x: jigsaw.pos.x,
                                source_ground_y: junction_y - target_local_y + target_ground,
                                source_z: jigsaw.pos.z,
                                delta_y: -delta_y,
                                dest_projection: source_projection,
                            }],
                            liquid,
                        });
                        if depth < max_depth {
                            self.queue.insert(
                                (std::cmp::Reverse(placement_priority), self.sequence),
                                PieceState {
                                    piece: self.pieces.len() - 1,
                                    free: children_free,
                                    depth: depth + 1,
                                },
                            );
                            self.sequence += 1;
                        }
                        continue 'jigsaws;
                    }
                }
            }
        }
        Ok(())
    }
}
