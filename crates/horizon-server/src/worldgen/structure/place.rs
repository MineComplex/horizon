//! Placing a template (`StructurePlaceSettings`, `StructureTemplate.placeInWorld`,
//! `processBlockInfos`, `filterBlocks`, `getJigsaws`) into a [`FeatureWorld`].
//!
//! What the world cannot hold yet comes back as data in [`PlacedTemplate`]: the block entities' NBT
//! (vanilla loads it into the block entity `WorldGenRegion.getBlockEntity` makes from the proto
//! chunk's `DUMMY` tag) and the entities (vanilla creates them with `EntityType.create` and adds
//! them to the proto chunk; their yaw turn, `Entity.rotate/mirror`, belongs to the entity port).
//! Block behaviour after the writes, `Block.updateFromNeighbourShapes` and `updateNeighborsAt`,
//! is WP6's; the edge walk calls [`FeatureWorld::update_shape_at_face`] as trees do.

use std::borrow::Cow;

use super::processor::{KnownBlocks, ProcessContext, ProcessError, Processor, SettingsRandom};
use super::template::{
    BlockInfo, JigsawInfo, StructureTemplate, palette_at, transform, transform_vec,
};
use crate::block::flags::{
    FLAG_HAS_BLOCK_ENTITY, FLAG_LIQUID_CONTAINER, FLAG_RANDOMIZABLE_CONTAINER,
};
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockId, BlockState, BlockStateData};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::nbt::{Compound, Tag};
use crate::util::random::RandomSource;
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::placement::{PlacementWorld, WorldError};

/// `Block.UPDATE_SKIP_ALL_SIDEEFFECTS | Block.UPDATE_NONE`: how the barrier goes in first.
const BARRIER_FLAGS: i32 = 820;
/// `WaterFluid.getTickDelay`.
const WATER_TICK_DELAY: i32 = 5;

/// `LiquidSettings`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidSettings {
    #[default]
    ApplyWaterlogging,
    IgnoreWaterlogging,
}

/// `StructurePlaceSettings`. The processors are borrowed from the registry where they live there
/// (`Cow::Borrowed`), or owned when a piece builds its own (`Cow::Owned`, e.g. a `block_rot`).
#[derive(Clone, Debug, Default)]
pub struct PlaceSettings<'a> {
    pub mirror: Mirror,
    pub rotation: Rotation,
    pub rotation_pivot: BlockPos,
    pub ignore_entities: bool,
    pub bounding_box: Option<StructureBounds>,
    pub liquid: LiquidSettings,
    /// CraftBukkit's forced palette (`palette >= 0`).
    pub palette: Option<usize>,
    pub processors: Vec<Cow<'a, Processor>>,
    pub known_shape: bool,
    pub finalize_entities: bool,
    /// `setRandom(random)` with the placement's own random (`FossilFeature`, `TemplateFeature`):
    /// the palette and the processors' `getRandom` draw from it. Otherwise each position seeds its
    /// own `RandomSource.create(Mth.getSeed(pos))`.
    pub shared_random: bool,
}

impl PlaceSettings<'_> {
    /// `calculateRelativePosition(settings, pos)`.
    pub fn relative_position(&self, pos: BlockPos) -> BlockPos {
        transform(pos, self.mirror, self.rotation, self.rotation_pivot)
    }
    fn inside(&self, pos: BlockPos) -> bool {
        self.bounding_box
            .is_none_or(|b| b.contains([pos.x, pos.y, pos.z]))
    }
    fn waterlogging(&self) -> bool {
        self.liquid == LiquidSettings::ApplyWaterlogging
    }
}

/// What the placement needs besides the world: the built-in processors' blocks and the level seed.
#[derive(Clone, Copy, Debug)]
pub struct PlaceContext<'a> {
    pub known: &'a KnownBlocks,
    pub seed: i64,
}

/// A palette index the settings force but the template lacks (vanilla throws).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(
        "Palette index out of bounds. Got {index} where there are only {size} palettes available."
    )]
    Palette { index: usize, size: usize },
}

impl From<WorldError> for PlaceError {
    fn from(e: WorldError) -> Self {
        Self::Process(e.into())
    }
}

/// An entity the template places: its NBT as vanilla hands it to `EntityType.create`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedEntity {
    pub pos: [f64; 3],
    pub block_pos: BlockPos,
    /// The template's tag with `Pos` set, `UUID` removed and `block_pos` moved.
    pub nbt: Compound,
    /// Applied by the entity (`Entity.rotate`, `Entity.mirror`) when it is created.
    pub rotation: Rotation,
    pub mirror: Mirror,
    /// `finalizeSpawn(..., STRUCTURE)` for a mob.
    pub finalize: bool,
}

