//! `minecraft:tree` and `minecraft:fallen_tree` (`TreeFeature`, `FallenTreeFeature`): trunk,
//! foliage and root placers, decorators, minimum-size clipping and the leaf-distance pass.
//! Copied from Horizon's tree port and rebuilt on the server's block tables; a `HashSet<BlockPos>`
//! iterates in vanilla's order wherever that feeds random draws ([`blocks::PosSet`]).
//! `StructureTemplate.updateShapeAtEdge` is walked face by face and handed to
//! [`FeatureWorld::update_shape_at_face`]: what a block does on a neighbour's change is the block
//! behaviour port's (WP6).
mod blocks;
mod decorator;
mod fallen;
mod foliage;
mod hash_order;
mod trunk;

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use serde_json::value::RawValue;

use super::ConfiguredFeatureId;
use super::simple::FeatureWorld;
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, Object, float, int_range, optional, required};
use crate::worldgen::placement::provider::IntProvider;
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
pub(crate) use blocks::PosSet;
use blocks::{self as tb, TreeBlocks};
use decorator::TreeDecorator;
pub use fallen::FallenTreeFeature;
use foliage::{FoliageAttachment, FoliagePlacer};
use trunk::TrunkPlacer;

pub(crate) type TreeResult<T> = Result<T, ProviderError>;

/// `Registries.FEATURE` key `PALE_MOSS_PATCH`, which `PaleMossDecorator` places by key.
pub(crate) const PALE_MOSS_PATCH: &str = "minecraft:pale_moss_patch";

/// Places a nested configured feature (the pale-moss ground patch).
pub(crate) type NestedPlacer<'a, W, R> =
    dyn FnMut(ConfiguredFeatureId, &mut W, &mut R, [i32; 3]) -> TreeResult<bool> + 'a;

/// The tree classifications, built when the first tree is placed (they need the state data,
/// which the registries are loaded without) and shared by all trees of the registry.
#[derive(Debug)]
pub struct TreeBlocksCell {
    tags: Vec<(String, Vec<i32>)>,
    cell: OnceLock<TreeBlocks>,
}
impl TreeBlocksCell {
    pub fn new(tags: &[(String, Vec<i32>)]) -> Self {
        Self {
            tags: tags.to_vec(),
            cell: OnceLock::new(),
        }
    }
    pub(crate) fn get(&self, blocks: &BlockStates, data: &BlockStateData) -> &TreeBlocks {
        self.cell
            .get_or_init(|| TreeBlocks::build(blocks, data, &self.tags))
    }
}

pub(crate) fn read<W: FeatureWorld>(world: &W, position: [i32; 3]) -> TreeResult<BlockState> {
    Ok(world.block_state(BlockPos::new(position[0], position[1], position[2]))?)
}
pub(crate) fn write<W: FeatureWorld>(
    world: &mut W,
    position: [i32; 3],
    state: BlockState,
    flags: i32,
) -> TreeResult<bool> {
    Ok(world.set_block(
        BlockPos::new(position[0], position[1], position[2]),
        state,
        flags,
    )?)
}
pub(crate) fn sample_state<W: FeatureWorld>(
    provider: &BlockStateProvider,
    world: &W,
    random: &mut impl RandomSource,
    position: [i32; 3],
) -> TreeResult<BlockState> {
    provider.state(
        world,
        random,
        BlockPos::new(position[0], position[1], position[2]),
    )
}
pub(crate) fn sample_optional<W: FeatureWorld>(
    provider: &BlockStateProvider,
    world: &W,
    random: &mut impl RandomSource,
    position: [i32; 3],
) -> TreeResult<Option<BlockState>> {
    provider.optional_state(
        world,
        random,
        BlockPos::new(position[0], position[1], position[2]),
    )
}

