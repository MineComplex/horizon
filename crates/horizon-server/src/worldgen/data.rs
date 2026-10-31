//! The worldgen registries of the server's datapacks, decoded once at startup (and `/reload`)
//! into immutable tables, as `RegistryDataLoader` loads `WORLDGEN_REGISTRIES`: every element is
//! decoded before the server starts, an invalid one stops it. Seed-independent; a level compiles
//! what it needs from it ([`WorldgenData::compile_level`]).
//!
//! Decoded so far: `worldgen/noise`, `worldgen/density_function` and `worldgen/noise_settings`
//! (into one [`DensityGraph`]), `worldgen/material_rule` and `material_condition` (the surface
//! rules, [`MaterialRules`], with each biome's climate), and `worldgen/structure` and `worldgen/structure_set` with `tags/worldgen/structure`
//! ([`StructureRegistry`]), `worldgen/processor_list` ([`ProcessorLists`]), `worldgen/carver` with each biome's carvers ([`CarverRegistry`]). The other worldgen registries are kept as parsed JSON
//! ([`RawRegistry`]) for the packages that decode them (presets).

use serde_json::value::RawValue;

use super::aquifer::AquiferRandom;
use super::carver::CarverRegistry;
use super::climate::Parameter;
use super::codec::{DecodeError, DensityLoader, decode_noise};
use super::density::{
    DensityCompiler, DensityGraph, DensityProgram, FunctionId, NodeId, ProgramError,
};
use super::feature::FeatureRegistry;
use super::placement::DecodeContext;
use super::settings::{AquiferField, NoiseGeneratorSettings, RouterField};
use super::structure::jigsaw::JigsawStructure;
use super::structure::pool::{PoolContext, TemplatePools};
use super::structure::processor::ProcessorLists;
use super::structure::template::StructureTemplates;
use super::structure::{
    BiomeLookup, InlineElements, StructureId, StructureRegistry, decode_structure,
    decode_structure_set,
};
use super::surface::rules::MaterialRules;
use super::surface::temperature::{BiomeClimate, BiomeClimates};
use crate::block::BlockStates;
use crate::registry::{DataPack, ElementFile, WORLDGEN, for_each_element, load_tags, registry_dir};

/// An element that failed to decode: `RegistryDataLoader`'s message, which stops the server.
#[derive(Debug, thiserror::Error)]
#[error("Failed to parse {file} from pack {pack}: {source}")]
pub struct WorldgenError {
    pub file: String,
    pub pack: String,
    #[source]
    pub source: DecodeError,
}

/// A registry not decoded yet: its elements' JSON in registration order.
#[derive(Debug)]
pub struct RawRegistry {
    pub name: &'static str,
    pub elements: Vec<(String, Box<RawValue>)>,
}

/// The worldgen registries (see the module docs). Immutable once loaded; the server shares it
/// read-only (one `Arc`) between level creation and `/reload`.
#[derive(Debug)]
pub struct WorldgenData {
    graph: DensityGraph,
    /// `worldgen/noise_settings` in registration order.
    noise_settings: Vec<(String, NoiseGeneratorSettings)>,
    /// `worldgen/material_rule` and `material_condition`, with Paper's bedrock condition.
    surface: MaterialRules,
    /// Each biome's climate (by registry id) and the fixed temperature noises.
    climates: BiomeClimates,
    structures: StructureRegistry,
    /// `worldgen/processor_list` and the built-in processors' blocks.
    processor_lists: ProcessorLists,
    features: FeatureRegistry,
    /// `worldgen/carver` and each biome's carvers.
    carvers: CarverRegistry,
    raw: Vec<RawRegistry>,
}

