//! Structure processors (`StructureProcessor` and its 11 vanilla types), their rules (`ProcessorRule`
//! with `RuleTest`, `PosRuleTest` and `RuleBlockEntityModifier`) and the `worldgen/processor_list`
//! registry, decoded once with the other worldgen registries.
//!
//! A processor is a closed enum; the blocks the built-in types name (jigsaw, lava, the stone bricks
//! `block_age` turns mossy...) are looked up once into [`KnownBlocks`], which the registry owns and
//! processing reads. Vanilla's processor lists cannot refer to one another (`capped`'s delegate is
//! an inline processor), so Horizon's thread-local nesting guard has no counterpart here: the JSON
//! parser's own nesting limit bounds a decode.

use std::collections::HashMap;

use serde_json::value::RawValue;

use super::BiomeSet;
use super::template::BlockInfo;
use crate::block::flags::FLAG_SHAPE_FULL_BLOCK;
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::BlockPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::registry::{DataPack, for_each_element, load_tags, registry_dir};
use crate::util::random::{
    InvalidBound, LegacyRandom, PositionalRandom, RandomSource, position_seed,
};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{
    DecodeError, Object, float, identifier, integer, object, optional, required, string,
};
use crate::worldgen::data::WorldgenError;
use crate::worldgen::feature::rule_test::RuleTest;
use crate::worldgen::placement::{DecodeContext, IntProvider, PlacementWorld, WorldError};

/// `PosRuleTest`.
#[derive(Clone, Debug, PartialEq)]
pub enum PosRuleTest {
    AlwaysTrue,
    /// `LinearPosTest`: the Manhattan distance to the reference.
    Linear {
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
    },
    /// `AxisAlignedLinearPosTest`: the distance along one axis (0 x, 1 y, 2 z).
    AxisAlignedLinear {
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
        axis: usize,
    },
}

/// `Mth.clampedLerp(Mth.inverseLerp(dist, min, max), minChance, maxChance)` in float arithmetic.
fn chance(dist: i32, min_dist: i32, max_dist: i32, min_chance: f32, max_chance: f32) -> f32 {
    let factor = (dist as f32 - min_dist as f32) / (max_dist as f32 - min_dist as f32);
    if factor < 0.0 {
        min_chance
    } else if factor > 1.0 {
        max_chance
    } else {
        min_chance + factor * (max_chance - min_chance)
    }
}

impl PosRuleTest {
    fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let kind = identifier(&string(required(&fields, "predicate_type")?)?)?;
        let float_or = |name, default| optional(&fields, name).map_or(Ok(default), float);
        let int_or = |name, default| optional(&fields, name).map_or(Ok(default), integer);
        let range = || -> Result<(f32, f32, i32, i32), DecodeError> {
            let (min_dist, max_dist) = (int_or("min_dist", 0)?, int_or("max_dist", 0)?);
            if min_dist >= max_dist {
                // The constructor throws `Invalid range` while the codec decodes.
                return Err(DecodeError::Field("position rule distance range"));
            }
            Ok((
                float_or("min_chance", 0.0)?,
                float_or("max_chance", 0.0)?,
                min_dist,
                max_dist,
            ))
        };
        Ok(match kind.as_str() {
            "minecraft:always_true" => Self::AlwaysTrue,
            "minecraft:linear_pos" => {
                let (min_chance, max_chance, min_dist, max_dist) = range()?;
                Self::Linear {
                    min_chance,
                    max_chance,
                    min_dist,
                    max_dist,
                }
            }
            "minecraft:axis_aligned_linear_pos" => {
                let (min_chance, max_chance, min_dist, max_dist) = range()?;
                let axis = match optional(&fields, "axis")
                    .map(string)
                    .transpose()?
                    .as_deref()
                {
                    None | Some("y") => 1,
                    Some("x") => 0,
                    Some("z") => 2,
                    Some(_) => return Err(DecodeError::Field("axis")),
                };
                Self::AxisAlignedLinear {
                    min_chance,
                    max_chance,
                    min_dist,
                    max_dist,
                    axis,
                }
            }
            _ => return Err(DecodeError::Field("position rule test type")),
        })
    }

    /// `test(inTemplatePos, worldPos, worldReference, random)`: one `nextFloat` unless always true.
    pub fn test(
        &self,
        world: BlockPos,
        reference: BlockPos,
        random: &mut impl RandomSource,
    ) -> bool {
        match *self {
            Self::AlwaysTrue => true,
            Self::Linear {
                min_chance,
                max_chance,
                min_dist,
                max_dist,
            } => {
                // `Vec3i.distManhattan`: each `abs` of an int difference as a float, summed.
                let d = |a: i32, b: i32| b.wrapping_sub(a).wrapping_abs() as f32;
                let dist = (d(world.x, reference.x)
                    + d(world.y, reference.y)
                    + d(world.z, reference.z)) as i32;
                random.next_float() <= chance(dist, min_dist, max_dist, min_chance, max_chance)
            }
            Self::AxisAlignedLinear {
                min_chance,
                max_chance,
                min_dist,
                max_dist,
                axis,
            } => {
                let w = [world.x, world.y, world.z];
                let r = [reference.x, reference.y, reference.z];
                let dist = w[axis].wrapping_sub(r[axis]).wrapping_abs() as f32 as i32;
                random.next_float() <= chance(dist, min_dist, max_dist, min_chance, max_chance)
            }
        }
    }
}

