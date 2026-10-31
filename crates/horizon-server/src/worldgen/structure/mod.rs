//! Structures (26.3's `levelgen.structure`, with Paper's and Leaf's changes): the `worldgen/structure`
//! and `worldgen/structure_set` registries with `tags/worldgen/structure`, decoded once at startup
//! (this module), where structures may start ([`placement`]), and structure starts and references
//! as chunks save them ([`start`]).
//!
//! What is decoded here is what placement needs and what every structure type shares
//! (`Structure.StructureSettings`); each type's own fields stay as the element's JSON for the
//! piece generators (jigsaw, templates, legacy pieces), which come next.
//!
//! Pieces build on templates ([`template`]: `StructureTemplate` and its manager), their placement
//! ([`place`]: `StructurePlaceSettings`, `placeInWorld`) and the structure processors with the
//! `worldgen/processor_list` registry ([`processor`]).
//!
//! Ported from Horizon's generator (`horizon-world::worldgen::structure::start`), without its
//! modpack placements and keys (`horizon:*`).

pub mod jigsaw;
pub mod legacy_piece;
pub mod piece_place;
pub mod place;
pub mod placement;
pub mod pool;
pub mod processor;
pub mod scattered;
pub mod start;
pub mod template;

#[cfg(test)]
mod jigsaw_oracle_tests;
#[cfg(test)]
mod legacy_oracle_tests;
#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod template_oracle_tests;
#[cfg(test)]
mod template_tests;
#[cfg(test)]
mod tests;

use serde_json::value::RawValue;

use super::codec::{
    DecodeError, Object, float, identifier, int_range, integer, object, optional, required, string,
};
use super::density::TerrainAdjustment;

/// A structure: its index in [`StructureRegistry::structures`] (registration order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructureId(pub u32);

/// A structure set: its index in [`StructureRegistry::sets`] (registration order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructureSetId(pub u32);

/// `GenerationStep.Decoration`, in its order (the FEATURES step's index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Decoration {
    RawGeneration,
    Lakes,
    LocalModifications,
    UndergroundStructures,
    SurfaceStructures,
    Strongholds,
    UndergroundOres,
    UndergroundDecoration,
    FluidSprings,
    VegetalDecoration,
    TopLayerModification,
}

impl Decoration {
    pub const ALL: [Self; 11] = [
        Self::RawGeneration,
        Self::Lakes,
        Self::LocalModifications,
        Self::UndergroundStructures,
        Self::SurfaceStructures,
        Self::Strongholds,
        Self::UndergroundOres,
        Self::UndergroundDecoration,
        Self::FluidSprings,
        Self::VegetalDecoration,
        Self::TopLayerModification,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::RawGeneration => "raw_generation",
            Self::Lakes => "lakes",
            Self::LocalModifications => "local_modifications",
            Self::UndergroundStructures => "underground_structures",
            Self::SurfaceStructures => "surface_structures",
            Self::Strongholds => "strongholds",
            Self::UndergroundOres => "underground_ores",
            Self::UndergroundDecoration => "underground_decoration",
            Self::FluidSprings => "fluid_springs",
            Self::VegetalDecoration => "vegetal_decoration",
            Self::TopLayerModification => "top_layer_modification",
        }
    }
}

/// A set of biomes (`HolderSet<Biome>`) as `worldgen/biome` registry indices (protocol ids),
/// sorted. Vanilla only asks whether it contains a biome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BiomeSet(Box<[u32]>);

impl BiomeSet {
    pub fn new(mut biomes: Vec<u32>) -> Self {
        biomes.sort_unstable();
        biomes.dedup();
        Self(biomes.into())
    }
    pub fn contains(&self, biome: u32) -> bool {
        self.0.binary_search(&biome).is_ok()
    }
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().copied()
    }
}

/// The biome registry's ids and tags, to decode biome holder sets.
pub(crate) struct BiomeLookup<'a> {
    /// Ids in registration order.
    pub ids: &'a [String],
    pub tags: &'a [(String, Vec<i32>)],
}