/// Decoding context shared by every tree component.
pub(crate) struct TreeDecoder<'a, 'r> {
    pub ctx: &'a ProviderContext<'r>,
}
impl TreeDecoder<'_, '_> {
    pub fn provider(&mut self, raw: &RawValue) -> Result<BlockStateProvider, DecodeError> {
        BlockStateProvider::decode(raw, self.ctx)
    }
    pub fn int_provider(
        &mut self,
        raw: &RawValue,
        low: i32,
        high: i32,
    ) -> Result<IntProvider, DecodeError> {
        IntProvider::decode_within(raw, low, high)
    }
    /// A block holder set as the set of all its blocks' states.
    pub fn block_set(&self, raw: &RawValue) -> Result<Arc<BTreeSet<BlockState>>, DecodeError> {
        let names = self.ctx.names;
        let set = names.block_set(raw)?;
        let mut states = BTreeSet::new();
        for block in 0..names.blocks.block_count() {
            if set.contains(block) {
                states.extend(names.blocks.states(crate::block::BlockId(block)));
            }
        }
        Ok(Arc::new(states))
    }
}
pub(crate) fn unit_float(raw: &RawValue, name: &'static str) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(DecodeError::Field(name));
    }
    Ok(value)
}
pub(crate) fn kind(object: &Object) -> Result<String, DecodeError> {
    crate::worldgen::codec::identifier(&crate::worldgen::codec::string(required(object, "type")?)?)
}
fn optional_int(
    object: &Object,
    name: &'static str,
    default: i32,
    low: i32,
    high: i32,
) -> Result<i32, DecodeError> {
    optional(object, name)
        .map(|raw| int_range(raw, name, low, high))
        .transpose()
        .map(|v| v.unwrap_or(default))
}