/// `RuleBlockEntityModifier`.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockEntityModifier {
    Passthrough,
    Clear,
    AppendStatic(Compound),
    /// `AppendLoot`: the loot table id, with a `LootTableSeed` drawn from the rule's random.
    AppendLoot(String),
}

impl BlockEntityModifier {
    fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        Ok(
            match identifier(&string(required(&fields, "type")?)?)?.as_str() {
                "minecraft:passthrough" => Self::Passthrough,
                "minecraft:clear" => Self::Clear,
                "minecraft:append_static" => {
                    let data: serde_json::Value =
                        serde_json::from_str(required(&fields, "data")?.get())?;
                    match json_to_nbt(&data) {
                        Tag::Compound(tag) => Self::AppendStatic(tag),
                        _ => return Err(DecodeError::Field("data")),
                    }
                }
                "minecraft:append_loot" => {
                    Self::AppendLoot(identifier(&string(required(&fields, "loot_table")?)?)?)
                }
                _ => return Err(DecodeError::Field("block entity modifier type")),
            },
        )
    }

    /// `apply(random, existingTag)`.
    fn apply(
        &self,
        random: &mut impl RandomSource,
        existing: Option<Box<Compound>>,
    ) -> Option<Box<Compound>> {
        match self {
            Self::Passthrough => existing,
            Self::Clear => Some(Box::default()),
            Self::AppendStatic(data) => Some(match existing {
                None => Box::new(data.clone()),
                Some(mut tag) => {
                    merge(&mut tag, data);
                    tag
                }
            }),
            Self::AppendLoot(table) => {
                let mut tag = existing.unwrap_or_default();
                tag.insert("LootTable".into(), Tag::String(table.as_str().into()));
                tag.insert("LootTableSeed".into(), Tag::Long(random.next_long()));
                Some(tag)
            }
        }
    }
}