impl BiomeLookup<'_> {
    /// `RegistryCodecs.homogeneousList(Registries.BIOME)`: `"#tag"`, an id, or a list of ids.
    pub(crate) fn decode(&self, raw: &RawValue) -> Result<BiomeSet, DecodeError> {
        let id = |name: &str| {
            let name = identifier(name)?;
            self.ids
                .iter()
                .position(|id| *id == name)
                .map(|i| i as u32)
                .ok_or_else(|| DecodeError::Registry(format!("Missing biome: {name}")))
        };
        if raw.get().starts_with('[') {
            let names: Vec<String> = serde_json::from_str(raw.get())?;
            return names
                .iter()
                .map(|n| id(n))
                .collect::<Result<_, _>>()
                .map(BiomeSet::new);
        }
        let name = string(raw)?;
        match name.strip_prefix('#') {
            Some(tag) => {
                let tag = identifier(tag)?;
                let (_, biomes) = self.tags.iter().find(|(t, _)| *t == tag).ok_or_else(|| {
                    DecodeError::Registry(format!("Missing tag: #{tag} in worldgen/biome"))
                })?;
                Ok(BiomeSet::new(biomes.iter().map(|&b| b as u32).collect()))
            }
            None => Ok(BiomeSet::new(vec![id(&name)?])),
        }
    }
}

/// A `worldgen/structure` element: `Structure.StructureSettings` and its type's own fields.
#[derive(Debug)]
pub struct Structure {
    pub id: String,
    /// The `StructureType` (`minecraft:jigsaw`, `minecraft:mineshaft`...).
    pub kind: String,
    pub biomes: BiomeSet,
    pub step: Decoration,
    /// `StructureStart.getBoundingBox` inflates a start's box by 12 unless this is `None`.
    pub terrain_adaptation: TerrainAdjustment,
    /// `spawn_overrides` (by mob category), for natural spawning to decode.
    pub spawn_overrides: Box<RawValue>,
    /// The whole element, for the type's own fields.
    pub json: Box<RawValue>,
    /// A `minecraft:jigsaw` structure's fields (decoded with the template pools).
    pub jigsaw: Option<jigsaw::JigsawStructure>,
}

/// `AbstractSpreadingStructurePlacement.FrequencyReductionMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrequencyReduction {
    Default,
    /// `legacy_type_1`: pillager outposts' reducer.
    LegacyType1,
    /// `legacy_type_2`: the arbitrary salt 10387320 (buried treasure).
    LegacyType2,
    /// `legacy_type_3`: `setLargeFeatureSeed` and a double (mineshafts).
    LegacyType3,
}

/// `AbstractSpreadingStructurePlacement.ExclusionZone`: no start within `chunk_count` chunks of a
/// start chunk of `other_set` (by that set's own placement, never Paper's salts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExclusionZone {
    pub other_set: StructureSetId,
    pub chunk_count: i32,
}

/// `AbstractSpreadingStructurePlacement`'s fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spreading {
    pub locate_offset: [i32; 3],
    pub frequency_reduction: FrequencyReduction,
    pub frequency: f32,
    pub salt: i32,
    pub exclusion_zone: Option<ExclusionZone>,
}

/// `RandomSpreadType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpreadType {
    Linear,
    Triangular,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RandomSpread {
    pub spreading: Spreading,
    pub spacing: i32,
    pub separation: i32,
    pub spread_type: SpreadType,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConcentricRings {
    pub spreading: Spreading,
    pub distance: i32,
    pub spread: i32,
    pub count: i32,
    pub preferred_biomes: BiomeSet,
}

/// `StructurePlacement` (the `structure_placement` registry's three types).
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    RandomSpread(RandomSpread),
    ConcentricRings(ConcentricRings),
    /// Only the dimension's origin chunk (`ChunkGeneratorStructureState.getDimensionOrigin`).
    DimensionOrigin,
}

impl Placement {
    pub fn spreading(&self) -> Option<&Spreading> {
        match self {
            Self::RandomSpread(spread) => Some(&spread.spreading),
            Self::ConcentricRings(rings) => Some(&rings.spreading),
            Self::DimensionOrigin => None,
        }
    }
    /// `StructurePlacement.locateOffset`.
    pub fn locate_offset(&self) -> [i32; 3] {
        self.spreading().map_or([0; 3], |s| s.locate_offset)
    }
}

/// A `worldgen/structure_set` element.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureSet {
    pub id: String,
    /// Structures and their weights, in the file's order.
    pub structures: Vec<(StructureId, i32)>,
    pub placement: Placement,
    /// Written inline in another set's file (a direct `Holder`): never a candidate for a level.
    pub inline: bool,
}

/// The elements written inline in `worldgen/structure_set` files while decoding. They take the
/// ids after the registered ones, so they are appended to the registry once every file is decoded.
#[derive(Debug, Default)]
pub struct InlineElements {
    pub structures: Vec<Structure>,
    pub sets: Vec<StructureSet>,
}

/// The structure registries, decoded.
#[derive(Debug, Default)]
pub struct StructureRegistry {
    pub structures: Vec<Structure>,
    pub sets: Vec<StructureSet>,
    /// `tags/worldgen/structure`, structures in tag order.
    pub tags: Vec<(String, Vec<StructureId>)>,
    /// `worldgen/template_pool`.
    pub pools: pool::TemplatePools,
    /// The structure templates (`StructureTemplateManager`), given once loaded
    /// ([`crate::worldgen::data::WorldgenData::set_templates`]).
    pub templates: template::StructureTemplates,
}

