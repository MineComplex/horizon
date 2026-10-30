//! `worldgen/noise_settings` (`NoiseGeneratorSettings.DIRECT_CODEC`): the vertical bounds, the
//! default block and fluid, the noise router's eight roots, the aquifer's six roots, the spawn
//! targets and the flags. Density roots are decoded into the data's [`DensityGraph`]; the
//! material rule stays a registry reference until surface rules are ported.
//!
//! [`DensityGraph`]: super::density::DensityGraph

use serde_json::value::RawValue;

use super::codec::{
    DecodeError, DensityLoader, Object, boolean, identifier, int_range, object, optional, required,
    string,
};
use super::density::FunctionId;
use crate::block::{BlockState, BlockStates};

/// `NoiseSettings`: the generator's vertical range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoiseSettings {
    pub min_y: i32,
    pub height: i32,
}

impl NoiseSettings {
    /// `NoiseSettings.create`'s checks: the wrapped top, then height alignment, then minimum
    /// alignment.
    pub fn create(min_y: i32, height: i32) -> Result<Self, DecodeError> {
        if min_y.wrapping_add(height) > 2032 {
            return Err(DecodeError::Field("noise upper bound"));
        }
        if height % 16 != 0 {
            return Err(DecodeError::Field("noise height alignment"));
        }
        if min_y % 16 != 0 {
            return Err(DecodeError::Field("noise minimum alignment"));
        }
        Ok(Self { min_y, height })
    }

    fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let object = object(raw)?;
        let min_y = int_range(required(&object, "min_y")?, "noise minimum", -2032, 2031)?;
        let height = int_range(required(&object, "height")?, "noise height", 0, 4064)?;
        Self::create(min_y, height)
    }

    /// `clampToHeightAccessor`: the top is minimum + height with Java wrapping; disjoint ranges
    /// can give a negative height, which vanilla keeps.
    pub fn clamp_to_height(self, accessor_minimum: i32, accessor_height: i32) -> Self {
        let min_y = self.min_y.max(accessor_minimum);
        let upper = self
            .min_y
            .wrapping_add(self.height)
            .min(accessor_minimum.wrapping_add(accessor_height));
        Self {
            min_y,
            height: upper.wrapping_sub(min_y),
        }
    }
}

/// `NoiseRouter`'s fields, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum RouterField {
    Temperature,
    Vegetation,
    Continents,
    Erosion,
    Depth,
    Ridges,
    ChunkSurfaceLevel,
    FinalDensity,
}

impl RouterField {
    pub const ALL: [Self; 8] = [
        Self::Temperature,
        Self::Vegetation,
        Self::Continents,
        Self::Erosion,
        Self::Depth,
        Self::Ridges,
        Self::ChunkSurfaceLevel,
        Self::FinalDensity,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Vegetation => "vegetation",
            Self::Continents => "continents",
            Self::Erosion => "erosion",
            Self::Depth => "depth",
            Self::Ridges => "ridges",
            Self::ChunkSurfaceLevel => "chunk_surface_level",
            Self::FinalDensity => "final_density",
        }
    }
}

/// `Aquifer.Config`'s density roots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum AquiferField {
    Barrier,
    Floodedness,
    Spread,
    Lava,
    Exclusion,
    SurfaceLevel,
}

impl AquiferField {
    pub const ALL: [Self; 6] = [
        Self::Barrier,
        Self::Floodedness,
        Self::Spread,
        Self::Lava,
        Self::Exclusion,
        Self::SurfaceLevel,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Barrier => "barrier",
            Self::Floodedness => "fluid_level_floodedness",
            Self::Spread => "fluid_level_spread",
            Self::Lava => "lava",
            Self::Exclusion => "exclusion",
            Self::SurfaceLevel => "surface_level",
        }
    }
}

/// `Climate.Parameter`: a quantized interval (`Climate.quantizeCoord`: times 10000, truncated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClimateParameter {
    pub min: i64,
    pub max: i64,
}

impl ClimateParameter {
    pub fn quantize(value: f32) -> i64 {
        (value * 10_000.0) as i64
    }

    /// The scalar, two-element list or min/max object codec, values in [-2, 2].
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        fn bounded(raw: &RawValue) -> Result<f32, DecodeError> {
            let value = super::codec::float(raw)?;
            if value.total_cmp(&-2.0).is_lt() || value.total_cmp(&2.0).is_gt() {
                return Err(DecodeError::Field("climate float bounds"));
            }
            Ok(value)
        }
        let (min, max) = if raw.get().starts_with('[') {
            let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
            if values.len() != 2 {
                return Err(DecodeError::Field("climate interval length"));
            }
            (bounded(&values[0])?, bounded(&values[1])?)
        } else if raw.get().starts_with('{') {
            let object = object(raw)?;
            (
                bounded(required(&object, "min")?)?,
                bounded(required(&object, "max")?)?,
            )
        } else {
            let value = bounded(raw)?;
            (value, value)
        };
        if min.total_cmp(&max).is_gt() {
            return Err(DecodeError::Field("climate interval order"));
        }
        Ok(Self {
            min: Self::quantize(min),
            max: Self::quantize(max),
        })
    }
}