/// `CompoundTag.merge`: compounds merge recursively, anything else is replaced.
fn merge(into: &mut Compound, other: &Compound) {
    for (key, value) in other {
        match (into.get_mut(key), value) {
            (Some(Tag::Compound(mine)), Tag::Compound(theirs)) => merge(mine, theirs),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

/// `CompoundTag.CODEC` over JSON (`JsonOps.convertTo(NbtOps)`): integers take the narrowest of
/// byte/short/int/long, other numbers float when exact else double, booleans are bytes, and lists
/// of bytes, ints or longs become arrays as `NbtOps` collects them.
pub(crate) fn json_to_nbt(value: &serde_json::Value) -> Tag {
    use serde_json::Value;
    match value {
        Value::Null => Tag::End,
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => {
            let integral = n.as_i64().or_else(|| {
                let f = n.as_f64()?;
                (f.fract() == 0.0 && f.abs() < 9.2e18).then_some(f as i64)
            });
            match integral {
                Some(l) if l as i8 as i64 == l => Tag::Byte(l as i8),
                Some(l) if l as i16 as i64 == l => Tag::Short(l as i16),
                Some(l) if l as i32 as i64 == l => Tag::Int(l as i32),
                Some(l) => Tag::Long(l),
                None => {
                    let d = n.as_f64().unwrap_or(0.0);
                    if f64::from(d as f32) == d {
                        Tag::Float((d as f32).to_bits())
                    } else {
                        Tag::Double(d.to_bits())
                    }
                }
            }
        }
        Value::String(s) => Tag::String(s.as_str().into()),
        Value::Array(items) => {
            let tags: Vec<Tag> = items.iter().map(json_to_nbt).collect();
            let all = |f: fn(&Tag) -> bool| !tags.is_empty() && tags.iter().all(f);
            if all(|t| matches!(t, Tag::Byte(_))) {
                Tag::ByteArray(
                    tags.iter()
                        .map(|t| if let Tag::Byte(b) = t { *b as u8 } else { 0 })
                        .collect(),
                )
            } else if all(|t| matches!(t, Tag::Int(_))) {
                Tag::IntArray(tags.iter().filter_map(Tag::int_value).collect())
            } else if all(|t| matches!(t, Tag::Long(_))) {
                Tag::LongArray(
                    tags.iter()
                        .map(|t| if let Tag::Long(l) = t { *l } else { 0 })
                        .collect(),
                )
            } else {
                Tag::List(tags)
            }
        }
        Value::Object(map) => Tag::Compound(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (JavaString::from(k.as_str()), json_to_nbt(v)))
                .collect(),
        ),
    }
}

/// `ProcessorRule`.
#[derive(Clone, Debug)]
pub struct ProcessorRule {
    pub input: RuleTest,
    pub location: RuleTest,
    pub position: PosRuleTest,
    pub output: BlockState,
    pub modifier: BlockEntityModifier,
}

impl ProcessorRule {
    fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        // `lenientOptionalFieldOf`: a field that does not decode falls back to the default.
        let position = optional(&fields, "position_predicate")
            .and_then(|raw| PosRuleTest::decode(raw).ok())
            .unwrap_or(PosRuleTest::AlwaysTrue);
        let modifier = optional(&fields, "block_entity_modifier")
            .and_then(|raw| BlockEntityModifier::decode(raw).ok())
            .unwrap_or(BlockEntityModifier::Passthrough);
        Ok(Self {
            input: RuleTest::decode(required(&fields, "input_predicate")?, names)?,
            location: RuleTest::decode(required(&fields, "location_predicate")?, names)?,
            position,
            output: names.block_state(required(&fields, "output_state")?)?,
            modifier,
        })
    }
}

/// `StructureProcessor`: the vanilla types (`StructureProcessorTypes`).
#[derive(Clone, Debug)]
pub enum Processor {
    /// `RuleProcessor`: the first rule that matches replaces the block.
    Rule(Vec<ProcessorRule>),
    /// `BlockRotProcessor`: drops (rottable) blocks with probability `1 - integrity`.
    BlockRot {
        rottable: Option<BiomeSet>,
        integrity: f32,
    },
    /// `BlockIgnoreProcessor`: drops these blocks.
    BlockIgnore(Vec<BlockId>),
    /// `GravityProcessor`: moves each block to the surface plus `offset` plus its template y.
    Gravity { heightmap: Heightmap, offset: i32 },
    /// `ProtectedBlockProcessor`: keeps the world's blocks of this set.
    ProtectedBlocks(BiomeSet),
    /// `CappedProcessor`: runs `delegate` on at most `limit` blocks of the piece, after the rest.
    Capped {
        delegate: Box<Processor>,
        limit: IntProvider,
    },
    /// `LavaSubmergedBlockProcessor`.
    LavaSubmergedBlock,
    /// `JigsawReplacementProcessor`: a jigsaw becomes its `final_state`.
    JigsawReplacement,
    /// `BlockAgeProcessor`.
    BlockAge { mossiness: f32 },
    /// `BlackstoneReplaceProcessor`.
    BlackstoneReplace,
    /// `NopProcessor`.
    Nop,
}

/// `StructureProcessorList`.
#[derive(Clone, Debug, Default)]
pub struct ProcessorList(pub Vec<Processor>);

/// A processor list as structures and pools name it (`StructureProcessorType.LIST_CODEC`).
#[derive(Clone, Debug)]
pub enum ProcessorListHolder {
    Registry(ProcessorListId),
    Inline(ProcessorList),
}

/// A `worldgen/processor_list` element: its index in registration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessorListId(pub u32);

/// Blocks the built-in processors name, looked up once. A block the table lacks is `None` and its
/// processor leaves blocks alone.
#[derive(Clone, Debug)]
pub struct KnownBlocks {
    pub air: BlockState,
    air_block: Option<BlockId>,
    jigsaw: Option<BlockId>,
    structure_void: Option<BlockId>,
    structure_block: Option<BlockId>,
    lava: Option<BlockId>,
    /// `BlockAgeProcessor`'s blocks, by name (see [`AGE_BLOCKS`]).
    age: [Option<BlockId>; AGE_BLOCKS.len()],
    stairs: BiomeSet,
    slabs: BiomeSet,
    walls: BiomeSet,
    /// `BlackstoneReplaceProcessor.replacements`.
    blackstone: HashMap<BlockId, BlockId>,
}

const AGE_BLOCKS: [&str; 13] = [
    "stone_bricks",
    "stone",
    "chiseled_stone_bricks",
    "obsidian",
    "crying_obsidian",
    "cracked_stone_bricks",
    "mossy_stone_bricks",
    "stone_brick_stairs",
    "mossy_stone_brick_stairs",
    "stone_slab",
    "stone_brick_slab",
    "mossy_stone_brick_slab",
    "mossy_stone_brick_wall",
];
const STONE_BRICKS: usize = 0;
const STONE: usize = 1;
const CHISELED_STONE_BRICKS: usize = 2;
const OBSIDIAN: usize = 3;
const CRYING_OBSIDIAN: usize = 4;
const CRACKED_STONE_BRICKS: usize = 5;
const MOSSY_STONE_BRICKS: usize = 6;
const STONE_BRICK_STAIRS: usize = 7;
const MOSSY_STONE_BRICK_STAIRS: usize = 8;
const STONE_SLAB: usize = 9;
const STONE_BRICK_SLAB: usize = 10;
const MOSSY_STONE_BRICK_SLAB: usize = 11;
const MOSSY_STONE_BRICK_WALL: usize = 12;

const BLACKSTONE: [(&str, &str); 23] = [
    ("cobblestone", "blackstone"),
    ("mossy_cobblestone", "blackstone"),
    ("stone", "polished_blackstone"),
    ("stone_bricks", "polished_blackstone_bricks"),
    ("mossy_stone_bricks", "polished_blackstone_bricks"),
    ("cobblestone_stairs", "blackstone_stairs"),
    ("mossy_cobblestone_stairs", "blackstone_stairs"),
    ("stone_stairs", "polished_blackstone_stairs"),
    ("stone_brick_stairs", "polished_blackstone_brick_stairs"),
    (
        "mossy_stone_brick_stairs",
        "polished_blackstone_brick_stairs",
    ),
    ("cobblestone_slab", "blackstone_slab"),
    ("mossy_cobblestone_slab", "blackstone_slab"),
    ("smooth_stone_slab", "polished_blackstone_slab"),
    ("stone_slab", "polished_blackstone_slab"),
    ("stone_brick_slab", "polished_blackstone_brick_slab"),
    ("mossy_stone_brick_slab", "polished_blackstone_brick_slab"),
    ("stone_brick_wall", "polished_blackstone_brick_wall"),
    ("mossy_stone_brick_wall", "polished_blackstone_brick_wall"),
    ("cobblestone_wall", "blackstone_wall"),
    ("mossy_cobblestone_wall", "blackstone_wall"),
    ("chiseled_stone_bricks", "chiseled_polished_blackstone"),
    ("cracked_stone_bricks", "cracked_polished_blackstone_bricks"),
    ("iron_bars", "iron_chain"),
];

impl KnownBlocks {
    pub fn new(names: &DecodeContext<'_>) -> Self {
        let blocks = names.blocks;
        let id = |name: &str| blocks.block_by_name(&format!("minecraft:{name}"));
        let tag = |name: &str| {
            names
                .block_tag(name)
                .unwrap_or_else(|_| BiomeSet::new(Vec::new()))
        };
        let air = id("air").map_or(BlockState(0), |b| blocks.default_state(b));
        Self {
            air,
            air_block: id("air"),
            jigsaw: id("jigsaw"),
            structure_void: id("structure_void"),
            structure_block: id("structure_block"),
            lava: id("lava"),
            age: AGE_BLOCKS.map(id),
            stairs: tag("minecraft:stairs"),
            slabs: tag("minecraft:slabs"),
            walls: tag("minecraft:walls"),
            blackstone: BLACKSTONE
                .iter()
                .filter_map(|(from, to)| Some((id(from)?, id(to)?)))
                .collect(),
        }
    }

    fn ignore(&self, air: bool, structure_block: bool) -> Processor {
        let mut list = Vec::new();
        if air {
            list.extend(self.air_block);
        }
        if structure_block {
            list.extend(self.structure_block);
        }
        Processor::BlockIgnore(list)
    }

    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
    pub fn ignore_structure_block(&self) -> Processor {
        self.ignore(false, true)
    }
    /// `BlockIgnoreProcessor.AIR`.
    pub fn ignore_air(&self) -> Processor {
        self.ignore(true, false)
    }
    /// `BlockIgnoreProcessor.STRUCTURE_AND_AIR`.
    pub fn ignore_structure_and_air(&self) -> Processor {
        self.ignore(true, true)
    }
}

fn heightmap(name: &str) -> Result<Heightmap, DecodeError> {
    (Heightmap::ALL.into_iter())
        .find(|h| h.key() == name)
        .ok_or(DecodeError::Field("heightmap"))
}

impl Processor {
    /// `StructureProcessorType.SINGLE_CODEC`.
    pub fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let kind = identifier(&string(required(&fields, "processor_type")?)?)?;
        Self::decode_fields(&kind, &fields, names)
    }

    fn decode_fields(
        kind: &str,
        fields: &Object,
        names: &DecodeContext<'_>,
    ) -> Result<Self, DecodeError> {
        Ok(match kind {
            "minecraft:rule" => {
                let rules: Vec<Box<RawValue>> =
                    serde_json::from_str(required(fields, "rules")?.get())?;
                Self::Rule(
                    rules
                        .iter()
                        .map(|r| ProcessorRule::decode(r, names))
                        .collect::<Result<_, _>>()?,
                )
            }
            "minecraft:block_rot" => {
                let integrity = float(required(fields, "integrity")?)?;
                if !(0.0..=1.0).contains(&integrity) {
                    return Err(DecodeError::Field("integrity"));
                }
                Self::BlockRot {
                    rottable: optional(fields, "rottable_blocks")
                        .map(|raw| names.block_set(raw))
                        .transpose()?,
                    integrity,
                }
            }
            "minecraft:block_ignore" => {
                let states: Vec<Box<RawValue>> =
                    serde_json::from_str(required(fields, "blocks")?.get())?;
                Self::BlockIgnore(
                    states
                        .iter()
                        .map(|s| Ok(names.blocks.block(names.block_state(s)?)))
                        .collect::<Result<_, DecodeError>>()?,
                )
            }
            "minecraft:gravity" => Self::Gravity {
                heightmap: optional(fields, "heightmap")
                    .map_or(Ok(Heightmap::WorldSurfaceWg), |raw| {
                        heightmap(&string(raw)?)
                    })?,
                offset: optional(fields, "offset").map_or(Ok(0), integer)?,
            },
            "minecraft:protected_blocks" => {
                Self::ProtectedBlocks(names.block_set(required(fields, "value")?)?)
            }
            "minecraft:capped" => Self::Capped {
                delegate: Box::new(Self::decode(required(fields, "delegate")?, names)?),
                limit: IntProvider::decode_within(required(fields, "limit")?, 1, i32::MAX)?,
            },
            "minecraft:lava_submerged_block" => Self::LavaSubmergedBlock,
            "minecraft:jigsaw_replacement" => Self::JigsawReplacement,
            "minecraft:block_age" => Self::BlockAge {
                mossiness: float(required(fields, "mossiness")?)?,
            },
            "minecraft:blackstone_replace" => Self::BlackstoneReplace,
            "minecraft:nop" => Self::Nop,
            _ => return Err(DecodeError::Field("structure processor type")),
        })
    }

    /// `evaluatesEntirePieceState()`.
    pub fn evaluates_entire_piece(&self) -> bool {
        matches!(self, Self::Capped { .. })
    }
}