#[derive(Clone, Debug)]
enum FeatureSize {
    TwoLayers {
        limit: i32,
        lower: i32,
        upper: i32,
    },
    ThreeLayers {
        limit: i32,
        upper_limit: i32,
        lower: i32,
        middle: i32,
        upper: i32,
    },
}
impl FeatureSize {
    fn decode(raw: &RawValue) -> Result<(Self, Option<i32>), DecodeError> {
        let object: Object = serde_json::from_str(raw.get())?;
        let clipped = optional(&object, "min_clipped_height")
            .map(|raw| int_range(raw, "min_clipped_height", 0, 80))
            .transpose()?;
        let size = match kind(&object)?.as_str() {
            "minecraft:two_layers_feature_size" => Self::TwoLayers {
                limit: optional_int(&object, "limit", 1, 0, 81)?,
                lower: optional_int(&object, "lower_size", 0, 0, 16)?,
                upper: optional_int(&object, "upper_size", 1, 0, 16)?,
            },
            "minecraft:three_layers_feature_size" => Self::ThreeLayers {
                limit: optional_int(&object, "limit", 1, 0, 80)?,
                upper_limit: optional_int(&object, "upper_limit", 1, 0, 80)?,
                lower: optional_int(&object, "lower_size", 0, 0, 16)?,
                middle: optional_int(&object, "middle_size", 1, 0, 16)?,
                upper: optional_int(&object, "upper_size", 1, 0, 16)?,
            },
            _ => return Err(DecodeError::Field("feature size type")),
        };
        Ok((size, clipped))
    }
    fn size_at_height(&self, tree_height: i32, yo: i32) -> i32 {
        match *self {
            Self::TwoLayers {
                limit,
                lower,
                upper,
            } => {
                if yo < limit {
                    lower
                } else {
                    upper
                }
            }
            Self::ThreeLayers {
                limit,
                upper_limit,
                lower,
                middle,
                upper,
            } => {
                if yo < limit {
                    lower
                } else if yo >= tree_height.wrapping_sub(upper_limit) {
                    upper
                } else {
                    middle
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct MangroveRoots {
    can_grow_through: Arc<BTreeSet<BlockState>>,
    muddy_roots_in: Arc<BTreeSet<BlockState>>,
    muddy_roots_provider: BlockStateProvider,
    max_root_width: i32,
    max_root_length: i32,
    random_skew_chance: f32,
}
#[derive(Clone, Debug)]
struct RootPlacer {
    trunk_offset_y: IntProvider,
    root_provider: BlockStateProvider,
    above: Option<(BlockStateProvider, f32)>,
    mangrove: MangroveRoots,
}
impl RootPlacer {
    fn decode(raw: &RawValue, decoder: &mut TreeDecoder<'_, '_>) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(raw.get())?;
        if kind(&object)? != "minecraft:mangrove_root_placer" {
            return Err(DecodeError::Field("root placer type"));
        }
        let trunk_offset_y = IntProvider::decode(required(&object, "trunk_offset_y")?)?;
        let root_provider = decoder.provider(required(&object, "root_provider")?)?;
        let above = optional(&object, "above_root_placement")
            .map(|raw| -> Result<_, DecodeError> {
                let above: Object = serde_json::from_str(raw.get())?;
                Ok((
                    decoder.provider(required(&above, "above_root_provider")?)?,
                    unit_float(
                        required(&above, "above_root_placement_chance")?,
                        "above_root_placement_chance",
                    )?,
                ))
            })
            .transpose()?;
        let placement: Object =
            serde_json::from_str(required(&object, "mangrove_root_placement")?.get())?;
        let mangrove = MangroveRoots {
            can_grow_through: decoder.block_set(required(&placement, "can_grow_through")?)?,
            muddy_roots_in: decoder.block_set(required(&placement, "muddy_roots_in")?)?,
            muddy_roots_provider: decoder
                .provider(required(&placement, "muddy_roots_provider")?)?,
            max_root_width: int_range(
                required(&placement, "max_root_width")?,
                "max_root_width",
                1,
                12,
            )?,
            max_root_length: int_range(
                required(&placement, "max_root_length")?,
                "max_root_length",
                1,
                64,
            )?,
            random_skew_chance: unit_float(
                required(&placement, "random_skew_chance")?,
                "random_skew_chance",
            )?,
        };
        Ok(Self {
            trunk_offset_y,
            root_provider,
            above,
            mangrove,
        })
    }
    fn can_place_root<W: FeatureWorld, R>(
        &self,
        run: &mut Run<'_, W, R>,
        position: [i32; 3],
    ) -> TreeResult<bool> {
        let state = read(run.world, position)?;
        Ok(run.blocks.valid_tree_pos(state) || self.mangrove.can_grow_through.contains(&state))
    }
    fn waterlogged<W: FeatureWorld>(
        blocks: &TreeBlocks,
        world: &W,
        position: [i32; 3],
        state: BlockState,
    ) -> TreeResult<BlockState> {
        if blocks.has(state, tb::WATERLOGGABLE) {
            let here = read(world, position)?;
            Ok(blocks.waterlogged(world.blocks(), state, blocks.has(here, tb::WATER_FLUID)))
        } else {
            Ok(state)
        }
    }
    fn place_root<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        position: [i32; 3],
    ) -> TreeResult<()> {
        let here = read(run.world, position)?;
        if self.mangrove.muddy_roots_in.contains(&here) {
            let state = sample_state(
                &self.mangrove.muddy_roots_provider,
                run.world,
                run.random,
                position,
            )?;
            let state = Self::waterlogged(run.blocks, run.world, position, state)?;
            return run.set(Layer::Root, position, state);
        }
        if !self.can_place_root(run, position)? {
            return Ok(());
        }
        let state = sample_state(&self.root_provider, run.world, run.random, position)?;
        let state = Self::waterlogged(run.blocks, run.world, position, state)?;
        run.set(Layer::Root, position, state)?;
        if let Some((provider, chance)) = &self.above {
            let above = tb::offset(position, 0, 1, 0);
            if run.random.next_float() < *chance && run.blocks.is_air(read(run.world, above)?) {
                let state = sample_state(provider, run.world, run.random, above)?;
                let state = Self::waterlogged(run.blocks, run.world, above, state)?;
                run.set(Layer::Root, above, state)?;
            }
        }
        Ok(())
    }
    fn place_roots<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        origin: [i32; 3],
        trunk_origin: [i32; 3],
    ) -> TreeResult<bool> {
        let mut column = origin;
        while column[1] < trunk_origin[1] {
            if !self.can_place_root(run, column)? {
                return Ok(false);
            }
            column[1] = column[1].wrapping_add(1);
        }
        let mut roots = vec![tb::offset(trunk_origin, 0, -1, 0)];
        for direction in tb::HORIZONTAL {
            let start = tb::relative(trunk_origin, direction, 1);
            let mut positions = Vec::new();
            if !self.simulate(run, start, direction, trunk_origin, &mut positions, 0)? {
                return Ok(false);
            }
            roots.extend(positions);
            roots.push(start);
        }
        for root in roots {
            self.place_root(run, root)?;
        }
        Ok(true)
    }
    #[allow(clippy::too_many_arguments)]
    fn simulate<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        position: [i32; 3],
        direction: Direction,
        origin: [i32; 3],
        positions: &mut Vec<[i32; 3]>,
        layer: i32,
    ) -> TreeResult<bool> {
        let max = self.mangrove.max_root_length;
        if layer == max || positions.len() as i64 > i64::from(max) {
            return Ok(false);
        }
        for next in self.potential(run.random, position, direction, origin)? {
            if self.can_place_root(run, next)? {
                positions.push(next);
                if !self.simulate(run, next, direction, origin, positions, layer + 1)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    #[allow(clippy::if_same_then_else)] // Native branch order controls random draws.
    fn potential(
        &self,
        random: &mut impl RandomSource,
        position: [i32; 3],
        direction: Direction,
        origin: [i32; 3],
    ) -> TreeResult<Vec<[i32; 3]>> {
        let below = tb::offset(position, 0, -1, 0);
        let next_to = tb::relative(position, direction, 1);
        let width = tb::dist_manhattan(position, origin);
        let max = self.mangrove.max_root_width;
        let skew = self.mangrove.random_skew_chance;
        Ok(if width > max - 3 && width <= max {
            if random.next_float() < skew {
                vec![below, tb::offset(next_to, 0, -1, 0)]
            } else {
                vec![below]
            }
        } else if width > max {
            vec![below]
        } else if random.next_float() < skew {
            vec![below]
        } else if random.next_boolean() {
            vec![next_to]
        } else {
            vec![below]
        })
    }
}

#[derive(Clone, Debug)]
pub struct TreeFeature {
    trunk_provider: BlockStateProvider,
    trunk_placer: TrunkPlacer,
    foliage_provider: BlockStateProvider,
    foliage_placer: FoliagePlacer,
    root_placer: Option<RootPlacer>,
    minimum_size: FeatureSize,
    min_clipped_height: Option<i32>,
    decorators: Vec<TreeDecorator>,
    ignore_vines: bool,
    below_trunk_provider: BlockStateProvider,
    cell: Arc<TreeBlocksCell>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Root,
    Trunk,
    Foliage,
    Decoration,
}
#[derive(Default)]
pub(crate) struct Sets {
    roots: PosSet,
    trunks: PosSet,
    foliage: PosSet,
    decorations: PosSet,
}
pub(crate) struct Run<'a, W, R> {
    pub world: &'a mut W,
    pub random: &'a mut R,
    pub blocks: &'a TreeBlocks,
    sets: Sets,
    /// Fallen trees write decorations without recording them.
    track: bool,
}
impl<W: FeatureWorld, R> Run<'_, W, R> {
    pub(crate) fn set(
        &mut self,
        layer: Layer,
        position: [i32; 3],
        state: BlockState,
    ) -> TreeResult<()> {
        if self.track {
            match layer {
                Layer::Root => self.sets.roots.insert(position),
                Layer::Trunk => self.sets.trunks.insert(position),
                Layer::Foliage => self.sets.foliage.insert(position),
                Layer::Decoration => self.sets.decorations.insert(position),
            };
        }
        write(self.world, position, state, 19)?;
        Ok(())
    }
    pub(crate) fn foliage_contains(&self, position: [i32; 3]) -> bool {
        self.sets.foliage.contains(position)
    }
}

impl TreeFeature {
    pub(crate) fn decode(
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
        cell: Arc<TreeBlocksCell>,
    ) -> Result<Self, DecodeError> {
        let object: Object = crate::worldgen::codec::object(raw)?;
        if kind(&object)? != "minecraft:tree" {
            return Err(DecodeError::Field("tree feature type"));
        }
        let mut decoder = TreeDecoder { ctx };
        let trunk_provider = decoder.provider(required(&object, "trunk_provider")?)?;
        let trunk_placer = TrunkPlacer::decode(required(&object, "trunk_placer")?, &mut decoder)?;
        let foliage_provider = decoder.provider(required(&object, "foliage_provider")?)?;
        let foliage_placer =
            FoliagePlacer::decode(required(&object, "foliage_placer")?, &mut decoder)?;
        let root_placer = optional(&object, "root_placer")
            .map(|raw| RootPlacer::decode(raw, &mut decoder))
            .transpose()?;
        let (minimum_size, min_clipped_height) =
            FeatureSize::decode(required(&object, "minimum_size")?)?;
        let decorators =
            TreeDecorator::decode_list(required(&object, "decorators")?, &mut decoder)?;
        let ignore_vines = optional(&object, "ignore_vines")
            .map(|raw| serde_json::from_str::<bool>(raw.get()))
            .transpose()?
            .unwrap_or(false);
        let below_trunk_provider = decoder.provider(required(&object, "below_trunk_provider")?)?;
        Ok(Self {
            trunk_provider,
            trunk_placer,
            foliage_provider,
            foliage_placer,
            root_placer,
            minimum_size,
            min_clipped_height,
            decorators,
            ignore_vines,
            below_trunk_provider,
            cell,
        })
    }

    /// Binds the `pale_moss` decorator to the `PALE_MOSS_PATCH` feature.
    pub(crate) fn set_pale_moss_patch(&mut self, patch: ConfiguredFeatureId) {
        for decorator in &mut self.decorators {
            decorator.set_patch(patch);
        }
    }

    /// Whether a decorator places the pale moss patch (a feature of its own).
    pub fn places_pale_moss(&self) -> bool {
        self.decorators.iter().any(TreeDecorator::is_pale_moss)
    }

    pub(crate) fn place<W: FeatureWorld, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
        nested: &mut NestedPlacer<'_, W, R>,
    ) -> TreeResult<bool> {
        let blocks = self.cell.get(world.blocks(), world.state_data());
        let mut run = Run {
            world,
            random,
            blocks,
            sets: Sets::default(),
            track: true,
        };
        let result = self.do_place(&mut run, origin)?;
        if !result || (run.sets.trunks.is_empty() && run.sets.foliage.is_empty()) {
            return Ok(false);
        }
        if !self.decorators.is_empty() {
            let lists = decorator::Lists::new(
                run.sets.trunks.to_vec(),
                run.sets.foliage.to_vec(),
                run.sets.roots.to_vec(),
            );
            for decorator in &self.decorators {
                decorator.place(&mut run, &lists, nested)?;
            }
        }
        let mut bounds: Option<([i32; 3], [i32; 3])> = None;
        for set in [
            &run.sets.roots,
            &run.sets.trunks,
            &run.sets.foliage,
            &run.sets.decorations,
        ] {
            for p in set.to_vec() {
                bounds = Some(match bounds {
                    None => (p, p),
                    Some((lo, hi)) => (
                        std::array::from_fn(|i| lo[i].min(p[i])),
                        std::array::from_fn(|i| hi[i].max(p[i])),
                    ),
                });
            }
        }
        let Some((low, high)) = bounds else {
            return Ok(false);
        };
        let shape = update_leaves(&mut run, low, high)?;
        update_shape_at_edge(run.world, &shape, low, 3)?;
        Ok(true)
    }

    fn do_place<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        origin: [i32; 3],
    ) -> TreeResult<bool> {
        let tree_height = self.trunk_placer.tree_height(run.random)?;
        let foliage_height = self
            .foliage_placer
            .foliage_height(run.random, tree_height)?;
        let trunk_height = tree_height.wrapping_sub(foliage_height);
        let leaf_radius = self
            .foliage_placer
            .foliage_radius(run.random, trunk_height)?;
        let trunk_origin = match &self.root_placer {
            Some(root) => tb::offset(origin, 0, root.trunk_offset_y.sample(run.random)?, 0),
            None => origin,
        };
        let min_y = origin[1].min(trunk_origin[1]);
        let max_y = origin[1]
            .max(trunk_origin[1])
            .wrapping_add(tree_height)
            .wrapping_add(1);
        let level = run.world.level_generation();
        let world_min = level.min_y;
        let world_max = level.min_y.wrapping_add(level.height).wrapping_sub(1);
        if min_y < world_min.wrapping_add(1) || max_y > world_max.wrapping_add(1) {
            return Ok(false);
        }
        let clipped = self.max_free_tree_height(run, tree_height, trunk_origin)?;
        if clipped < tree_height && self.min_clipped_height.is_none_or(|min| clipped < min) {
            return Ok(false);
        }
        if let Some(root) = &self.root_placer
            && !root.place_roots(run, origin, trunk_origin)?
        {
            return Ok(false);
        }
        let attachments = self
            .trunk_placer
            .place_trunk(run, self, clipped, trunk_origin)?;
        for attachment in attachments {
            self.foliage_placer.create_foliage(
                run,
                self,
                clipped,
                attachment,
                foliage_height,
                leaf_radius,
            )?;
        }
        Ok(true)
    }

