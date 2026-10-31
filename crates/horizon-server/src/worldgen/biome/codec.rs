//! Biome sources as `BiomeSource.CODEC` reads them (a level stem's `generator.biome_source`, in
//! a world preset's JSON or `level.dat`'s NBT), and the multi-noise presets.
//!
//! The two built-in presets (`MultiNoiseBiomeSourceParameterList.Preset`: overworld, nether) are
//! Java code (`OverworldBiomeBuilder`), not data: the prepare step's data generator writes them
//! as `reports/biome_parameters/<ns>/<path>.json` (`BiomeParametersDumpReport`, the parameter
//! points in their codec's float form), read here. The `worldgen/multi_noise_biome_source_
//! parameter_list` registry elements name a preset (`{"preset": "minecraft:overworld"}`).
//!
//! Copied from Horizon's `horizon-world::worldgen::biome_source::codec` and Horizon's
//! `quantize_parameter_report`, decoding straight to registry ids.

use std::collections::HashMap;
use std::path::Path;

use serde_json::value::RawValue;

use super::{BiomeSource, EndBiomes};
use crate::nbt::Tag;
use crate::registry::SyncedRegistry;
use crate::worldgen::climate::{ClimateIndex, ParameterPoint};
use crate::worldgen::codec::{
    DecodeError, identifier, int_range, object, optional, required, string,
};
use crate::worldgen::data::WorldgenData;

/// `MultiNoiseBiomeSourceParameterList.Preset.BY_NAME`.
pub const BUILT_IN_PRESETS: [&str; 2] = ["minecraft:overworld", "minecraft:nether"];

/// The biome registry by name (the synced `worldgen/biome` registry: index = id).
#[derive(Debug)]
pub struct BiomeIds<'a> {
    registry: &'a SyncedRegistry,
    by_name: HashMap<&'a str, u32>,
}

impl<'a> BiomeIds<'a> {
    pub fn new(registry: &'a SyncedRegistry) -> Self {
        Self {
            registry,
            by_name: registry
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.id.as_str(), i as u32))
                .collect(),
        }
    }

    pub fn id(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    pub fn name(&self, id: u32) -> Option<&'a str> {
        self.registry
            .entries
            .get(id as usize)
            .map(|e| e.id.as_str())
    }

    /// `Biome.CODEC`'s reference form: a registry id. (An inline biome would have no id.)
    fn decode(&self, raw: &RawValue) -> Result<u32, DecodeError> {
        if !raw.get().starts_with('"') {
            return Err(DecodeError::Field("inline biomes are not supported"));
        }
        let name = identifier(&string(raw)?)?;
        self.id(&name)
            .ok_or_else(|| DecodeError::Registry(format!("Unknown registry key: {name}")))
    }

    /// `Biome.LIST_CODEC` (a holder set): `#tag`, one id, or a list of ids.
    fn decode_set(&self, raw: &RawValue) -> Result<Vec<u32>, DecodeError> {
        if raw.get().starts_with('"') {
            let text = string(raw)?;
            if let Some(tag) = text.strip_prefix('#') {
                let tag = identifier(tag)?;
                let ids = self
                    .registry
                    .tags
                    .get(&tag)
                    .ok_or_else(|| DecodeError::Registry(format!("Missing tag: {tag}")))?;
                return Ok(ids.iter().map(|&id| id as u32).collect());
            }
            return Ok(vec![self.decode(raw)?]);
        }
        let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
        values.iter().map(|v| self.decode(v)).collect()
    }
}

/// A parameter list in `ParameterList.codec`'s form: a non-empty list of `{parameters, biome}`.
pub fn decode_parameter_list(
    raw: &RawValue,
    biomes: &BiomeIds<'_>,
) -> Result<Vec<(ParameterPoint, u32)>, DecodeError> {
    let entries: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
    if entries.is_empty() {
        return Err(DecodeError::Field("an empty biome parameter list"));
    }
    entries
        .iter()
        .map(|entry| {
            let entry = object(entry)?;
            Ok((
                ParameterPoint::decode(required(&entry, "parameters")?)?,
                biomes.decode(required(&entry, "biome")?)?,
            ))
        })
        .collect()
}

/// The multi-noise presets and the parameter list registry's elements. Immutable; levels clone
/// the trees they use.
#[derive(Clone, Debug)]
pub struct BiomePresets {
    /// Built-in preset id -> its tree.
    presets: Vec<(String, ClimateIndex<u32>)>,
    /// Registry element id -> index into `presets`.
    elements: Vec<(String, usize)>,
}