/// What a placement left for the block entity and entity ports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacedTemplate {
    /// Each placed block entity's NBT, in placement order (with the `LootTableSeed` of a
    /// randomizable container).
    pub block_entities: Vec<(BlockPos, Compound)>,
    pub entities: Vec<PlacedEntity>,
}

/// `getRandomPalette(palettes, pos)` with the settings' random.
fn palette_index(
    template: &StructureTemplate,
    settings: &PlaceSettings<'_>,
    position: BlockPos,
    random: &mut impl RandomSource,
) -> Result<Option<usize>, PlaceError> {
    let size = template.palettes().len();
    if size == 0 {
        return Ok(None);
    }
    if let Some(index) = settings.palette {
        return if index < size {
            Ok(Some(index))
        } else {
            Err(PlaceError::Palette { index, size })
        };
    }
    Ok(if settings.shared_random {
        Some(
            random
                .next_int_bound(size as i32)
                .map_err(ProcessError::from)? as usize,
        )
    } else {
        palette_at(size, position)
    })
}

/// `StructureTemplate.processBlockInfos`: each block moved to the world and run through the
/// processors, then `finalizeProcessing` of each.
pub fn process_block_infos<W: PlacementWorld, R: RandomSource>(
    world: &W,
    cx: PlaceContext<'_>,
    position: BlockPos,
    reference: BlockPos,
    settings: &PlaceSettings<'_>,
    blocks: &[BlockInfo],
    random: &mut R,
) -> Result<Vec<BlockInfo>, ProcessError> {
    let only_in_chunk = !settings
        .processors
        .iter()
        .any(|p| p.evaluates_entire_piece());
    let pcx = ProcessContext {
        world,
        known: cx.known,
        position,
        reference,
        seed: cx.seed,
    };
    let mut settings_random = if settings.shared_random {
        SettingsRandom::Shared(random)
    } else {
        SettingsRandom::PerPosition
    };
    let mut original = Vec::new();
    let mut processed = Vec::new();
    for info in blocks {
        let pos = settings.relative_position(info.pos);
        let pos = pos.offset(position.x, position.y, position.z);
        if only_in_chunk && !settings.inside(pos) {
            continue;
        }
        let mut current = Some(BlockInfo {
            pos,
            state: info.state,
            nbt: info.nbt.clone(),
        });
        for processor in &settings.processors {
            let Some(block) = current.take() else { break };
            current = processor.process_block(&pcx, info.pos, block, &mut settings_random)?;
        }
        if let Some(block) = current {
            processed.push(block);
            original.push(info.clone());
        }
    }
    for processor in &settings.processors {
        processor.finalize(&pcx, &original, &mut processed, &mut settings_random)?;
    }
    Ok(processed)
}

fn fluid_source(data: &BlockStateData, state: BlockState) -> Option<&str> {
    let name = data.fluid_name(data.fluid(state).fluid);
    matches!(name, "minecraft:water" | "minecraft:lava").then_some(name)
}

/// `LiquidBlockContainer.placeLiquid(level, pos, state, fluid)`: only a water source waterlogs
/// (the block tables give the state it becomes, `BlockStateData::waterlogged`).
fn place_liquid<W: FeatureWorld>(
    world: &mut W,
    pos: BlockPos,
    state: BlockState,
    fluid: BlockState,
) -> Result<(), WorldError> {
    let data = world.state_data();
    let water = data.fluid(fluid);
    if data.fluid_name(water.fluid) != "minecraft:water" {
        return Ok(());
    }
    if let Some((wet, flags)) = data.waterlogged(state) {
        world.set_block(pos, wet, flags)?;
        world.schedule_fluid_tick(pos, water.fluid, WATER_TICK_DELAY)?;
    }
    Ok(())
}

