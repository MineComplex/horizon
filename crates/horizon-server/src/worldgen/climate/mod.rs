//! `Climate` (26.3, with Moonrise's search): quantized climate coordinates, parameter points, the
//! six-field climate sampler over a level's [`DensityProgram`], and the R-tree biome search
//! ([`ClimateIndex`]).
//!
//! Copied from Horizon's `horizon-world::worldgen::climate` and adapted: the sampler reads the
//! level's compiled program by node instead of six `Arc<dyn DensitySampler>`s, and the previous
//! search result (vanilla's and Moonrise's thread-local) is a [`ClimateSearch`] the caller owns.

mod index;

use serde_json::value::RawValue;

pub use index::{ClimateIndex, ClimateSearch, IndexError};

use super::codec::{DecodeError, float, object, required};
use super::data::LevelDensity;
use super::density::{DensityContext, DensityProgram, NodeId};
use super::noise::{DensityError, DensityVolume};
use super::settings::RouterField;

/// `Climate.quantizeCoord`: the float product, then Java's saturating `long` cast.
pub fn quantize(value: f32) -> i64 {
    (value * 10_000.0) as i64
}

/// `Climate.unquantizeCoord`.
pub fn unquantize(value: i64) -> f32 {
    value as f32 / 10_000.0
}

/// `Climate.Parameter`: a quantized interval. The record constructor does not check the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub min: i64,
    pub max: i64,
}

impl Parameter {
    pub const fn new(min: i64, max: i64) -> Self {
        Self { min, max }
    }

    /// `Parameter.CODEC`: a scalar, a two-element list or a `{min, max}` object of floats in
    /// [-2, 2], `min <= max`.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let (min, max) = if raw.get().starts_with('[') {
            let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
            if values.len() != 2 {
                return Err(DecodeError::Field("climate interval length"));
            }
            (
                bounded(&values[0], -2.0, 2.0)?,
                bounded(&values[1], -2.0, 2.0)?,
            )
        } else if raw.get().starts_with('{') {
            let object = object(raw)?;
            (
                bounded(required(&object, "min")?, -2.0, 2.0)?,
                bounded(required(&object, "max")?, -2.0, 2.0)?,
            )
        } else {
            let value = bounded(raw, -2.0, 2.0)?;
            (value, value)
        };
        if min.total_cmp(&max).is_gt() {
            return Err(DecodeError::Field("climate interval order"));
        }
        Ok(Self::new(quantize(min), quantize(max)))
    }

    /// `Parameter.distance`: how far `value` lies outside the interval.
    pub fn distance(self, value: i64) -> i64 {
        let above = value.wrapping_sub(self.max);
        if above > 0 {
            above
        } else {
            self.min.wrapping_sub(value).max(0)
        }
    }

    fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    fn center(self) -> i64 {
        self.min.wrapping_add(self.max) / 2
    }
}

/// `Codec.floatRange`.
fn bounded(raw: &RawValue, min: f32, max: f32) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if value.total_cmp(&min).is_lt() || value.total_cmp(&max).is_gt() {
        return Err(DecodeError::Field("climate float bounds"));
    }
    Ok(value)
}

/// The six climate coordinates, in `ParameterPoint`/`TargetPoint` order.
pub const COORDINATES: [&str; 6] = [
    "temperature",
    "humidity",
    "continentalness",
    "erosion",
    "depth",
    "weirdness",
];

/// `Climate.ParameterPoint`: six intervals and an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterPoint {
    /// In [`COORDINATES`] order.
    pub coordinates: [Parameter; 6],
    pub offset: i64,
}

impl ParameterPoint {
    /// `ParameterPoint.CODEC`: the six intervals and an `offset` float in [0, 1].
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let object = object(raw)?;
        let mut coordinates = [Parameter::new(0, 0); 6];
        for (coordinate, name) in coordinates.iter_mut().zip(COORDINATES) {
            *coordinate = Parameter::decode(object.get(name).ok_or(DecodeError::Field(name))?)?;
        }
        let offset = quantize(bounded(required(&object, "offset")?, 0.0, 1.0)?);
        Ok(Self {
            coordinates,
            offset,
        })
    }

    /// `ParameterPoint.fitness`: summed squared distances plus the squared offset, wrapping.
    pub fn fitness(&self, target: TargetPoint) -> i64 {
        self.coordinates.iter().zip(target.parameters()).fold(
            self.offset.wrapping_mul(self.offset),
            |sum, (parameter, value)| {
                let distance = parameter.distance(value);
                sum.wrapping_add(distance.wrapping_mul(distance))
            },
        )
    }

    /// `parameterSpace`: the six intervals and the offset as a point interval.
    pub fn parameter_space(&self) -> [Parameter; 7] {
        std::array::from_fn(|i| {
            if i < 6 {
                self.coordinates[i]
            } else {
                Parameter::new(self.offset, self.offset)
            }
        })
    }
}