impl BiomePresets {
    /// The presets from the data generator's reports directory and the registry elements from
    /// the worldgen data.
    pub fn load(
        reports: &Path,
        data: &WorldgenData,
        biomes: &BiomeIds<'_>,
    ) -> Result<Self, DecodeError> {
        let elements = data
            .raw("minecraft:worldgen/multi_noise_biome_source_parameter_list")
            .map(|registry| {
                registry
                    .elements
                    .iter()
                    .map(|(id, raw)| (id.clone(), raw.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Self::build(
            |preset| {
                let (ns, path) = preset.split_once(':').expect("an identifier");
                let file = reports.join(format!("biome_parameters/{ns}/{path}.json"));
                std::fs::read_to_string(&file)
                    .map_err(|e| DecodeError::Registry(format!("{}: {e}", file.display())))
            },
            elements,
            biomes,
        )
    }

    /// `report` gives a built-in preset's report text; `elements` are the registry's.
    pub fn build(
        mut report: impl FnMut(&str) -> Result<String, DecodeError>,
        elements: Vec<(String, Box<RawValue>)>,
        biomes: &BiomeIds<'_>,
    ) -> Result<Self, DecodeError> {
        let mut presets = Vec::new();
        for name in BUILT_IN_PRESETS {
            let text = report(name)?;
            let raw: Box<RawValue> = serde_json::from_str(&text)?;
            let entries = decode_parameter_list(required(&object(&raw)?, "biomes")?, biomes)?;
            let index =
                ClimateIndex::new(entries).map_err(|e| DecodeError::Registry(e.to_string()))?;
            presets.push((name.to_owned(), index));
        }
        let mut this = Self {
            presets,
            elements: Vec::new(),
        };
        for (id, raw) in elements {
            let preset = this.decode_direct(&raw)?;
            this.elements.push((id, preset));
        }
        Ok(this)
    }

    /// A built-in preset's tree.
    pub fn preset(&self, name: &str) -> Option<&ClimateIndex<u32>> {
        self.presets
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, index)| index)
    }

    /// `MultiNoiseBiomeSourceParameterList.DIRECT_CODEC`: `{"preset": <built-in preset>}`.
    fn decode_direct(&self, raw: &RawValue) -> Result<usize, DecodeError> {
        let name = identifier(&string(required(&object(raw)?, "preset")?)?)?;
        self.presets
            .iter()
            .position(|(id, _)| *id == name)
            .ok_or_else(|| DecodeError::Registry(format!("Unknown preset: {name}")))
    }

    /// `MultiNoiseBiomeSourceParameterList.CODEC`: a registry element id, or the direct form.
    fn decode_holder(&self, raw: &RawValue) -> Result<&ClimateIndex<u32>, DecodeError> {
        let preset = if raw.get().starts_with('"') {
            let name = identifier(&string(raw)?)?;
            self.elements
                .iter()
                .find(|(id, _)| *id == name)
                .map(|(_, preset)| *preset)
                .ok_or_else(|| DecodeError::Registry(format!("Unknown registry key: {name}")))?
        } else {
            self.decode_direct(raw)?
        };
        Ok(&self.presets[preset].1)
    }
}

/// `BiomeSource.CODEC`: dispatched on `type`.
pub fn decode_biome_source(
    raw: &RawValue,
    biomes: &BiomeIds<'_>,
    presets: &BiomePresets,
) -> Result<BiomeSource, DecodeError> {
    let fields = object(raw)?;
    let kind = identifier(&string(required(&fields, "type")?)?)?;
    Ok(match kind.as_str() {
        "minecraft:fixed" => BiomeSource::Fixed(biomes.decode(required(&fields, "biome")?)?),
        "minecraft:checkerboard" => BiomeSource::Checkerboard {
            biomes: biomes.decode_set(required(&fields, "biomes")?)?,
            scale: optional(&fields, "scale")
                .map_or(Ok(2), |v| int_range(v, "checkerboard scale", 0, 62))?,
        },
        "minecraft:the_end" => {
            let get = |name: &str| {
                biomes
                    .id(name)
                    .ok_or_else(|| DecodeError::Registry(format!("Missing biome {name}")))
            };
            BiomeSource::End(EndBiomes {
                center: get("minecraft:the_end")?,
                highlands: get("minecraft:end_highlands")?,
                midlands: get("minecraft:end_midlands")?,
                islands: get("minecraft:small_end_islands")?,
                barrens: get("minecraft:end_barrens")?,
            })
        }
        "minecraft:multi_noise" => {
            // `Codec.mapEither(DIRECT_CODEC, PRESET_CODEC)`: the inline list when it decodes,
            // else the preset.
            let inline = optional(&fields, "biomes").map(|raw| decode_parameter_list(raw, biomes));
            match inline {
                Some(Ok(entries)) => BiomeSource::MultiNoise(
                    ClimateIndex::new(entries).map_err(|e| DecodeError::Registry(e.to_string()))?,
                ),
                _ => BiomeSource::MultiNoise(
                    presets.decode_holder(required(&fields, "preset")?)?.clone(),
                ),
            }
        }
        other => {
            return Err(DecodeError::Registry(format!(
                "Unknown registry key in minecraft:worldgen/biome_source: {other}"
            )));
        }
    })
}

/// A level stem's `minecraft:noise` generator: its noise settings and biome source.
#[derive(Clone, Debug)]
pub struct NoiseGeneratorStem {
    /// The `worldgen/noise_settings` element.
    pub settings: String,
    pub biome_source: BiomeSource,
}

/// A level stem's `generator` (JSON). Only `minecraft:noise` so far; `minecraft:flat` and
/// `minecraft:debug` come with their generators.
pub fn decode_noise_generator(
    raw: &RawValue,
    biomes: &BiomeIds<'_>,
    presets: &BiomePresets,
) -> Result<NoiseGeneratorStem, DecodeError> {
    let fields = object(raw)?;
    let kind = identifier(&string(required(&fields, "type")?)?)?;
    if kind != "minecraft:noise" {
        return Err(DecodeError::Registry(format!(
            "the {kind} generator is not ported yet"
        )));
    }
    let settings = required(&fields, "settings")?;
    if !settings.get().starts_with('"') {
        return Err(DecodeError::Field(
            "inline noise settings are not supported",
        ));
    }
    Ok(NoiseGeneratorStem {
        settings: identifier(&string(settings)?)?,
        biome_source: decode_biome_source(required(&fields, "biome_source")?, biomes, presets)?,
    })
}

/// [`decode_noise_generator`] for `level.dat`'s NBT form (`WorldGenSettings.dimensions.<stem>.
/// generator`), through `NbtOps` -> `JsonOps`.
pub fn decode_noise_generator_nbt(
    generator: &Tag,
    biomes: &BiomeIds<'_>,
    presets: &BiomePresets,
) -> Result<NoiseGeneratorStem, DecodeError> {
    let json = crate::registry::nbt_json::nbt_to_json(generator);
    let raw = serde_json::value::to_raw_value(&json)?;
    decode_noise_generator(&raw, biomes, presets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::JavaString;
    use crate::registry::Registries;
    use crate::registry::tests::pack;

    /// Biomes in id order: desert 0, end_barrens 1, end_highlands 2, end_midlands 3, plains 4,
    /// small_end_islands 5, the_end 6; `#minecraft:warm` = plains, desert.
    fn registries() -> Registries {
        let names = [
            "plains",
            "desert",
            "the_end",
            "end_highlands",
            "end_midlands",
            "small_end_islands",
            "end_barrens",
        ];
        let paths: Vec<String> = names
            .iter()
            .map(|n| format!("minecraft/worldgen/biome/{n}.json"))
            .collect();
        let mut files: Vec<(&str, &str)> = paths.iter().map(|p| (p.as_str(), "{}")).collect();
        files.push((
            "minecraft/tags/worldgen/biome/warm.json",
            r#"{"values": ["plains", "desert"]}"#,
        ));
        Registries::build(Vec::new(), &[pack("vanilla", None, &files)]).unwrap()
    }

    const POINT: &str = r#"{"temperature":0.5,"humidity":0,"continentalness":0,"erosion":0,
        "depth":0,"weirdness":0,"offset":0}"#;

    fn presets(biomes: &BiomeIds<'_>) -> BiomePresets {
        let report =
            |biome: &str| format!(r#"{{"biomes":[{{"biome":"{biome}","parameters":{POINT}}}]}}"#);
        BiomePresets::build(
            |name| {
                Ok(report(if name == "minecraft:overworld" {
                    "minecraft:plains"
                } else {
                    "minecraft:desert"
                }))
            },
            vec![(
                "minecraft:nether".into(),
                serde_json::from_str(r#"{"preset": "minecraft:nether"}"#).unwrap(),
            )],
            biomes,
        )
        .unwrap()
    }

    fn decode(json: &str) -> Result<BiomeSource, DecodeError> {
        let registries = registries();
        let biomes = BiomeIds::new(registries.synced("minecraft:worldgen/biome").unwrap());
        let raw: Box<RawValue> = serde_json::from_str(json).unwrap();
        decode_biome_source(&raw, &biomes, &presets(&biomes))
    }

    fn multi_noise_biomes(source: BiomeSource) -> Vec<u32> {
        match source {
            BiomeSource::MultiNoise(index) => index.entries().iter().map(|e| e.1).collect(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn biome_sources_decode() {
        assert!(matches!(
            decode(r#"{"type":"fixed","biome":"plains"}"#),
            Ok(BiomeSource::Fixed(4))
        ));
        assert!(matches!(
            decode(r##"{"type":"minecraft:checkerboard","biomes":"#minecraft:warm"}"##),
            Ok(BiomeSource::Checkerboard { biomes, scale: 2 }) if biomes == [4, 0]
        ));
        assert!(matches!(
            decode(r#"{"type":"minecraft:checkerboard","biomes":["desert"],"scale":62}"#),
            Ok(BiomeSource::Checkerboard { biomes, scale: 62 }) if biomes == [0]
        ));
        assert!(
            decode(r#"{"type":"minecraft:checkerboard","biomes":"plains","scale":63}"#).is_err()
        );
        assert!(matches!(
            decode(r#"{"type":"minecraft:the_end"}"#),
            Ok(BiomeSource::End(EndBiomes {
                center: 6,
                highlands: 2,
                midlands: 3,
                islands: 5,
                barrens: 1
            }))
        ));
        let inline = format!(
            r#"{{"type":"minecraft:multi_noise","biomes":[{{"biome":"desert","parameters":{POINT}}}]}}"#
        );
        assert_eq!(multi_noise_biomes(decode(&inline).unwrap()), [0]);
        let by_element = r#"{"type":"minecraft:multi_noise","preset":"minecraft:nether"}"#;
        assert_eq!(multi_noise_biomes(decode(by_element).unwrap()), [0]);
        let direct = r#"{"type":"minecraft:multi_noise","preset":{"preset":"overworld"}}"#;
        assert_eq!(multi_noise_biomes(decode(direct).unwrap()), [4]);
        // A list that fails falls back to the preset (`mapEither`).
        let fallback =
            r#"{"type":"minecraft:multi_noise","biomes":[],"preset":"minecraft:nether"}"#;
        assert_eq!(multi_noise_biomes(decode(fallback).unwrap()), [0]);
        // Only `minecraft:nether` is a registry element here.
        assert!(
            decode(r#"{"type":"minecraft:multi_noise","preset":"minecraft:overworld"}"#).is_err()
        );
        assert!(decode(r#"{"type":"fixed","biome":"ocean"}"#).is_err());
        assert!(decode(r#"{"type":"minecraft:spiral"}"#).is_err());
    }

    #[test]
    fn level_dat_stems_decode_from_nbt() {
        let registries = registries();
        let biomes = BiomeIds::new(registries.synced("minecraft:worldgen/biome").unwrap());
        let presets = presets(&biomes);
        let float = |v: f32| Tag::Float(v.to_bits());
        let string = |s: &str| Tag::String(JavaString::from(s));
        let mut parameters: Vec<(&str, Tag)> = crate::worldgen::climate::COORDINATES
            .iter()
            .map(|name| (*name, Tag::List(vec![float(-0.3), float(0.7)])))
            .collect();
        parameters.push(("offset", float(0.1)));
        let generator = Tag::compound([
            ("type", string("minecraft:noise")),
            ("settings", string("minecraft:overworld")),
            (
                "biome_source",
                Tag::compound([
                    ("type", string("minecraft:multi_noise")),
                    (
                        "biomes",
                        Tag::List(vec![Tag::compound([
                            ("biome", string("minecraft:plains")),
                            ("parameters", Tag::compound(parameters)),
                        ])]),
                    ),
                ]),
            ),
        ]);
        let stem = decode_noise_generator_nbt(&generator, &biomes, &presets).unwrap();
        assert_eq!(stem.settings, "minecraft:overworld");
        let BiomeSource::MultiNoise(index) = stem.biome_source else {
            panic!("multi-noise");
        };
        let (point, biome) = index.entries()[0];
        assert_eq!(biome, 4);
        // The floats come back exactly, quantized as `(long) (f * 10000f)`.
        let q = crate::worldgen::climate::quantize;
        assert_eq!(point.coordinates[0].min, q(-0.3));
        assert_eq!(point.coordinates[5].max, q(0.7));
        assert_eq!(point.offset, q(0.1));
        let flat = Tag::compound([("type", string("minecraft:flat"))]);
        assert!(decode_noise_generator_nbt(&flat, &biomes, &presets).is_err());
    }
}