    fn max_free_tree_height<W: FeatureWorld, R>(
        &self,
        run: &mut Run<'_, W, R>,
        max_tree_height: i32,
        tree_pos: [i32; 3],
    ) -> TreeResult<i32> {
        let mut y = 0;
        while y <= max_tree_height.wrapping_add(1) {
            let r = self.minimum_size.size_at_height(max_tree_height, y);
            for x in -r..=r {
                for z in -r..=r {
                    let p = tb::offset(tree_pos, x, y, z);
                    let state = read(run.world, p)?;
                    if !self.trunk_placer.is_free(run.blocks, state)
                        || (!self.ignore_vines && run.blocks.has(state, tb::VINE))
                    {
                        return Ok(y - 2);
                    }
                }
            }
            y += 1;
        }
        Ok(max_tree_height)
    }

    /// FoliagePlacer.tryPlaceLeaf.
    pub(crate) fn try_place_leaf<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        position: [i32; 3],
    ) -> TreeResult<bool> {
        let here = read(run.world, position)?;
        if run.blocks.has(here, tb::PERSISTENT) || !run.blocks.valid_tree_pos(here) {
            return Ok(false);
        }
        let mut state = sample_state(&self.foliage_provider, run.world, run.random, position)?;
        if run.blocks.has(state, tb::WATERLOGGABLE) {
            let here = read(run.world, position)?;
            state = run.blocks.waterlogged(
                run.world.blocks(),
                state,
                run.blocks.has(here, tb::WATER_SOURCE),
            );
        }
        run.set(Layer::Foliage, position, state)?;
        Ok(true)
    }
}