/// `SpawnTargetPoint`: density functions and the intervals the spawn search wants them in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnTargetPoint {
    pub parameters: Vec<(FunctionId, ClimateParameter)>,
}

/// One `debug_functions` entry (vanilla's debug renderer; kept because the codec validates it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugFunction {
    pub function: FunctionId,
    pub label: String,
}

/// A decoded `worldgen/noise_settings` element.
#[derive(Clone, Debug)]
pub struct NoiseGeneratorSettings {
    pub noise: NoiseSettings,
    pub default_block: BlockState,
    pub default_fluid: BlockState,
    /// By [`RouterField`].
    pub router: [FunctionId; 8],
    /// The `worldgen/material_rule` element (surface rules come with their port).
    pub material_rule: String,
    pub spawn_target: Vec<SpawnTargetPoint>,
    pub sea_level: i32,
    pub disable_mob_generation: bool,
    /// By [`AquiferField`]; `None` disables aquifers.
    pub aquifers: Option<[FunctionId; 6]>,
    pub legacy_random_source: bool,
    pub debug_functions: Vec<DebugFunction>,
}

impl NoiseGeneratorSettings {
    pub fn router(&self, field: RouterField) -> FunctionId {
        self.router[field as usize]
    }

    pub(crate) fn decode(
        raw: &RawValue,
        densities: &mut DensityLoader<'_>,
        blocks: &BlockStates,
    ) -> Result<Self, DecodeError> {
        let object = object(raw)?;
        let roots = |densities: &mut DensityLoader<'_>,
                     object: &Object,
                     names: &[&'static str]|
         -> Result<Vec<FunctionId>, DecodeError> {
            names
                .iter()
                .map(|name| densities.decode(required(object, name)?, 0))
                .collect()
        };
        let router_object = super::codec::object(required(&object, "noise_router")?)?;
        let router = roots(
            densities,
            &router_object,
            &RouterField::ALL.map(RouterField::name),
        )?
        .try_into()
        .expect("eight router roots");
        let aquifers = match optional(&object, "aquifers") {
            Some(raw) => {
                let aquifer_object = super::codec::object(raw)?;
                Some(
                    roots(
                        densities,
                        &aquifer_object,
                        &AquiferField::ALL.map(AquiferField::name),
                    )?
                    .try_into()
                    .expect("six aquifer roots"),
                )
            }
            None => None,
        };
        let spawn_target = match optional(&object, "spawn_target") {
            Some(raw) => {
                let points: Vec<Object> = serde_json::from_str(raw.get())?;
                points
                    .into_iter()
                    .map(|point| {
                        let parameters = point
                            .iter()
                            .map(|(name, interval)| {
                                let function =
                                    densities.decode(&serde_json::value::to_raw_value(name)?, 0)?;
                                Ok((function, ClimateParameter::decode(interval)?))
                            })
                            .collect::<Result<_, DecodeError>>()?;
                        Ok(SpawnTargetPoint { parameters })
                    })
                    .collect::<Result<_, DecodeError>>()?
            }
            None => Vec::new(),
        };
        let debug_functions = match optional(&object, "debug_functions") {
            Some(raw) => {
                let entries: Vec<Object> = serde_json::from_str(raw.get())?;
                entries
                    .iter()
                    .map(|entry| {
                        Ok(DebugFunction {
                            function: densities.decode(required(entry, "function")?, 0)?,
                            label: string(required(entry, "label")?)?,
                        })
                    })
                    .collect::<Result<_, DecodeError>>()?
            }
            None => Vec::new(),
        };
        Ok(Self {
            noise: NoiseSettings::decode(required(&object, "noise")?)?,
            default_block: block_state(required(&object, "default_block")?, blocks)?,
            default_fluid: block_state(required(&object, "default_fluid")?, blocks)?,
            router,
            material_rule: identifier(&string(required(&object, "material_rule")?)?)?,
            spawn_target,
            sea_level: super::codec::integer(required(&object, "sea_level")?)?,
            disable_mob_generation: boolean(required(&object, "disable_mob_generation")?)?,
            aquifers,
            legacy_random_source: boolean(required(&object, "legacy_random_source")?)?,
            debug_functions,
        })
    }
}

/// A block state: a block id (its default state) or `{"Name": ..., "Properties": {...}}`.
pub fn block_state(raw: &RawValue, blocks: &BlockStates) -> Result<BlockState, DecodeError> {
    let unknown = |what: &str| DecodeError::Registry(format!("unknown block state {what}"));
    let (name, properties) = if raw.get().starts_with('{') {
        let object = object(raw)?;
        let properties: Vec<(String, String)> = match optional(&object, "Properties") {
            Some(raw) => {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw.get())?
                    .into_iter()
                    .map(|(k, v)| match v {
                        serde_json::Value::String(v) => Ok((k, v)),
                        _ => Err(DecodeError::Field("block state property")),
                    })
                    .collect::<Result<_, _>>()?
            }
            None => Vec::new(),
        };
        (string(required(&object, "Name")?)?, properties)
    } else {
        (string(raw)?, Vec::new())
    };
    let name = identifier(&name)?;
    let block = blocks.block_by_name(&name).ok_or_else(|| unknown(&name))?;
    blocks
        .state_with(
            block,
            properties.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        )
        .ok_or_else(|| unknown(&name))
}