/// `StructureTemplate.placeInWorld(level, position, referencePos, settings, random, updateMode)`:
/// `None` where vanilla returns false.
#[allow(clippy::too_many_arguments)]
pub fn place_in_world<W: FeatureWorld, R: RandomSource>(
    world: &mut W,
    cx: PlaceContext<'_>,
    template: &StructureTemplate,
    position: BlockPos,
    reference: BlockPos,
    settings: &PlaceSettings<'_>,
    random: &mut R,
    flags: i32,
) -> Result<Option<PlacedTemplate>, PlaceError> {
    let Some(palette) = palette_index(template, settings, position, random)? else {
        return Ok(None);
    };
    let blocks = template.palettes()[palette].blocks();
    let size = template.size();
    if (blocks.is_empty() && (settings.ignore_entities || template.entities().is_empty()))
        || size.iter().any(|&s| s < 1)
    {
        return Ok(None);
    }
    let waterlogging = settings.waterlogging();
    let processed =
        process_block_infos(&*world, cx, position, reference, settings, blocks, random)?;
    let barrier = world
        .blocks()
        .block_by_name("minecraft:barrier")
        .map(|b| world.blocks().default_state(b));
    let mut to_fill = Vec::new();
    let mut locked = Vec::new();
    let mut placed = Vec::new();
    let mut out = PlacedTemplate::default();
    let (mut min, mut max) = ([i32::MAX; 3], [i32::MIN; 3]);
    for info in processed {
        let pos = info.pos;
        if !settings.inside(pos) {
            continue;
        }
        let previous = if waterlogging {
            Some(world.block_state(pos)?)
        } else {
            None
        };
        let data = world.state_data();
        let state = data.rotate(data.mirror(info.state, settings.mirror), settings.rotation);
        if info.nbt.is_some()
            && let Some(barrier) = barrier
        {
            world.set_block(pos, barrier, BARRIER_FLAGS)?;
        }
        if !world.set_block(pos, state, flags)? {
            continue;
        }
        for (axis, v) in [pos.x, pos.y, pos.z].into_iter().enumerate() {
            min[axis] = min[axis].min(v);
            max[axis] = max[axis].max(v);
        }
        placed.push(pos);
        let data = world.state_data();
        if let Some(mut nbt) = info.nbt
            && data.flags(state) & FLAG_HAS_BLOCK_ENTITY != 0
        {
            if data.flags(state) & FLAG_RANDOMIZABLE_CONTAINER != 0 {
                nbt.insert("LootTableSeed".into(), Tag::Long(random.next_long()));
            }
            out.block_entities.push((pos, *nbt));
        }
        if let Some(previous) = previous {
            let data = world.state_data();
            if fluid_source(data, state).is_some() {
                locked.push(pos);
            } else if data.flags(state) & FLAG_LIQUID_CONTAINER != 0 {
                let previous_source = fluid_source(data, previous).is_some();
                place_liquid(world, pos, state, previous)?;
                if !previous_source {
                    to_fill.push(pos);
                }
            }
        }
    }
    fill_liquids(world, to_fill, &locked)?;
    if min[0] <= max[0] && !settings.known_shape {
        update_shape_at_edge(world, &placed, min, max, flags)?;
        // `Block.updateFromNeighbourShapes` and `updateNeighborsAt` per placed block: block
        // behaviour (WP6).
    }
    if !settings.ignore_entities {
        out.entities = place_entities(template, position, settings);
    }
    Ok(Some(out))
}