impl ProcessorList {
    /// `StructureProcessorType.DIRECT_CODEC`: `{"processors": [...]}` or the list itself.
    pub fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let items: Vec<Box<RawValue>> = if raw.get().trim_start().starts_with('[') {
            serde_json::from_str(raw.get())?
        } else {
            serde_json::from_str(required(&object(raw)?, "processors")?.get())?
        };
        Ok(Self(
            items
                .iter()
                .map(|p| Processor::decode(p, names))
                .collect::<Result<_, _>>()?,
        ))
    }
}

/// The `worldgen/processor_list` registry and the blocks the built-in processors use.
#[derive(Debug)]
pub struct ProcessorLists {
    lists: Vec<(String, ProcessorList)>,
    ids: HashMap<String, ProcessorListId>,
    known: KnownBlocks,
}

impl ProcessorLists {
    pub fn load(packs: &[DataPack], blocks: &BlockStates) -> Result<Self, WorldgenError> {
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let names = DecodeContext {
            blocks,
            block_tags: &block_tags,
            biomes: &[],
            biome_tags: &[],
            fluid_tags: &[],
        };
        let mut lists = Vec::new();
        let mut ids = HashMap::new();
        for_each_element(packs, "minecraft:worldgen/processor_list", |file| {
            let error = |source| WorldgenError {
                file: file.file.to_owned(),
                pack: file.pack.id.clone(),
                source,
            };
            let raw: Box<RawValue> =
                serde_json::from_slice(file.bytes).map_err(|e| error(e.into()))?;
            let list = ProcessorList::decode(&raw, &names).map_err(error)?;
            ids.insert(file.id.to_owned(), ProcessorListId(lists.len() as u32));
            lists.push((file.id.to_owned(), list));
            Ok(())
        })?;
        Ok(Self {
            lists,
            ids,
            known: KnownBlocks::new(&names),
        })
    }

