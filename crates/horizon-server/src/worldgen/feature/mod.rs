//! The feature registries: `worldgen/feature` (the configured features: their type and JSON, which the
//! feature implementations decode), `worldgen/placed_feature` (a configured feature with its
//! placement modifiers) and each biome's `features` per decoration step, with the tags of
//! `tags/worldgen/placed_feature`. Everything is decoded when the datapacks load; ids are
//! positions in registration order.
//!
//! [`sort`] orders the features of a set of biomes per step (`FeatureSorter`).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::value::RawValue;

use crate::block::BlockStates;
use crate::core::position::BlockPos;
use crate::registry::{DataPack, ElementFile, for_each_element, load_tags, registry_dir};
use crate::util::random::{LegacyRandom, RandomSource};
use crate::worldgen::codec::{DecodeError, object, required, string};
use crate::worldgen::data::WorldgenError;
use crate::worldgen::noise::SimplexNoise;
use crate::worldgen::placement::{
    DecodeContext, FLUIDS, FeaturePlacer, PlaceError, PlacementError, PlacementModifier,
};
use crate::worldgen::state_provider::{NamedProviders, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;
use crate::worldgen::surface::temperature::{BiomeClimate, BiomeClimates};
use tree::TreeBlocksCell;

pub mod cave;
mod common;
pub mod decorate;
#[cfg(test)]
mod decorate_oracle_tests;
pub mod ice;
pub mod lake;
pub mod misc;
pub mod multiface;
pub mod mushroom;
#[cfg(test)]
mod oracle_tests;
pub mod ore;
pub mod patch;
pub mod rule_test;
pub mod select;
pub mod shape;
pub mod simple;
pub mod sort;
pub mod terrain;
pub mod tree;
pub use simple::{Feature, FeatureWorld};

/// A `worldgen/feature` element's (a configured feature's) index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfiguredFeatureId(pub u32);

/// A `worldgen/placed_feature` element's index. Features are told apart by it, as vanilla tells
/// holders apart by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlacedFeatureId(pub u32);

/// A `Feature` (26.3 registers them under `worldgen/feature`, what earlier versions called
/// configured features): its type and its JSON, in which the type's own fields sit beside `type`.
#[derive(Debug)]
pub struct ConfiguredFeature {
    pub id: String,
    /// The `Feature` type (`minecraft:tree`, `minecraft:ore`...).
    pub kind: String,
    /// The whole element, for the feature's implementation to decode.
    pub config: Box<RawValue>,
    /// The decoded feature, for the types [`simple`] implements.
    pub feature: Option<Feature>,
}

/// `PlacedFeature`.
#[derive(Debug)]
pub struct PlacedFeature {
    pub id: String,
    pub feature: ConfiguredFeatureId,
    pub placement: Vec<PlacementModifier>,
}

/// A biome's `BiomeGenerationSettings` features.
#[derive(Debug, Default)]
pub struct BiomeFeatures {
    /// The placed features of each decoration step, in order.
    pub steps: Vec<Vec<PlacedFeatureId>>,
    /// Every feature of every step (`hasFeature`).
    all: BiomeSet,
}

impl BiomeFeatures {
    fn new(steps: Vec<Vec<PlacedFeatureId>>) -> Self {
        let all = BiomeSet::new(steps.iter().flatten().map(|f| f.0).collect());
        Self { steps, all }
    }
}

/// The feature registries of a set of datapacks. Immutable once loaded.
#[derive(Debug)]
pub struct FeatureRegistry {
    configured: Vec<ConfiguredFeature>,
    placed: Vec<PlacedFeature>,
    /// `tags/worldgen/placed_feature`, members in tag order.
    placed_tags: Vec<(String, Vec<PlacedFeatureId>)>,
    /// By `worldgen/biome` id.
    biomes: Vec<BiomeFeatures>,
    /// `Biome.BIOME_INFO_NOISE`, which `noise_based_count` and `noise_threshold_count` read.
    info_noise: SimplexNoise,
    /// Every biome's temperature (`Biome.getTemperature`), which freezing and snow read.
    climates: BiomeClimates,
    /// `Biome.hasPrecipitation`, by biome id.
    precipitation: Vec<bool>,
}

/// Why a feature could not be placed.
fn placement_error(error: PlacementError) -> ProviderError {
    match error {
        PlacementError::Random(e) => ProviderError::Random(e),
        PlacementError::World(e) => ProviderError::World(e),
        PlacementError::NoTopFeature => ProviderError::Invalid("a placed feature without a biome"),
    }
}

impl FeatureRegistry {
    pub fn configured(&self, id: ConfiguredFeatureId) -> &ConfiguredFeature {
        &self.configured[id.0 as usize]
    }
    pub fn placed(&self, id: PlacedFeatureId) -> &PlacedFeature {
        &self.placed[id.0 as usize]
    }
    pub fn configured_len(&self) -> usize {
        self.configured.len()
    }
    pub fn placed_len(&self) -> usize {
        self.placed.len()
    }
    pub fn configured_id(&self, name: &str) -> Option<ConfiguredFeatureId> {
        let index = self.configured.iter().position(|f| f.id == name)?;
        Some(ConfiguredFeatureId(index as u32))
    }
    pub fn placed_id(&self, name: &str) -> Option<PlacedFeatureId> {
        let index = self.placed.iter().position(|f| f.id == name)?;
        Some(PlacedFeatureId(index as u32))
    }
    pub fn placed_tag(&self, tag: &str) -> Option<&[PlacedFeatureId]> {
        (self.placed_tags.iter())
            .find(|(t, _)| t == tag)
            .map(|(_, members)| members.as_slice())
    }
    /// The biome's features; biomes are `worldgen/biome` ids.
    pub fn biome(&self, biome: u32) -> &BiomeFeatures {
        &self.biomes[biome as usize]
    }
    pub fn biome_len(&self) -> usize {
        self.biomes.len()
    }
    /// `BiomeGenerationSettings.hasFeature`.
    pub fn biome_has_feature(&self, biome: u32, feature: PlacedFeatureId) -> bool {
        self.biome(biome).all.contains(feature.0)
    }
    pub fn info_noise(&self) -> &SimplexNoise {
        &self.info_noise
    }
    pub fn climates(&self) -> &BiomeClimates {
        &self.climates
    }
    /// `Biome.hasPrecipitation`.
    pub fn has_precipitation(&self, biome: u32) -> bool {
        self.precipitation[biome as usize]
    }

    /// `Feature.place` of a configured feature: false for one whose type is not ported yet.
    pub fn place_configured<W: FeatureWorld, R: RandomSource>(
        &self,
        id: ConfiguredFeatureId,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match &self.configured(id).feature {
            Some(feature) => feature.place(self, world, random, origin),
            None => Err(ProviderError::Invalid("the feature type is not ported")),
        }
    }

    /// `PlacedFeature.place(level, generator, random, origin)`: its placement chain, without a
    /// biome check (what the selector features call).
    pub fn place_placed<W: FeatureWorld, R: RandomSource>(
        &self,
        id: PlacedFeatureId,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        self.place_placed_checked(id, false, world, random, origin)
    }

    /// `FeaturePlacer.place` (`biome_check` false) or `placeWithBiomeCheck`.
    pub fn place_placed_checked<W: FeatureWorld, R: RandomSource>(
        &self,
        id: PlacedFeatureId,
        biome_check: bool,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let configured = self.placed(id).feature;
        FeaturePlacer::default()
            .place(self, id, biome_check, world, random, origin, |w, r, at| {
                self.place_configured(configured, w, r, at)
            })
            .map_err(|e| match e {
                PlaceError::Placement(e) => placement_error(e),
                PlaceError::Feature(e) => e,
            })
    }

    /// Decodes the feature registries of `packs` (bottom to top).
    pub fn load(packs: &[DataPack], blocks: &BlockStates) -> Result<Self, WorldgenError> {
        let error = |file: &ElementFile<'_>, source: DecodeError| WorldgenError {
            file: file.file.to_owned(),
            pack: file.pack.id.clone(),
            source,
        };
        let parse = |file: &ElementFile<'_>| {
            serde_json::from_slice::<Box<RawValue>>(file.bytes).map_err(|e| error(file, e.into()))
        };

        let mut biome_ids = Vec::new();
        for_each_element(packs, "minecraft:worldgen/biome", |file| {
            biome_ids.push(file.id.to_owned());
            Ok::<_, WorldgenError>(())
        })?;
        let biome_tags = load_tags(packs, &registry_dir("minecraft:worldgen/biome"), |id| {
            biome_ids.iter().position(|b| b == id).map(|i| i as i32)
        });
        let block_tags = load_tags(packs, &registry_dir("minecraft:block"), |id| {
            blocks.block_by_name(id).map(|b| b.0 as i32)
        });
        let fluid_tags = load_tags(packs, &registry_dir("minecraft:fluid"), |id| {
            FLUIDS.iter().position(|f| *f == id).map(|i| i as i32)
        });
        let names = DecodeContext {
            blocks,
            block_tags: &block_tags,
            biomes: &biome_ids,
            biome_tags: &biome_tags,
            fluid_tags: &fluid_tags,
        };

        let trees = Arc::new(TreeBlocksCell::new(&block_tags));
        let mut named = Vec::new();
        for_each_element(packs, "minecraft:worldgen/block_state_provider", |file| {
            named.push((file.id.to_owned(), parse(&file)?));
            Ok(())
        })?;
        let providers = ProviderContext {
            names: &names,
            named: NamedProviders(&named),
        };
        let mut configured = Vec::new();
        let mut configured_ids = HashMap::new();
        for_each_element(packs, "minecraft:worldgen/feature", |file| {
            let feature = decode_configured(file.id, &parse(&file)?, &providers, &trees)
                .map_err(|e| error(&file, e))?;
            configured_ids.insert(
                file.id.to_owned(),
                ConfiguredFeatureId(configured.len() as u32),
            );
            configured.push(feature);
            Ok(())
        })?;

        let mut placed = Vec::new();
        let mut placed_ids = HashMap::new();
        for_each_element(packs, "minecraft:worldgen/placed_feature", |file| {
            let feature = decode_placed(
                file.id,
                &parse(&file)?,
                &configured_ids,
                &mut configured,
                &names,
                &providers,
                &trees,
            )
            .map_err(|e| error(&file, e))?;
            placed_ids.insert(file.id.to_owned(), PlacedFeatureId(placed.len() as u32));
            placed.push(feature);
            Ok(())
        })?;
        let placed_tags = load_tags(
            packs,
            &registry_dir("minecraft:worldgen/placed_feature"),
            |id| placed_ids.get(id).map(|p| p.0 as i32),
        )
        .into_iter()
        .map(|(tag, ids)| {
            (
                tag,
                ids.into_iter().map(|i| PlacedFeatureId(i as u32)).collect(),
            )
        })
        .collect::<Vec<_>>();

        let mut biomes = Vec::new();
        let mut climates = Vec::new();
        let mut precipitation = Vec::new();
        for_each_element(packs, "minecraft:worldgen/biome", |file| {
            let raw = parse(&file)?;
            climates.push(BiomeClimate::decode(&raw).map_err(|e| error(&file, e))?);
            precipitation.push(has_precipitation(&raw).map_err(|e| error(&file, e))?);
            let mut lookup = PlacedLookup {
                ids: &placed_ids,
                tags: &placed_tags,
                owner: file.id,
                configured_ids: &configured_ids,
                names: &names,
                providers: &providers,
                trees: &trees,
            };
            let steps = decode_biome_features(&raw, &mut lookup, &mut placed, &mut configured)
                .map_err(|e| error(&file, e))?;
            biomes.push(BiomeFeatures::new(steps));
            Ok::<_, WorldgenError>(())
        })?;

        // The placed features that configured features name (selectors) and the pale moss patch.
        let mut index = 0;
        let pale_moss = configured_ids.get(tree::PALE_MOSS_PATCH).copied();
        while index < configured.len() {
            let Some(mut feature) = configured[index].feature.take() else {
                index += 1;
                continue;
            };
            let owner = configured[index].id.clone();
            let mut resolve = || -> Result<(), DecodeError> {
                let mut lookup = PlacedLookup {
                    ids: &placed_ids,
                    tags: &placed_tags,
                    owner: &owner,
                    configured_ids: &configured_ids,
                    names: &names,
                    providers: &providers,
                    trees: &trees,
                };
                match &mut feature {
                    Feature::Select(selector) => {
                        for refs in selector.refs_mut() {
                            if let Some(raw) = refs.take_pending() {
                                let ids = resolve_entries(
                                    &raw,
                                    &mut lookup,
                                    &mut placed,
                                    &mut configured,
                                )?;
                                refs.resolve(ids);
                            }
                        }
                    }
                    Feature::Patch(patch) => {
                        let refs = patch.refs_mut();
                        if let Some(raw) = refs.take_pending() {
                            let ids =
                                resolve_entries(&raw, &mut lookup, &mut placed, &mut configured)?;
                            refs.resolve(ids);
                        }
                    }
                    Feature::Tree(tree) => {
                        if let Some(patch) = pale_moss {
                            tree.set_pale_moss_patch(patch);
                        }
                    }
                    Feature::FallenTree(tree) => {
                        if let Some(patch) = pale_moss {
                            tree.set_pale_moss_patch(patch);
                        }
                    }
                    _ => {}
                }
                Ok(())
            };
            resolve().map_err(|source| WorldgenError {
                file: owner.clone(),
                pack: "<registry>".into(),
                source,
            })?;
            configured[index].feature = Some(feature);
            index += 1;
        }

        Ok(Self {
            configured,
            placed,
            placed_tags,
            biomes,
            info_noise: SimplexNoise::with_skipped_offsets(&mut LegacyRandom::new(2345), true),
            climates: BiomeClimates::new(climates),
            precipitation,
        })
    }
}