/// A level's compiled density functions: the program and its roots.
#[derive(Clone, Debug)]
pub struct LevelDensity {
    pub program: DensityProgram,
    /// By [`RouterField`].
    pub router: [NodeId; 8],
    /// By [`AquiferField`].
    pub aquifers: Option<[NodeId; 6]>,
    /// `getOrCreateRandomFactory("minecraft:aquifer")` of the level.
    pub aquifer_random: AquiferRandom,
    /// The settings' `spawn_target` points with their functions compiled (what
    /// `NoiseSpawnFinder` samples through `RandomState.samplersWithContext`).
    pub spawn_target: Vec<Vec<(NodeId, Parameter)>>,
    /// The density functions of the settings' surface rule (its ore veins') compiled into the
    /// program.
    pub surface_functions: Vec<(FunctionId, NodeId)>,
}

impl LevelDensity {
    pub fn router(&self, field: RouterField) -> NodeId {
        self.router[field as usize]
    }
    pub fn aquifer(&self, field: AquiferField) -> Option<NodeId> {
        self.aquifers.map(|roots| roots[field as usize])
    }
    /// A function of the surface rule's ore veins, compiled with the level.
    pub fn surface_node(&self, function: FunctionId) -> NodeId {
        self.surface_functions
            .iter()
            .find(|(id, _)| *id == function)
            .expect("the level compiles every function of its surface rule")
            .1
    }
}