impl StructureRegistry {
    pub fn structure(&self, id: StructureId) -> &Structure {
        &self.structures[id.0 as usize]
    }
    pub fn set(&self, id: StructureSetId) -> &StructureSet {
        &self.sets[id.0 as usize]
    }
    pub fn structure_id(&self, name: &str) -> Option<StructureId> {
        let index = self.structures.iter().position(|s| s.id == name)?;
        Some(StructureId(index as u32))
    }
    pub fn set_id(&self, name: &str) -> Option<StructureSetId> {
        let index = self.sets.iter().position(|s| s.id == name)?;
        Some(StructureSetId(index as u32))
    }
    pub fn tag(&self, name: &str) -> Option<&[StructureId]> {
        let (_, structures) = self.tags.iter().find(|(tag, _)| tag == name)?;
        Some(structures)
    }
}

/// `Structure.DIRECT_CODEC`'s shared part (`StructureSettings.CODEC`; unknown fields ignored).
pub(crate) fn decode_structure(
    id: &str,
    raw: Box<RawValue>,
    biomes: &BiomeLookup<'_>,
) -> Result<Structure, DecodeError> {
    let fields = object(&raw)?;
    let kind = identifier(&string(required(&fields, "type")?)?)?;
    let step = string(required(&fields, "step")?)?;
    let step = (Decoration::ALL.into_iter())
        .find(|d| d.name() == step)
        .ok_or(DecodeError::Field("step"))?;
    let terrain_adaptation = match optional(&fields, "terrain_adaptation") {
        None => TerrainAdjustment::None,
        Some(raw) => match string(raw)?.as_str() {
            "none" => TerrainAdjustment::None,
            "bury" => TerrainAdjustment::Bury,
            "beard_thin" => TerrainAdjustment::BeardThin,
            "beard_box" => TerrainAdjustment::BeardBox,
            "encapsulate" => TerrainAdjustment::Encapsulate,
            _ => return Err(DecodeError::Field("terrain_adaptation")),
        },
    };
    let spawn_overrides = required(&fields, "spawn_overrides")?;
    if !spawn_overrides.get().starts_with('{') {
        return Err(DecodeError::Field("spawn_overrides"));
    }
    Ok(Structure {
        id: id.to_owned(),
        kind,
        biomes: biomes.decode(required(&fields, "biomes")?)?,
        step,
        terrain_adaptation,
        spawn_overrides: spawn_overrides.to_owned(),
        json: raw,
        jigsaw: None,
    })
}

/// `StructureSet.DIRECT_CODEC`. A reference to a structure or set (a `RegistryFileCodec`) is an id
/// naming a registered element, or the element itself written inline, which is decoded the same
/// way and kept in `inline`.
pub(crate) fn decode_structure_set(
    id: &str,
    raw: &RawValue,
    structures: &[Structure],
    set_ids: &[String],
    biomes: &BiomeLookup<'_>,
    inline: &mut InlineElements,
) -> Result<StructureSet, DecodeError> {
    let context = SetContext {
        owner: id,
        registered: structures,
        set_ids,
        biomes,
    };
    let fields = object(raw)?;
    let entries: Vec<Box<RawValue>> = serde_json::from_str(required(&fields, "structures")?.get())?;
    let structures = entries
        .iter()
        .map(|entry| {
            let entry = object(entry)?;
            let reference = required(&entry, "structure")?;
            let index = if reference.get().starts_with('{') {
                let index = context.registered.len() + inline.structures.len();
                let inline_id = format!("{id}#inline{index}");
                let structure = decode_structure(&inline_id, reference.to_owned(), biomes)?;
                inline.structures.push(structure);
                index
            } else {
                let name = identifier(&string(reference)?)?;
                (context.registered.iter().position(|s| s.id == name))
                    .ok_or_else(|| DecodeError::Registry(format!("Missing structure: {name}")))?
            };
            let weight = int_range(required(&entry, "weight")?, "weight", 1, i32::MAX)?;
            Ok((StructureId(index as u32), weight))
        })
        .collect::<Result<_, DecodeError>>()?;
    let placement = decode_placement(&object(required(&fields, "placement")?)?, &context, inline)?;
    Ok(StructureSet {
        id: id.to_owned(),
        structures,
        placement,
        inline: false,
    })
}

/// What decoding one set's references needs: the registered elements and the owner's id.
struct SetContext<'a> {
    owner: &'a str,
    registered: &'a [Structure],
    set_ids: &'a [String],
    biomes: &'a BiomeLookup<'a>,
}