/// Voxel occupancy over the tree's bounding box.
pub(crate) struct Shape {
    size: [i32; 3],
    bits: Vec<bool>,
}
impl Shape {
    fn new(size: [i32; 3]) -> Self {
        let len = size.iter().map(|v| *v as usize).product();
        Self {
            size,
            bits: vec![false; len],
        }
    }
    fn index(&self, [x, y, z]: [i32; 3]) -> usize {
        ((x * self.size[1] + y) * self.size[2] + z) as usize
    }
    fn fill(&mut self, p: [i32; 3]) {
        let i = self.index(p);
        self.bits[i] = true;
    }
    fn full(&self, p: [i32; 3]) -> bool {
        p.iter()
            .zip(self.size)
            .all(|(v, size)| (0..size).contains(v))
            && self.bits[self.index(p)]
    }
}

const MAX_TREE_SPAN: i64 = 512;
fn inside(low: [i32; 3], high: [i32; 3], p: [i32; 3]) -> bool {
    (0..3).all(|i| p[i] >= low[i] && p[i] <= high[i])
}

/// TreeFeature.updateLeaves: native bucketed breadth-first distance pass.
fn update_leaves<W: FeatureWorld, R>(
    run: &mut Run<'_, W, R>,
    low: [i32; 3],
    high: [i32; 3],
) -> TreeResult<Shape> {
    let span: [i64; 3] = std::array::from_fn(|i| i64::from(high[i]) - i64::from(low[i]) + 1);
    if span.iter().any(|s| *s > MAX_TREE_SPAN) {
        return Err(ProviderError::Invalid("tree too large"));
    }
    let mut shape = Shape::new(span.map(|v| v as i32));
    let local = |p: [i32; 3]| -> [i32; 3] { std::array::from_fn(|i| p[i].wrapping_sub(low[i])) };
    let mut fixed = run.sets.decorations.to_vec();
    fixed.extend(run.sets.roots.to_vec());
    for p in fixed {
        if inside(low, high, p) {
            shape.fill(local(p));
        }
    }
    let mut to_check: [PosSet; 7] = Default::default();
    for p in run.sets.trunks.to_vec() {
        to_check[0].insert(p);
    }
    let mut smallest = 0usize;
    loop {
        while smallest >= 7 || !to_check[smallest].is_empty() {
            if smallest >= 7 {
                return Ok(shape);
            }
            let pos = to_check[smallest]
                .pop_first()
                .ok_or(ProviderError::Invalid("tree state"))?;
            if !inside(low, high, pos) {
                continue;
            }
            if smallest != 0 {
                let state = read(run.world, pos)?;
                let updated = run
                    .world
                    .blocks()
                    .with_property(state, "distance", &smallest.to_string())
                    .ok_or(ProviderError::Invalid("tree state"))?;
                write(run.world, pos, updated, 19)?;
            }
            shape.fill(local(pos));
            for direction in Direction::ALL {
                let neighbor = tb::relative(pos, direction, 1);
                if !inside(low, high, neighbor) || shape.full(local(neighbor)) {
                    continue;
                }
                let state = read(run.world, neighbor)?;
                if let Some(distance) = run.blocks.leaf_distance(state) {
                    let next = distance.min(smallest as i32 + 1);
                    if next < 7 {
                        to_check[next as usize].insert(neighbor);
                        smallest = smallest.min(next as usize);
                    }
                }
            }
        }
        smallest += 1;
    }
}

/// StructureTemplate.updateShapeAtEdge face traversal (DiscreteVoxelShape.forAllFaces).
pub(crate) fn update_shape_at_edge<W: FeatureWorld>(
    world: &mut W,
    shape: &Shape,
    start: [i32; 3],
    flags: i32,
) -> TreeResult<()> {
    let [sx, sy, sz] = shape.size;
    // (c-axis, a size, b size, mapping (a, b, c) -> (x, y, z))
    type Map = fn(i32, i32, i32) -> [i32; 3];
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
                    let full = c != c_size && shape.full(map(a, b, c));
                    let face = if !last && full {
                        Some((negative, map(a, b, c)))
                    } else if last && !full {
                        Some((positive, map(a, b, c - 1)))
                    } else {
                        None
                    };
                    if let Some((direction, local)) = face {
                        let p: [i32; 3] = std::array::from_fn(|i| start[i].wrapping_add(local[i]));
                        world.update_shape_at_face(
                            BlockPos::new(p[0], p[1], p[2]),
                            direction,
                            flags,
                        )?;
                    }
                    last = full;
                }
            }
        }
    }
    Ok(())
}