impl WorldgenData {
    /// Decodes the worldgen registries of `packs` (bottom to top).
    pub fn load(packs: &[DataPack], blocks: &BlockStates) -> Result<Self, WorldgenError> {
        let mut graph = DensityGraph::new();
        let error = |file: &crate::registry::ElementFile<'_>, source: DecodeError| WorldgenError {
            file: file.file.to_owned(),
            pack: file.pack.id.clone(),
            source,
        };
        let parse = |file: &crate::registry::ElementFile<'_>| {
            serde_json::from_slice::<Box<RawValue>>(file.bytes).map_err(|e| error(file, e.into()))
        };
        for_each_element(packs, "minecraft:worldgen/noise", |file| {
            let noise = decode_noise(&parse(&file)?).map_err(|e| error(&file, e))?;
            graph
                .register_noise(file.id.to_owned(), noise)
                .map_err(|e| error(&file, e.into()))?;
            Ok(())
        })?;
        let mut densities = Vec::new();
        let mut first_density = None;
        for_each_element(packs, "minecraft:worldgen/density_function", |file| {
            first_density.get_or_insert_with(|| (file.file.to_owned(), file.pack.id.clone()));
            densities.push((file.id.to_owned(), parse(&file)?));
            Ok(())
        })?;
        let mut loader = DensityLoader::new(&mut graph);
        // References resolve across the registry, so a failure is the registry's.
        loader.load_registry(densities).map_err(|source| {
            let (file, pack) = first_density.clone().unwrap_or_default();
            WorldgenError { file, pack, source }
        })?;
        let mut noise_settings = Vec::new();
        for_each_element(packs, "minecraft:worldgen/noise_settings", |file| {
            let settings = NoiseGeneratorSettings::decode(&parse(&file)?, &mut loader, blocks)
                .map_err(|e| error(&file, e))?;
            noise_settings.push((file.id.to_owned(), settings));
            Ok(())
        })?;
        let (biome_ids, biome_climates) = load_biomes(packs, &error, &parse)?;
        let biome_tags = load_tags(packs, &registry_dir("minecraft:worldgen/biome"), |id| {
            biome_ids.iter().position(|b| b == id).map(|i| i as i32)
        });
        let biomes = BiomeLookup {
            ids: &biome_ids,
            tags: &biome_tags,
        };
        let material = |registry| -> Result<Vec<(String, Box<RawValue>)>, WorldgenError> {
            let mut elements = Vec::new();
            for_each_element(packs, registry, |file| {
                elements.push((file.id.to_owned(), parse(&file)?));
                Ok(())
            })?;
            Ok(elements)
        };
        let surface = MaterialRules::decode(
            &material("minecraft:worldgen/material_rule")?,
            &material("minecraft:worldgen/material_condition")?,
            &biomes,
            &mut loader,
            blocks,
        )
        .map_err(|source| WorldgenError {
            file: "minecraft:worldgen/material_rule".into(),
            pack: "<registry>".into(),
            source,
        })?;
        let structures = load_structures(packs, &biomes, &error, &parse)?;
        let mut raw = Vec::new();
        for name in WORLDGEN {
            if matches!(
                name,
                "minecraft:worldgen/noise"
                    | "minecraft:worldgen/density_function"
                    | "minecraft:worldgen/noise_settings"
                    | "minecraft:worldgen/material_rule"
                    | "minecraft:worldgen/material_condition"
                    | "minecraft:worldgen/structure"
                    | "minecraft:worldgen/structure_set"
                    | "minecraft:worldgen/feature"
                    | "minecraft:worldgen/placed_feature"
                    | "minecraft:worldgen/carver"
                    | "minecraft:worldgen/processor_list"
                    | "minecraft:worldgen/template_pool"
            ) {
                continue;
            }
            let mut elements = Vec::new();
            for_each_element(packs, name, |file| {
                elements.push((file.id.to_owned(), parse(&file)?));
                Ok(())
            })?;
            raw.push(RawRegistry { name, elements });
        }
        let processor_lists = ProcessorLists::load(packs, blocks)?;
        let features = FeatureRegistry::load(packs, blocks)?;
        let mut structures = structures;
        load_pools(packs, blocks, &processor_lists, &features, &mut structures)?;
        Ok(Self {
            graph,
            noise_settings,
            surface,
            climates: BiomeClimates::new(biome_climates),
            structures,
            processor_lists,
            features,
            carvers: CarverRegistry::load(packs, blocks)?,
            raw,
        })
    }

    /// The configured and placed features, and each biome's features per step.
    pub fn features(&self) -> &FeatureRegistry {
        &self.features
    }

    /// The carvers and each biome's carver list.
    pub fn carvers(&self) -> &CarverRegistry {
        &self.carvers
    }

    pub fn graph(&self) -> &DensityGraph {
        &self.graph
    }

    /// `worldgen/processor_list`.
    pub fn processor_lists(&self) -> &ProcessorLists {
        &self.processor_lists
    }

    /// Gives the structures their templates (`StructureTemplateManager`, loaded from the jar and the
    /// world's `generated` folder) and fills the pools' `getMaxSize` from them.
    pub fn set_templates(&mut self, templates: StructureTemplates) {
        self.features
            .attach_templates(&templates, &self.processor_lists);
        self.structures.templates = templates;
        let structures = &mut self.structures;
        structures.pools.compute_max_sizes(&structures.templates);
    }

    /// `worldgen/structure`, `worldgen/structure_set`, their tags, the template pools and templates.
    pub fn structures(&self) -> &StructureRegistry {
        &self.structures
    }

    pub fn noise_settings(&self, id: &str) -> Option<&NoiseGeneratorSettings> {
        self.noise_settings
            .iter()
            .find(|(name, _)| name == id)
            .map(|(_, settings)| settings)
    }

    pub fn noise_settings_ids(&self) -> impl Iterator<Item = &str> {
        self.noise_settings.iter().map(|(name, _)| name.as_str())
    }

    /// The surface rules (`worldgen/material_rule`, `material_condition`).
    pub fn surface_rules(&self) -> &MaterialRules {
        &self.surface
    }

    /// Every biome's climate, for temperatures.
    pub fn climates(&self) -> &BiomeClimates {
        &self.climates
    }

    /// A registry kept as JSON.
    pub fn raw(&self, name: &str) -> Option<&RawRegistry> {
        self.raw.iter().find(|registry| registry.name == name)
    }

    /// `RandomState.create`'s density part for a level: the settings' router and aquifer roots
    /// compiled for `seed`.
    pub fn compile_level(
        &self,
        settings: &NoiseGeneratorSettings,
        seed: i64,
        end_void_rings: bool,
    ) -> Result<LevelDensity, ProgramError> {
        Ok(self
            .compile_roots(self.graph.clone(), settings, seed, end_void_rings, &[])?
            .0)
    }

    /// [`Self::compile_level`] with more density functions (JSON, as `DensityFunction.CODEC`
    /// reads them) compiled into the same program, as `RandomState.getSampler` does for functions
    /// other systems hold (surface rules, debug functions, tests).
    pub fn compile_level_with(
        &self,
        settings: &NoiseGeneratorSettings,
        seed: i64,
        end_void_rings: bool,
        extra: &[&RawValue],
    ) -> Result<(LevelDensity, Vec<NodeId>), WorldgenError> {
        let mut graph = self.graph.clone();
        let mut loader = DensityLoader::new(&mut graph);
        let error = |source| WorldgenError {
            file: "<density function>".into(),
            pack: "<none>".into(),
            source,
        };
        let roots = extra
            .iter()
            .map(|raw| loader.decode(raw, 0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        self.compile_roots(graph, settings, seed, end_void_rings, &roots)
            .map_err(|e| error(DecodeError::Registry(e.to_string())))
    }

    fn compile_roots(
        &self,
        graph: DensityGraph,
        settings: &NoiseGeneratorSettings,
        seed: i64,
        end_void_rings: bool,
        extra: &[FunctionId],
    ) -> Result<(LevelDensity, Vec<NodeId>), ProgramError> {
        let mut compiler =
            DensityCompiler::new(graph, seed, settings.legacy_random_source, end_void_rings)?;
        let mut compile = |roots: &[FunctionId]| {
            roots
                .iter()
                .map(|root| compiler.compile(*root))
                .collect::<Result<Vec<_>, _>>()
        };
        let router = compile(&settings.router)?;
        let aquifers = settings.aquifers.map(|roots| compile(&roots)).transpose()?;
        let spawn_target = settings
            .spawn_target
            .iter()
            .map(|point| {
                let functions: Vec<FunctionId> = point.parameters.iter().map(|p| p.0).collect();
                let nodes = compile(&functions)?;
                Ok(nodes
                    .into_iter()
                    .zip(point.parameters.iter().map(|p| p.1))
                    .collect())
            })
            .collect::<Result<Vec<_>, ProgramError>>()?;
        let extra = compile(extra)?;
        let surface_functions = match self.surface.element(&settings.material_rule) {
            Some(rule) => {
                let functions = self.surface.ore_functions(rule);
                let nodes = compile(&functions)?;
                functions.into_iter().zip(nodes).collect()
            }
            None => Vec::new(),
        };
        let density = LevelDensity {
            program: compiler.finish(),
            router: router.try_into().expect("eight router roots"),
            aquifers: aquifers.map(|nodes| nodes.try_into().expect("six aquifer roots")),
            aquifer_random: AquiferRandom::new(seed, settings.legacy_random_source),
            spawn_target,
            surface_functions,
        };
        Ok((density, extra))
    }
}

/// The `worldgen/biome` ids in registration order, with each biome's climate.
fn load_biomes(
    packs: &[DataPack],
    error: &impl Fn(&ElementFile<'_>, DecodeError) -> WorldgenError,
    parse: &impl Fn(&ElementFile<'_>) -> Result<Box<RawValue>, WorldgenError>,
) -> Result<(Vec<String>, Vec<BiomeClimate>), WorldgenError> {
    let (mut ids, mut climates) = (Vec::new(), Vec::new());
    for_each_element(packs, "minecraft:worldgen/biome", |file| {
        ids.push(file.id.to_owned());
        climates.push(BiomeClimate::decode(&parse(&file)?).map_err(|e| error(&file, e))?);
        Ok::<_, WorldgenError>(())
    })?;
    Ok((ids, climates))
}

/// `worldgen/structure` (biomes resolved through `worldgen/biome` and its tags), then
/// `worldgen/structure_set` (which may name sets registered after it), then the structure tags.
fn load_structures(
    packs: &[DataPack],
    biomes: &BiomeLookup<'_>,
    error: &impl Fn(&ElementFile<'_>, DecodeError) -> WorldgenError,
    parse: &impl Fn(&ElementFile<'_>) -> Result<Box<RawValue>, WorldgenError>,
) -> Result<StructureRegistry, WorldgenError> {
    let mut registry = StructureRegistry::default();
    let blocking = RawValue::from_string("\"#minecraft:mineshaft_blocking\"".to_owned())
        .expect("a JSON string");
    registry.mineshaft_blocking = biomes.decode(&blocking).unwrap_or_default();
    let surrounding =
        RawValue::from_string("\"#minecraft:required_ocean_monument_surrounding\"".to_owned())
            .expect("a JSON string");
    registry.monument_surrounding = biomes.decode(&surrounding).unwrap_or_default();
    for_each_element(packs, "minecraft:worldgen/structure", |file| {
        let structure = decode_structure(file.id, parse(&file)?, biomes);
        registry
            .structures
            .push(structure.map_err(|e| error(&file, e))?);
        Ok(())
    })?;
    let mut sets = Vec::new();
    for_each_element(packs, "minecraft:worldgen/structure_set", |file| {
        sets.push((
            file.id.to_owned(),
            parse(&file)?,
            file.file.to_owned(),
            file.pack.id.clone(),
        ));
        Ok::<_, WorldgenError>(())
    })?;
    let set_ids: Vec<String> = sets.iter().map(|(id, ..)| id.clone()).collect();
    let mut inline = InlineElements::default();
    for (id, raw, file, pack) in sets {
        let set = decode_structure_set(
            &id,
            &raw,
            &registry.structures,
            &set_ids,
            biomes,
            &mut inline,
        )
        .map_err(|source| WorldgenError { file, pack, source })?;
        registry.sets.push(set);
    }
    registry.structures.extend(inline.structures);
    registry.sets.extend(inline.sets);
    let structure_ids = |id: &str| {
        let index = registry.structures.iter().position(|s| s.id == id)?;
        Some(index as i32)
    };
    let tags = load_tags(
        packs,
        &registry_dir("minecraft:worldgen/structure"),
        structure_ids,
    );
    registry.tags = tags
        .into_iter()
        .map(|(tag, ids)| {
            (
                tag,
                ids.into_iter().map(|i| StructureId(i as u32)).collect(),
            )
        })
        .collect();
    Ok(registry)
}

/// `worldgen/template_pool` (all pools first: fallbacks name any of them), then each jigsaw
/// structure's own fields, which name pools.
fn load_pools(
    packs: &[DataPack],
    blocks: &BlockStates,
    processors: &ProcessorLists,
    features: &FeatureRegistry,
    structures: &mut StructureRegistry,
) -> Result<(), WorldgenError> {
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
    let cx = PoolContext {
        names: &names,
        processors,
        features,
    };
    let mut elements = Vec::new();
    let mut files = Vec::new();
    for_each_element(packs, "minecraft:worldgen/template_pool", |file| {
        let raw =
            serde_json::from_slice::<Box<RawValue>>(file.bytes).map_err(|e| WorldgenError {
                file: file.file.to_owned(),
                pack: file.pack.id.clone(),
                source: e.into(),
            })?;
        elements.push((file.id.to_owned(), raw));
        files.push((file.file.to_owned(), file.pack.id.clone()));
        Ok::<_, WorldgenError>(())
    })?;
    let mut pools = TemplatePools::decode(&elements, &cx).map_err(|(i, source)| {
        let (file, pack) = files[i].clone();
        WorldgenError { file, pack, source }
    })?;
    for structure in &mut structures.structures {
        if structure.kind != "minecraft:jigsaw" {
            continue;
        }
        let error = |source| WorldgenError {
            file: structure.id.clone(),
            pack: "<structure>".into(),
            source,
        };
        let fields = super::codec::object(&structure.json).map_err(error)?;
        let jigsaw =
            JigsawStructure::decode(&fields, structure.terrain_adaptation, &mut pools, &cx)
                .map_err(error)?;
        structure.jigsaw = Some(jigsaw);
    }
    structures.pools = pools;
    Ok(())
}
