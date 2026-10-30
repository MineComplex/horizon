//! The worldgen registries of the server's datapacks, decoded once at startup (and `/reload`)
//! into immutable tables, as `RegistryDataLoader` loads `WORLDGEN_REGISTRIES`: every element is
//! decoded before the server starts, an invalid one stops it. Seed-independent; a level compiles
//! what it needs from it ([`WorldgenData::compile_level`]).
//!
//! Decoded so far: `worldgen/noise`, `worldgen/density_function` and `worldgen/noise_settings`
//! (into one [`DensityGraph`]), and Paper's flat bedrock conditions of the bedrock rules. The
//! other worldgen registries are kept as parsed JSON ([`RawRegistry`]) for the packages that
//! decode them (surface rules, carvers, features, structures, presets).

use serde_json::value::RawValue;

use super::anchor::{OPTIONALLY_FLAT_BEDROCK, OptionallyFlatBedrock};
use super::codec::{DecodeError, DensityLoader, decode_noise, object, optional, string};
use super::density::{
    DensityCompiler, DensityGraph, DensityProgram, FunctionId, NodeId, ProgramError,
};
use super::settings::{AquiferField, NoiseGeneratorSettings, RouterField};
use crate::block::BlockStates;
use crate::registry::{DataPack, WORLDGEN, for_each_element};

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
    /// Paper's conditions in `worldgen/material_rule`, by rule id (until surface rules decode
    /// the whole registry).
    flat_bedrock: Vec<(String, OptionallyFlatBedrock)>,
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
}

impl LevelDensity {
    pub fn router(&self, field: RouterField) -> NodeId {
        self.router[field as usize]
    }
    pub fn aquifer(&self, field: AquiferField) -> Option<NodeId> {
        self.aquifers.map(|roots| roots[field as usize])
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
        let mut flat_bedrock = Vec::new();
        for_each_element(packs, "minecraft:worldgen/material_rule", |file| {
            let raw = parse(&file)?;
            if let Some(condition) = find_flat_bedrock(&raw).map_err(|e| error(&file, e))? {
                flat_bedrock.push((file.id.to_owned(), condition));
            }
            Ok(())
        })?;
        let mut raw = Vec::new();
        for name in WORLDGEN {
            if matches!(
                name,
                "minecraft:worldgen/noise"
                    | "minecraft:worldgen/density_function"
                    | "minecraft:worldgen/noise_settings"
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
        Ok(Self {
            graph,
            noise_settings,
            flat_bedrock,
            raw,
        })
    }

    pub fn graph(&self) -> &DensityGraph {
        &self.graph
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

    /// Paper's flat bedrock condition of a material rule, where it has one.
    pub fn flat_bedrock(&self, rule: &str) -> Option<&OptionallyFlatBedrock> {
        self.flat_bedrock
            .iter()
            .find(|(name, _)| name == rule)
            .map(|(_, condition)| condition)
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
    ) -> Result<LevelDensity, ProgramError> {
        Ok(self
            .compile_roots(self.graph.clone(), settings, seed, &[])?
            .0)
    }

    /// [`Self::compile_level`] with more density functions (JSON, as `DensityFunction.CODEC`
    /// reads them) compiled into the same program, as `RandomState.getSampler` does for functions
    /// other systems hold (surface rules, debug functions, tests).
    pub fn compile_level_with(
        &self,
        settings: &NoiseGeneratorSettings,
        seed: i64,
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
        self.compile_roots(graph, settings, seed, &roots)
            .map_err(|e| error(DecodeError::Registry(e.to_string())))
    }

    fn compile_roots(
        &self,
        graph: DensityGraph,
        settings: &NoiseGeneratorSettings,
        seed: i64,
        extra: &[FunctionId],
    ) -> Result<(LevelDensity, Vec<NodeId>), ProgramError> {
        let mut compiler = DensityCompiler::new(graph, seed, settings.legacy_random_source)?;
        let mut compile = |roots: &[FunctionId]| {
            roots
                .iter()
                .map(|root| compiler.compile(*root))
                .collect::<Result<Vec<_>, _>>()
        };
        let router = compile(&settings.router)?;
        let aquifers = settings.aquifers.map(|roots| compile(&roots)).transpose()?;
        let extra = compile(extra)?;
        let density = LevelDensity {
            program: compiler.finish(),
            router: router.try_into().expect("eight router roots"),
            aquifers: aquifers.map(|nodes| nodes.try_into().expect("six aquifer roots")),
        };
        Ok((density, extra))
    }
}

/// Paper's condition inside a bedrock rule: `condition` rules whose `if_true` is the condition,
/// or `not` of it (the roof).
fn find_flat_bedrock(raw: &RawValue) -> Result<Option<OptionallyFlatBedrock>, DecodeError> {
    if !raw.get().starts_with('{') {
        return Ok(None);
    }
    let rule = object(raw)?;
    match optional(&rule, "if_true") {
        Some(condition) => find_flat_bedrock_condition(condition),
        None => Ok(None),
    }
}

fn find_flat_bedrock_condition(
    condition: &RawValue,
) -> Result<Option<OptionallyFlatBedrock>, DecodeError> {
    if !condition.get().starts_with('{') {
        return Ok(None);
    }
    let fields = object(condition)?;
    match optional(&fields, "type")
        .map(string)
        .transpose()?
        .as_deref()
    {
        Some(OPTIONALLY_FLAT_BEDROCK) => OptionallyFlatBedrock::decode(condition).map(Some),
        Some("minecraft:not") => match fields.get("invert") {
            Some(inner) => find_flat_bedrock_condition(inner),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}