    pub fn known(&self) -> &KnownBlocks {
        &self.known
    }
    pub fn id(&self, name: &str) -> Option<ProcessorListId> {
        self.ids.get(name).copied()
    }
    pub fn get(&self, id: ProcessorListId) -> &ProcessorList {
        &self.lists[id.0 as usize].1
    }
    pub fn name(&self, id: ProcessorListId) -> &str {
        &self.lists[id.0 as usize].0
    }
    pub fn len(&self) -> usize {
        self.lists.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }

    /// `StructureProcessorType.LIST_CODEC`: a registry id or an inline list.
    pub fn holder(
        &self,
        raw: &RawValue,
        names: &DecodeContext<'_>,
    ) -> Result<ProcessorListHolder, DecodeError> {
        if raw.get().trim_start().starts_with('"') {
            let name = identifier(&string(raw)?)?;
            return self
                .id(&name)
                .map(ProcessorListHolder::Registry)
                .ok_or_else(|| DecodeError::Registry(format!("Missing processor list: {name}")));
        }
        ProcessorList::decode(raw, names).map(ProcessorListHolder::Inline)
    }

    pub fn resolve<'a>(&'a self, holder: &'a ProcessorListHolder) -> &'a ProcessorList {
        match holder {
            ProcessorListHolder::Registry(id) => self.get(*id),
            ProcessorListHolder::Inline(list) => list,
        }
    }
}