/// The `toFill` loop: blocks next to a source that is not one of the template's own take it.
fn fill_liquids<W: FeatureWorld>(
    world: &mut W,
    mut to_fill: Vec<BlockPos>,
    locked: &[BlockPos],
) -> Result<(), WorldError> {
    const DIRECTIONS: [Direction; 5] = [
        Direction::Up,
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let mut filled = true;
    while filled && !to_fill.is_empty() {
        filled = false;
        let mut i = 0;
        while i < to_fill.len() {
            let pos = to_fill[i];
            let mut fluid = world.block_state(pos)?;
            let mut source = fluid_source(world.state_data(), fluid).is_some();
            for direction in DIRECTIONS {
                if source {
                    break;
                }
                let [dx, dy, dz] = direction.normal();
                let neighbour = pos.offset(dx, dy, dz);
                let there = world.block_state(neighbour)?;
                if fluid_source(world.state_data(), there).is_some() && !locked.contains(&neighbour)
                {
                    fluid = there;
                    source = true;
                }
            }
            if source {
                let state = world.block_state(pos)?;
                if world.state_data().flags(state) & FLAG_LIQUID_CONTAINER != 0 {
                    place_liquid(world, pos, state, fluid)?;
                    filled = true;
                    to_fill.remove(i);
                    continue;
                }
            }
            i += 1;
        }
    }
    Ok(())
}

/// `updateShapeAtEdge` over the placed blocks' `BitSetDiscreteVoxelShape`: each face of the shape
/// (`forAllFaces`: Z, then Y, then X passes, as `DiscreteVoxelShape.forAllAxisFaces` walks them).
fn update_shape_at_edge<W: FeatureWorld>(
    world: &mut W,
    placed: &[BlockPos],
    min: [i32; 3],
    max: [i32; 3],
    flags: i32,
) -> Result<(), WorldError> {
    let size: [i32; 3] = std::array::from_fn(|i| max[i] - min[i] + 1);
    let mut bits = vec![false; size.iter().map(|&s| s as usize).product()];
    let index = |p: [i32; 3]| ((p[0] * size[1] + p[1]) * size[2] + p[2]) as usize;
    for pos in placed {
        bits[index([pos.x - min[0], pos.y - min[1], pos.z - min[2]])] = true;
    }
    let full = |p: [i32; 3]| (0..3).all(|i| (0..size[i]).contains(&p[i])) && bits[index(p)];
    type Map = fn(i32, i32, i32) -> [i32; 3];
    let [sx, sy, sz] = size;
    let passes: [(Direction, Direction, i32, i32, i32, Map); 3] = [
        (Direction::North, Direction::South, sx, sy, sz, |a, b, c| {
            [a, b, c]
        }),
        (Direction::Down, Direction::Up, sz, sx, sy, |a, b, c| {
            [b, c, a]
        }),
        (Direction::West, Direction::East, sy, sz, sx, |a, b, c| {
            [c, a, b]
        }),
    ];
    for (negative, positive, a_size, b_size, c_size, map) in passes {
        for a in 0..a_size {
            for b in 0..b_size {
                let mut last = false;
                for c in 0..=c_size {
                    let here = c != c_size && full(map(a, b, c));
                    let face = if !last && here {
                        Some((negative, map(a, b, c)))
                    } else if last && !here {
                        Some((positive, map(a, b, c - 1)))
                    } else {
                        None
                    };
                    if let Some((direction, [x, y, z])) = face {
                        let pos = BlockPos::new(min[0] + x, min[1] + y, min[2] + z);
                        world.update_shape_at_face(pos, direction, flags)?;
                    }
                    last = here;
                }
            }
        }
    }
    Ok(())
}

/// `placeEntities`: each entity inside the box, moved, its `Pos`, `UUID` and `block_pos` updated.
pub(crate) fn place_entities(
    template: &StructureTemplate,
    position: BlockPos,
    settings: &PlaceSettings<'_>,
) -> Vec<PlacedEntity> {
    let (mirror, rotation, pivot) = (settings.mirror, settings.rotation, settings.rotation_pivot);
    let mut out = Vec::new();
    for entity in template.entities() {
        let block_pos = transform(entity.block_pos, mirror, rotation, pivot);
        let block_pos = block_pos.offset(position.x, position.y, position.z);
        if !settings.inside(block_pos) {
            continue;
        }
        let relative = transform_vec(entity.pos, mirror, rotation, pivot);
        let pos = [
            relative[0] + f64::from(position.x),
            relative[1] + f64::from(position.y),
            relative[2] + f64::from(position.z),
        ];
        let mut nbt = entity.nbt.clone();
        nbt.insert(
            "Pos".into(),
            Tag::List(pos.iter().map(|v| Tag::Double(v.to_bits())).collect()),
        );
        nbt.remove(&"UUID".into());
        let key = "block_pos".into();
        if nbt.contains_key(&key) {
            nbt.insert(
                key,
                Tag::IntArray(vec![block_pos.x, block_pos.y, block_pos.z]),
            );
        }
        out.push(PlacedEntity {
            pos,
            block_pos,
            nbt,
            rotation,
            mirror,
            finalize: settings.finalize_entities,
        });
    }
    out
}

/// `filterBlocks(position, settings, block, absolute = true)`: the palette's blocks of one kind,
/// moved into the world and rotated (not mirrored, as vanilla), inside the settings' box.
pub fn filter_blocks<R: RandomSource>(
    template: &StructureTemplate,
    data: &BlockStateData,
    blocks: &crate::block::BlockStates,
    position: BlockPos,
    settings: &PlaceSettings<'_>,
    block: BlockId,
    random: &mut R,
) -> Result<Vec<BlockInfo>, PlaceError> {
    let Some(palette) = palette_index(template, settings, position, random)? else {
        return Ok(Vec::new());
    };
    Ok(template.palettes()[palette]
        .blocks()
        .iter()
        .filter(|info| blocks.block(info.state) == block)
        .filter_map(|info| {
            let pos = settings.relative_position(info.pos);
            let pos = pos.offset(position.x, position.y, position.z);
            settings.inside(pos).then(|| BlockInfo {
                pos,
                state: data.rotate(info.state, settings.rotation),
                nbt: info.nbt.clone(),
            })
        })
        .collect())
}

/// `getJigsaws(position, rotation)`: the jigsaws of the palette `position` picks, moved and turned.
pub fn jigsaws(
    template: &StructureTemplate,
    data: &BlockStateData,
    position: BlockPos,
    rotation: Rotation,
) -> Vec<JigsawInfo> {
    let Some(palette) = palette_at(template.palettes().len(), position) else {
        return Vec::new();
    };
    template.palettes()[palette]
        .jigsaws()
        .iter()
        .map(|jigsaw| {
            let pos = transform(jigsaw.pos, Mirror::None, rotation, BlockPos::default());
            JigsawInfo {
                pos: pos.offset(position.x, position.y, position.z),
                state: data.rotate(jigsaw.state, rotation),
                ..jigsaw.clone()
            }
        })
        .collect()
}