/// `Biome.ClimateSettings.hasPrecipitation`.
fn has_precipitation(raw: &RawValue) -> Result<bool, DecodeError> {
    crate::worldgen::codec::boolean(required(&object(raw)?, "has_precipitation")?)
}

/// `Feature.DIRECT_CODEC`: dispatched on `type`; the rest is the feature's own to decode.
fn decode_configured(
    id: &str,
    raw: &RawValue,
    ctx: &ProviderContext<'_>,
    trees: &Arc<TreeBlocksCell>,
) -> Result<ConfiguredFeature, DecodeError> {
    let fields = object(raw)?;
    let kind = crate::worldgen::codec::identifier(&string(required(&fields, "type")?)?)?;
    Ok(ConfiguredFeature {
        id: id.to_owned(),
        feature: Feature::decode(&kind, raw, ctx, trees)?,
        kind,
        config: raw.to_owned(),
    })
}

/// `PlacedFeature.DIRECT_CODEC`. The feature is a registered id or the configured feature
/// itself, which is then added to the registry as `<id>#feature`.
fn decode_placed(
    id: &str,
    raw: &RawValue,
    configured_ids: &HashMap<String, ConfiguredFeatureId>,
    configured: &mut Vec<ConfiguredFeature>,
    names: &DecodeContext<'_>,
    providers: &ProviderContext<'_>,
    trees: &Arc<TreeBlocksCell>,
) -> Result<PlacedFeature, DecodeError> {
    let fields = object(raw)?;
    let reference = required(&fields, "feature")?;
    let feature = if reference.get().starts_with('{') {
        configured.push(decode_configured(
            &format!("{id}#feature"),
            reference,
            providers,
            trees,
        )?);
        ConfiguredFeatureId(configured.len() as u32 - 1)
    } else {
        let name = crate::worldgen::codec::identifier(&string(reference)?)?;
        *configured_ids
            .get(&name)
            .ok_or_else(|| DecodeError::Registry(format!("Missing configured feature: {name}")))?
    };
    let modifiers: Vec<Box<RawValue>> =
        serde_json::from_str(required(&fields, "placement")?.get())?;
    Ok(PlacedFeature {
        id: id.to_owned(),
        feature,
        placement: modifiers
            .iter()
            .map(|m| PlacementModifier::decode(m, names))
            .collect::<Result<_, _>>()?,
    })
}

