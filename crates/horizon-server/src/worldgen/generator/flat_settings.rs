//! `FlatLevelGeneratorSettings` (`CODEC`: `structure_overrides`, `layers`, `lakes`, `features`,
//! `biome`) and the `generator-settings` of `server.properties`
//! (`DedicatedServerProperties.WorldDimensionData.create`), decoded from JSON (a level's saved
//! generator goes through `nbt_to_json` first).
//!
//! Only what the codec keeps is here; the layers' nulling for non-opaque blocks, the feature
//! lists and the structure state are the generator's (`fixed.rs`).

use serde_json::Value;

use crate::block::{BlockState, BlockStates};
use crate::worldgen::biome::codec::BiomeIds;
use crate::worldgen::structure::{StructureRegistry, StructureSetId};

/// `DimensionType.Y_SIZE`.
const Y_SIZE: i64 = 4064;

/// Why settings do not decode (vanilla logs the codec's error and keeps the preset's generator).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FlatError(pub String);

fn err<T>(message: impl Into<String>) -> Result<T, FlatError> {
    Err(FlatError(message.into()))
}

/// Decoded `FlatLevelGeneratorSettings`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatSettings {
    /// The biome's `worldgen/biome` id.
    pub biome: u32,
    /// `layersInfo`: each layer's block (its default state) and height, bottom first.
    pub layers: Vec<(BlockState, i32)>,
    /// `features` (`setDecoration`).
    pub features: bool,
    /// `lakes` (`setAddLakes`).
    pub lakes: bool,
    /// `structure_overrides`: the structure sets, in the list's order (`None`: every set).
    pub structure_overrides: Option<Vec<StructureSetId>>,
}

/// What the codec resolves ids against.
pub struct FlatLookup<'a> {
    pub biomes: &'a BiomeIds<'a>,
    pub blocks: &'a BlockStates,
    pub structures: &'a StructureRegistry,
}

fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

/// A codec boolean: JSON's, or NBT's byte.
fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|v| v != 0),
        _ => None,
    }
}

impl FlatSettings {
    /// `FlatLevelGeneratorSettings.CODEC.parse`. A biome that does not resolve is plains (vanilla
    /// logs "Unknown biome, defaulting to plains"); everything else wrong fails the parse.
    pub fn decode(value: &Value, lookup: &FlatLookup<'_>) -> Result<Self, FlatError> {
        let Some(object) = value.as_object() else {
            return err("flat generator settings are not an object");
        };
        let structure_overrides = match object.get("structure_overrides") {
            // `lenientOptionalFieldOf`: a value that does not decode is as if missing.
            Some(value) => Self::decode_overrides(value, lookup.structures),
            None => None,
        };
        let Some(layers) = object.get("layers").and_then(Value::as_array) else {
            return err("No key layers in flat generator settings");
        };
        let mut decoded = Vec::with_capacity(layers.len());
        let mut total = 0i64;
        for layer in layers {
            let Some(height) = layer
                .get("height")
                .and_then(Value::as_i64)
                .filter(|h| (0..=Y_SIZE).contains(h))
            else {
                return err("a flat layer's height is not an integer in 0..=4064");
            };
            let Some(name) = layer.get("block").and_then(Value::as_str) else {
                return err("a flat layer has no block");
            };
            let Some(block) = lookup.blocks.block_by_name(&namespaced(name)) else {
                return err(format!("Unknown block {name} in a flat layer"));
            };
            total += height;
            decoded.push((lookup.blocks.default_state(block), height as i32));
        }
        if total > Y_SIZE {
            return err("Sum of layer heights is > 4064");
        }
        let flag = |key: &str| match object.get(key) {
            None => Ok(false),
            Some(v) => boolean(v).ok_or_else(|| FlatError(format!("{key} is not a boolean"))),
        };
        let biome = object
            .get("biome")
            .and_then(Value::as_str)
            .and_then(|name| lookup.biomes.id(&namespaced(name)))
            .or_else(|| {
                tracing::error!("Unknown biome, defaulting to plains");
                lookup.biomes.id("minecraft:plains")
            });
        let Some(biome) = biome else {
            return err("the registries have no minecraft:plains");
        };
        Ok(Self {
            biome,
            layers: decoded,
            features: flag("features")?,
            lakes: flag("lakes")?,
            structure_overrides,
        })
    }

    /// `RegistryCodecs.holderSet(STRUCTURE_SET)`: an id, or a list of ids (a tag does not decode here).
    fn decode_overrides(
        value: &Value,
        registry: &StructureRegistry,
    ) -> Option<Vec<StructureSetId>> {
        let one = |v: &Value| registry.set_id(&namespaced(v.as_str()?));
        match value {
            Value::String(_) => Some(vec![one(value)?]),
            Value::Array(items) => items.iter().map(one).collect(),
            _ => None,
        }
    }

    /// `FlatLevelGeneratorSettings.getLayers`: one state per block of height, from the bottom.
    pub fn layer_states(&self) -> Vec<BlockState> {
        self.layers
            .iter()
            .flat_map(|&(state, height)| std::iter::repeat_n(state, height.max(0) as usize))
            .collect()
    }

    /// The settings as `FlatLevelGeneratorSettings.CODEC` writes them (what a level's saved
    /// generator holds), through JSON: `structure_overrides` when present, `layers`, `lakes`,
    /// `features`, `biome`.
    pub fn to_json(
        &self,
        blocks: &BlockStates,
        biomes: &BiomeIds<'_>,
        structures: &StructureRegistry,
    ) -> Value {
        let mut out = serde_json::Map::new();
        if let Some(sets) = &self.structure_overrides {
            let names: Vec<Value> = sets
                .iter()
                .map(|&s| Value::String(structures.set(s).id.clone()))
                .collect();
            out.insert("structure_overrides".into(), Value::Array(names));
        }
        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|&(state, height)| {
                serde_json::json!({
                    "block": blocks.block_name(blocks.block(state)),
                    "height": height,
                })
            })
            .collect();
        out.insert("layers".into(), Value::Array(layers));
        out.insert("lakes".into(), Value::Bool(self.lakes));
        out.insert("features".into(), Value::Bool(self.features));
        out.insert(
            "biome".into(),
            Value::String(
                biomes
                    .name(self.biome)
                    .unwrap_or("minecraft:plains")
                    .to_owned(),
            ),
        );
        Value::Object(out)
    }
}