/// `Climate.TargetPoint`: a sampled, quantized climate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetPoint {
    /// In [`COORDINATES`] order.
    pub values: [i64; 6],
}

impl TargetPoint {
    /// `Climate.target`: each field quantized.
    pub fn from_values(values: [f32; 6]) -> Self {
        Self {
            values: values.map(quantize),
        }
    }

    /// `toParameterArray`: the six values and 0 for the offset axis.
    pub fn parameters(self) -> [i64; 7] {
        std::array::from_fn(|i| if i < 6 { self.values[i] } else { 0 })
    }
}

/// `Climate.Sampler`: the router's six climate fields (temperature, vegetation, continents,
/// erosion, depth, ridges) of a level's program. Borrowed; the context is the caller's.
#[derive(Clone, Copy, Debug)]
pub struct ClimateSampler<'a> {
    program: &'a DensityProgram,
    fields: [NodeId; 6],
}

impl<'a> ClimateSampler<'a> {
    pub fn new(density: &'a LevelDensity) -> Self {
        Self {
            program: &density.program,
            fields: [
                RouterField::Temperature,
                RouterField::Vegetation,
                RouterField::Continents,
                RouterField::Erosion,
                RouterField::Depth,
                RouterField::Ridges,
            ]
            .map(|field| density.router(field)),
        }
    }

    /// `Sampler.sample`: the six fields at the quart's block corner (`quart << 2`, wrapping), in
    /// field order with one context.
    pub fn sample(&self, context: &mut DensityContext, quart: [i32; 3]) -> TargetPoint {
        let position = quart.map(|v| v.wrapping_shl(2));
        TargetPoint::from_values(
            self.fields
                .map(|field| self.program.sample(context, field, position)),
        )
    }

    /// `MultiNoiseBiomeSource.createResolverForChunk`'s volumes: each field over the quart box
    /// (block steps of 4), field by field.
    pub fn sample_volume(
        &self,
        context: &mut DensityContext,
        quart_minimum: [i32; 3],
        sizes: [i32; 3],
    ) -> Result<ClimateVolume, DensityError> {
        let volume = DensityVolume::new(sizes, quart_minimum.map(|v| v.wrapping_shl(2)), [4; 3])?;
        let fields = self.fields.map(|field| {
            let mut values = vec![0.0; volume.size()];
            self.program.fill(context, field, &mut values, volume);
            values
        });
        Ok(ClimateVolume {
            quart_minimum,
            volume,
            fields,
        })
    }
}

/// The six climate fields sampled over a quart box.
#[derive(Clone, Debug)]
pub struct ClimateVolume {
    quart_minimum: [i32; 3],
    volume: DensityVolume,
    fields: [Vec<f32>; 6],
}

impl ClimateVolume {
    /// The target at a quart inside the box (vanilla indexes unchecked; outside is `None`).
    pub fn target(&self, quart: [i32; 3]) -> Option<TargetPoint> {
        let relative: [usize; 3] = std::array::from_fn(|i| {
            usize::try_from(quart[i].wrapping_sub(self.quart_minimum[i])).unwrap_or(usize::MAX)
        });
        let index = self.volume.index(relative)?;
        Some(TargetPoint::from_values(std::array::from_fn(|i| {
            self.fields[i][index]
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(json: &str) -> Box<RawValue> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parameters_decode_in_three_shapes() {
        let point = Parameter::decode(&raw("0.5")).unwrap();
        assert_eq!(point, Parameter::new(5000, 5000));
        assert_eq!(
            Parameter::decode(&raw("[-1.2, -1.05]")).unwrap(),
            Parameter::new(quantize(-1.2), quantize(-1.05))
        );
        assert_eq!(
            Parameter::decode(&raw(r#"{"min": -0.1, "max": 0.2}"#)).unwrap(),
            Parameter::new(-1000, 2000)
        );
        assert!(Parameter::decode(&raw("[0.3, 0.2]")).is_err());
        assert!(Parameter::decode(&raw("2.5")).is_err());
        let point = ParameterPoint::decode(&raw(
            r#"{"temperature":0,"humidity":[0,1],"continentalness":0,"erosion":0,
                "depth":0,"weirdness":0,"offset":0.375}"#,
        ))
        .unwrap();
        assert_eq!(point.offset, 3750);
        assert!(
            ParameterPoint::decode(&raw(
                r#"{"temperature":0,"humidity":0,"continentalness":0,"erosion":0,
                    "depth":0,"weirdness":0,"offset":1.5}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn distance_and_fitness() {
        let p = Parameter::new(-100, 200);
        assert_eq!(
            (p.distance(-150), p.distance(0), p.distance(250)),
            (50, 0, 50)
        );
        let point = ParameterPoint {
            coordinates: [p; 6],
            offset: 3,
        };
        let target = TargetPoint {
            values: [-150, 0, 0, 0, 0, 210],
        };
        assert_eq!(point.fitness(target), 9 + 2500 + 100);
    }
}