/// What a biome's feature lists resolve placed features against.
struct PlacedLookup<'a> {
    ids: &'a HashMap<String, PlacedFeatureId>,
    tags: &'a [(String, Vec<PlacedFeatureId>)],
    /// The biome, which names the inline placed features it holds.
    owner: &'a str,
    configured_ids: &'a HashMap<String, ConfiguredFeatureId>,
    names: &'a DecodeContext<'a>,
    providers: &'a ProviderContext<'a>,
    trees: &'a Arc<TreeBlocksCell>,
}

/// `BiomeGenerationSettings.CODEC`'s `features`: per step a `HolderSet<PlacedFeature>` (an id,
/// `#tag`, or a list of ids and inline placed features). Inline ones are added to the registry.
fn decode_biome_features(
    raw: &RawValue,
    lookup: &mut PlacedLookup<'_>,
    placed: &mut Vec<PlacedFeature>,
    configured: &mut Vec<ConfiguredFeature>,
) -> Result<Vec<Vec<PlacedFeatureId>>, DecodeError> {
    let fields = object(raw)?;
    let steps: Vec<Box<RawValue>> = serde_json::from_str(required(&fields, "features")?.get())?;
    let mut result = Vec::with_capacity(steps.len());
    for step in &steps {
        let features = resolve_entries(step, lookup, placed, configured)?;
        result.push(features);
    }
    Ok(result)
}