/// What a processor reads besides the block: the world, the piece's positions and randoms.
pub(crate) struct ProcessContext<'a, W> {
    pub world: &'a W,
    pub known: &'a KnownBlocks,
    /// `processBlock`'s `targetPosition` (the template's placement position).
    pub position: BlockPos,
    pub reference: BlockPos,
    /// The level seed (`CappedProcessor`'s random).
    pub seed: i64,
}

/// A processor failed: the world could not answer, or a provider's bounds were invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(transparent)]
    Bound(#[from] InvalidBound),
}

/// `StructurePlaceSettings.getRandom(pos)` as processors see it: the placement random when the
/// settings carry it, else `RandomSource.create(Mth.getSeed(pos))`.
pub(crate) enum SettingsRandom<'r, R> {
    Shared(&'r mut R),
    PerPosition,
}

impl<R: RandomSource> SettingsRandom<'_, R> {
    fn with<T>(
        &mut self,
        pos: BlockPos,
        shared: impl FnOnce(&mut R) -> T,
        own: impl FnOnce(&mut LegacyRandom) -> T,
    ) -> T {
        match self {
            Self::Shared(random) => shared(random),
            Self::PerPosition => own(&mut LegacyRandom::new(position_seed(pos))),
        }
    }
}

fn rot_drops(random: &mut impl RandomSource, integrity: f32) -> bool {
    // `!(nextFloat() <= integrity)`: neither is NaN (a JSON float, a draw in [0, 1)).
    random.next_float() > integrity
}

impl Processor {
    /// `processBlock(level, targetPosition, referencePos, templateRelativePos, info, settings)`;
    /// `None` drops the block.
    pub(crate) fn process_block<W: PlacementWorld, R: RandomSource>(
        &self,
        cx: &ProcessContext<'_, W>,
        relative: BlockPos,
        info: BlockInfo,
        random: &mut SettingsRandom<'_, R>,
    ) -> Result<Option<BlockInfo>, ProcessError> {
        let blocks = cx.world.blocks();
        let known = cx.known;
        Ok(match self {
            Self::Rule(rules) => {
                let mut rule_random = LegacyRandom::new(position_seed(info.pos));
                for rule in rules {
                    // `ProcessorRule.test`: the world is read only once the input matches.
                    let matches = rule
                        .input
                        .test(blocks, info.state, info.pos, &mut rule_random)
                        && {
                            let there = cx.world.block_state(info.pos)?;
                            rule.location
                                .test(blocks, there, info.pos, &mut rule_random)
                        }
                        && rule.position.test(info.pos, cx.reference, &mut rule_random);
                    if matches {
                        let nbt = rule.modifier.apply(&mut rule_random, info.nbt);
                        return Ok(Some(BlockInfo {
                            pos: info.pos,
                            state: rule.output,
                            nbt,
                        }));
                    }
                }
                Some(info)
            }
            Self::BlockRot {
                rottable,
                integrity,
            } => {
                let candidate = rottable
                    .as_ref()
                    .is_none_or(|set| set.contains(blocks.block(info.state).0));
                // `getRandom` is called first; the float only when the block may rot.
                let drops = candidate
                    && random.with(
                        info.pos,
                        |r| rot_drops(r, *integrity),
                        |r| rot_drops(r, *integrity),
                    );
                (!drops).then_some(info)
            }
            Self::BlockIgnore(ignored) => {
                (!ignored.contains(&blocks.block(info.state))).then_some(info)
            }
            Self::Gravity { heightmap, offset } => {
                // A `WorldGenRegion` is no `ServerLevel`: the heightmap is used as given.
                let height = cx
                    .world
                    .height(*heightmap, info.pos.x, info.pos.z)?
                    .wrapping_add(*offset);
                Some(BlockInfo {
                    pos: BlockPos::new(info.pos.x, height.wrapping_add(relative.y), info.pos.z),
                    ..info
                })
            }
            Self::ProtectedBlocks(set) => {
                let there = cx.world.block_state(info.pos)?;
                (!set.contains(blocks.block(there).0)).then_some(info)
            }
            Self::Capped { .. } => Some(info),
            Self::LavaSubmergedBlock => {
                let there = cx.world.block_state(info.pos)?;
                let was_lava = known.lava.is_some_and(|lava| blocks.block(there) == lava);
                let full = cx.world.state_data().flags(info.state) & FLAG_SHAPE_FULL_BLOCK != 0;
                match known.lava {
                    Some(lava) if was_lava && !full => Some(BlockInfo {
                        state: blocks.default_state(lava),
                        ..info
                    }),
                    _ => Some(info),
                }
            }
            Self::JigsawReplacement => {
                if known.jigsaw != Some(blocks.block(info.state)) {
                    return Ok(Some(info));
                }
                let Some(nbt) = info.nbt.as_deref() else {
                    // Vanilla warns: a jigsaw without NBT stays.
                    return Ok(Some(info));
                };
                let text = match nbt.get(&JavaString::from("final_state")) {
                    Some(Tag::String(s)) => s.to_string_lossy(),
                    _ => "minecraft:air".to_owned(),
                };
                let Some(state) = parse_state(blocks, &text) else {
                    // Vanilla logs the parse error and drops the block.
                    return Ok(None);
                };
                (known.structure_void != Some(blocks.block(state))).then_some(BlockInfo {
                    pos: info.pos,
                    state,
                    nbt: None,
                })
            }
            Self::BlockAge { mossiness } => {
                let state = random.with(
                    info.pos,
                    |r| age(blocks, known, info.state, *mossiness, r),
                    |r| age(blocks, known, info.state, *mossiness, r),
                )?;
                match state {
                    Some(state) => Some(BlockInfo { state, ..info }),
                    None => Some(info),
                }
            }
            Self::BlackstoneReplace => {
                let Some(&to) = known.blackstone.get(&blocks.block(info.state)) else {
                    return Ok(Some(info));
                };
                let mut state = blocks.default_state(to);
                for property in ["facing", "half", "type"] {
                    // `hasProperty(StairBlock.FACING / HALF, SlabBlock.TYPE)`: the stair facing is
                    // horizontal, so a vertical `facing` (none of these blocks) would not count.
                    if let Some(value) = blocks.value(info.state, property) {
                        state = blocks
                            .with_property(state, property, value)
                            .unwrap_or(state);
                    }
                }
                Some(BlockInfo { state, ..info })
            }
            Self::Nop => Some(info),
        })
    }