fn decode_placement(
    fields: &Object,
    context: &SetContext<'_>,
    inline: &mut InlineElements,
) -> Result<Placement, DecodeError> {
    let biomes = context.biomes;
    let int = |name: &'static str, low, high| int_range(required(fields, name)?, name, low, high);
    match identifier(&string(required(fields, "type")?)?)?.as_str() {
        "minecraft:random_spread" => {
            let spreading = decode_spreading(fields, context, inline)?;
            let spacing = int("spacing", 0, 4096)?;
            let separation = int("separation", 0, 4096)?;
            if spacing <= separation {
                return Err(DecodeError::Registry(
                    "Spacing has to be larger than separation".into(),
                ));
            }
            let spread_type = match optional(fields, "spread_type").map(string).transpose()? {
                None => SpreadType::Linear,
                Some(name) => match name.as_str() {
                    "linear" => SpreadType::Linear,
                    "triangular" => SpreadType::Triangular,
                    _ => return Err(DecodeError::Field("spread_type")),
                },
            };
            Ok(Placement::RandomSpread(RandomSpread {
                spreading,
                spacing,
                separation,
                spread_type,
            }))
        }
        "minecraft:concentric_rings" => Ok(Placement::ConcentricRings(ConcentricRings {
            spreading: decode_spreading(fields, context, inline)?,
            distance: int("distance", 0, 1023)?,
            spread: int("spread", 0, 1023)?,
            count: int("count", 1, 4095)?,
            preferred_biomes: biomes.decode(required(fields, "preferred_biomes")?)?,
        })),
        "minecraft:dimension_origin" => Ok(Placement::DimensionOrigin),
        other => Err(DecodeError::Registry(format!(
            "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/structure_placement]: {other}"
        ))),
    }
}

/// `AbstractSpreadingStructurePlacement.placementCodec`.
fn decode_spreading(
    fields: &Object,
    context: &SetContext<'_>,
    inline: &mut InlineElements,
) -> Result<Spreading, DecodeError> {
    let locate_offset = match optional(fields, "locate_offset") {
        None => [0; 3],
        Some(raw) => {
            let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
            let values = values
                .iter()
                .map(|v| integer(v))
                .collect::<Result<Vec<_>, _>>()?;
            let offset: [i32; 3] = values
                .try_into()
                .map_err(|_| DecodeError::Field("locate_offset"))?;
            if offset.iter().any(|v| v.unsigned_abs() > 16) {
                return Err(DecodeError::Field("locate_offset"));
            }
            offset
        }
    };
    let frequency_reduction = match optional(fields, "frequency_reduction_method")
        .map(string)
        .transpose()?
        .as_deref()
    {
        None | Some("default") => FrequencyReduction::Default,
        Some("legacy_type_1") => FrequencyReduction::LegacyType1,
        Some("legacy_type_2") => FrequencyReduction::LegacyType2,
        Some("legacy_type_3") => FrequencyReduction::LegacyType3,
        Some(_) => return Err(DecodeError::Field("frequency_reduction_method")),
    };
    let frequency = match optional(fields, "frequency") {
        None => 1.0,
        Some(raw) => {
            let value = float(raw)?;
            if !(0.0..=1.0).contains(&value) {
                return Err(DecodeError::Field("frequency"));
            }
            value
        }
    };
    let salt = int_range(required(fields, "salt")?, "salt", 0, i32::MAX)?;
    let exclusion_zone = match optional(fields, "exclusion_zone") {
        None => None,
        Some(raw) => {
            let zone = object(raw)?;
            let reference = required(&zone, "other_set")?;
            let index = if reference.get().starts_with('{') {
                let inline_id = format!("{}#inline_set", context.owner);
                let mut set = decode_structure_set(
                    &inline_id,
                    reference,
                    context.registered,
                    context.set_ids,
                    context.biomes,
                    inline,
                )?;
                set.inline = true;
                let index = context.set_ids.len() + inline.sets.len();
                inline.sets.push(set);
                index
            } else {
                let name = identifier(&string(reference)?)?;
                (context.set_ids.iter().position(|id| *id == name)).ok_or_else(|| {
                    DecodeError::Registry(format!("Missing structure set: {name}"))
                })?
            };
            Some(ExclusionZone {
                other_set: StructureSetId(index as u32),
                chunk_count: int_range(required(&zone, "chunk_count")?, "chunk_count", 1, 16)?,
            })
        }
    };
    Ok(Spreading {
        locate_offset,
        frequency_reduction,
        frequency,
        salt,
        exclusion_zone,
    })
}