/// A `HolderSet<PlacedFeature>` (an id, `#tag`, or a list of ids and inline placed features) as
/// ids. Inline placed features are added to the registry.
fn resolve_entries(
    raw: &RawValue,
    lookup: &mut PlacedLookup<'_>,
    placed: &mut Vec<PlacedFeature>,
    configured: &mut Vec<ConfiguredFeature>,
) -> Result<Vec<PlacedFeatureId>, DecodeError> {
    let entries: Vec<Box<RawValue>> = if raw.get().starts_with('[') {
        serde_json::from_str(raw.get())?
    } else {
        vec![raw.to_owned()]
    };
    let mut features = Vec::new();
    for entry in &entries {
        if entry.get().starts_with('{') {
            let id = format!("{}#placed{}", lookup.owner, placed.len());
            placed.push(decode_placed(
                &id,
                entry,
                lookup.configured_ids,
                configured,
                lookup.names,
                lookup.providers,
                lookup.trees,
            )?);
            features.push(PlacedFeatureId(placed.len() as u32 - 1));
            continue;
        }
        let name = string(entry)?;
        match name.strip_prefix('#') {
            Some(tag) => {
                let tag = crate::worldgen::codec::identifier(tag)?;
                let (_, members) =
                    lookup.tags.iter().find(|(t, _)| *t == tag).ok_or_else(|| {
                        DecodeError::Registry(format!(
                            "Missing tag: #{tag} in worldgen/placed_feature"
                        ))
                    })?;
                features.extend_from_slice(members);
            }
            None => {
                let name = crate::worldgen::codec::identifier(&name)?;
                features.push(*lookup.ids.get(&name).ok_or_else(|| {
                    DecodeError::Registry(format!("Missing placed feature: {name}"))
                })?);
            }
        }
    }
    Ok(features)
}