    /// `finalizeProcessing`: only `CappedProcessor` does anything.
    pub(crate) fn finalize<W: PlacementWorld, R: RandomSource>(
        &self,
        cx: &ProcessContext<'_, W>,
        original: &[BlockInfo],
        processed: &mut [BlockInfo],
        random: &mut SettingsRandom<'_, R>,
    ) -> Result<(), ProcessError> {
        let Self::Capped { delegate, limit } = self else {
            return Ok(());
        };
        // Lists out of step: vanilla logs and leaves them as they are.
        if limit.bounds()[1] == 0 || processed.is_empty() || original.len() != processed.len() {
            return Ok(());
        }
        let mut capped = LegacyRandom::new(cx.seed).fork_positional().at(cx.position);
        let most = limit.sample(&mut capped)?.min(processed.len() as i32);
        if most < 1 {
            return Ok(());
        }
        // `Util.toShuffledList(IntStream.range(0, size), random)`.
        let mut indices: Vec<usize> = (0..processed.len()).collect();
        for i in (2..=indices.len()).rev() {
            let to = capped.next_int_bound(i as i32)? as usize;
            indices.swap(i - 1, to);
        }
        let mut replaced = 0;
        for index in indices {
            if replaced >= most {
                break;
            }
            let info = processed[index].clone();
            let altered = delegate.process_block(cx, original[index].pos, info, random)?;
            if let Some(altered) = altered
                && altered != processed[index]
            {
                replaced += 1;
                processed[index] = altered;
            }
        }
        Ok(())
    }
}

/// `BlockAgeProcessor`'s choice for a state (`None`: unchanged), in its draw order.
fn age(
    blocks: &BlockStates,
    known: &KnownBlocks,
    state: BlockState,
    mossiness: f32,
    random: &mut impl RandomSource,
) -> Result<Option<BlockState>, InvalidBound> {
    let block = blocks.block(state);
    let is = |i: usize| known.age[i] == Some(block);
    let default = |i: usize| known.age[i].map(|b| blocks.default_state(b));
    let with_properties_of = |i: usize| {
        known.age[i].map(|b| {
            blocks
                .values(state)
                .fold(blocks.default_state(b), |s, (name, value)| {
                    blocks.with_property(s, name, value).unwrap_or(s)
                })
        })
    };
    // `getRandomFacingStairs`: a horizontal facing, then a half.
    let random_stairs = |random: &mut dyn FnMut(i32) -> Result<i32, InvalidBound>, i: usize| {
        let facing = ["north", "east", "south", "west"][random(4)? as usize];
        let half = ["top", "bottom"][random(2)? as usize];
        Ok::<_, InvalidBound>(default(i).map(|s| {
            let s = blocks.with_property(s, "facing", facing).unwrap_or(s);
            blocks.with_property(s, "half", half).unwrap_or(s)
        }))
    };
    let pick = |random: &mut dyn FnMut(i32) -> Result<i32, InvalidBound>,
                choices: [Option<BlockState>; 2]| {
        Ok::<_, InvalidBound>(choices[random(2)? as usize])
    };
    let block_set = |set: &BiomeSet| set.contains(block.0);
    if is(STONE_BRICKS) || is(STONE) || is(CHISELED_STONE_BRICKS) {
        if random.next_float() >= 0.5 {
            return Ok(None);
        }
        let mut int = |bound| random.next_int_bound(bound);
        let plain = [
            default(CRACKED_STONE_BRICKS),
            random_stairs(&mut int, STONE_BRICK_STAIRS)?,
        ];
        let mossy = [
            default(MOSSY_STONE_BRICKS),
            random_stairs(&mut int, MOSSY_STONE_BRICK_STAIRS)?,
        ];
        let mossy_pick = random.next_float() < mossiness;
        let mut int = |bound| random.next_int_bound(bound);
        return pick(&mut int, if mossy_pick { mossy } else { plain });
    }
    if block_set(&known.stairs) {
        if random.next_float() >= 0.5 {
            return Ok(None);
        }
        let mossy = [
            with_properties_of(MOSSY_STONE_BRICK_STAIRS),
            default(MOSSY_STONE_BRICK_SLAB),
        ];
        let plain = [default(STONE_SLAB), default(STONE_BRICK_SLAB)];
        let mossy_pick = random.next_float() < mossiness;
        let mut int = |bound| random.next_int_bound(bound);
        return pick(&mut int, if mossy_pick { mossy } else { plain });
    }
    if block_set(&known.slabs) {
        return Ok((random.next_float() < mossiness)
            .then(|| with_properties_of(MOSSY_STONE_BRICK_SLAB))
            .flatten());
    }
    if block_set(&known.walls) {
        return Ok((random.next_float() < mossiness)
            .then(|| with_properties_of(MOSSY_STONE_BRICK_WALL))
            .flatten());
    }
    if is(OBSIDIAN) {
        return Ok((random.next_float() < 0.15)
            .then(|| default(CRYING_OBSIDIAN))
            .flatten());
    }
    Ok(None)
}

/// `BlockStateParser.parseForBlock(blocks, text, allowNbt = true).blockState()`: an id, optional
/// `[key=value,...]` and optional SNBT; `None` where the parser throws. What follows is ignored.
pub(crate) fn parse_state(blocks: &BlockStates, text: &str) -> Option<BlockState> {
    use crate::command::StringReader;
    let mut reader = StringReader::new(text);
    let start = reader.cursor();
    // `Identifier.read`: the identifier characters, then `Identifier.parse`.
    while reader.can_read()
        && matches!(reader.peek(), 'a'..='z' | '0'..='9' | '_' | '-' | '.' | ':' | '/')
    {
        reader.skip();
    }
    let name = crate::registry::parse_identifier(&text[start..reader.cursor()]).ok()?;
    let block = blocks.block_by_name(&name)?;
    let mut state = blocks.default_state(block);
    if reader.can_read() && reader.peek() == '[' {
        reader.skip();
        reader.skip_whitespace();
        let mut seen = Vec::new();
        while reader.can_read() && reader.peek() != ']' {
            reader.skip_whitespace();
            let key = reader.read_string().ok()?;
            blocks.property(block, &key)?;
            if seen.contains(&key) {
                return None;
            }
            reader.skip_whitespace();
            if !reader.can_read() || reader.peek() != '=' {
                return None;
            }
            reader.skip();
            reader.skip_whitespace();
            let value = reader.read_string().ok()?;
            state = blocks.with_property(state, &key, &value)?;
            seen.push(key);
            reader.skip_whitespace();
            if reader.can_read() {
                if reader.peek() != ',' {
                    if reader.peek() != ']' {
                        return None;
                    }
                    break;
                }
                reader.skip();
            }
        }
        if !reader.can_read() {
            return None;
        }
        reader.skip();
    }
    if reader.can_read() && reader.peek() == '{' {
        match crate::nbt::parse_snbt(reader.remaining()) {
            Ok((Tag::Compound(_), _)) => {}
            _ => return None,
        }
    }
    Some(state)
}
