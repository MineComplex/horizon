//! `FloatProvider` (`net.minecraft.util.valueproviders`): the four float distributions the carvers'
//! configurations use, decoded as `FloatProviders.CODEC` reads them (a bare number is a constant)
//! and sampled with the random draws vanilla makes, in its order.

use serde_json::value::RawValue;

use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, float, object, required};
use crate::worldgen::placement::provider::type_of;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatProvider {
    Constant(f32),
    /// `min_inclusive` and `max_exclusive`.
    Uniform {
        min: f32,
        max: f32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
    Trapezoid {
        min: f32,
        max: f32,
        plateau: f32,
    },
}

impl FloatProvider {
    /// `FloatProviders.CODEC`.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        if let Ok(value) = float(raw) {
            return Ok(Self::Constant(value));
        }
        let fields = object(raw)?;
        let number = |name| required(&fields, name).and_then(float);
        match type_of(&fields)?.as_str() {
            "minecraft:constant" => Ok(Self::Constant(number("value")?)),
            "minecraft:uniform" => {
                let (min, max) = (number("min_inclusive")?, number("max_exclusive")?);
                if max <= min {
                    return Err(DecodeError::Field("uniform float: max must exceed min"));
                }
                Ok(Self::Uniform { min, max })
            }
            "minecraft:clamped_normal" => {
                let (min, max) = (number("min")?, number("max")?);
                if max < min {
                    return Err(DecodeError::Field("clamped normal float: max below min"));
                }
                Ok(Self::ClampedNormal {
                    mean: number("mean")?,
                    deviation: number("deviation")?,
                    min,
                    max,
                })
            }
            "minecraft:trapezoid" => {
                let (min, max, plateau) = (number("min")?, number("max")?, number("plateau")?);
                if max < min {
                    return Err(DecodeError::Field("trapezoid float: max below min"));
                }
                if plateau > max - min {
                    return Err(DecodeError::Field("trapezoid float: plateau past the span"));
                }
                Ok(Self::Trapezoid { min, max, plateau })
            }
            _ => Err(DecodeError::Field("float provider type")),
        }
    }

    /// `FloatProviders.codec(minValue)`: nothing below `low`.
    pub fn decode_at_least(raw: &RawValue, low: f32) -> Result<Self, DecodeError> {
        let provider = Self::decode(raw)?;
        if provider.min() < low {
            return Err(DecodeError::Field("float provider too low"));
        }
        Ok(provider)
    }

    /// `FloatProviders.codec(minValue, maxValue)`.
    pub fn decode_within(raw: &RawValue, low: f32, high: f32) -> Result<Self, DecodeError> {
        let provider = Self::decode_at_least(raw, low)?;
        if provider.max() > high {
            return Err(DecodeError::Field("float provider too high"));
        }
        Ok(provider)
    }

    pub fn min(&self) -> f32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, .. }
            | Self::ClampedNormal { min, .. }
            | Self::Trapezoid { min, .. } => min,
        }
    }

    pub fn max(&self) -> f32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { max, .. }
            | Self::ClampedNormal { max, .. }
            | Self::Trapezoid { max, .. } => max,
        }
    }

    pub fn sample(&self, random: &mut impl RandomSource) -> f32 {
        match *self {
            Self::Constant(value) => value,
            // `Mth.randomBetween`.
            Self::Uniform { min, max } => random.next_float() * (max - min) + min,
            // `Mth.clamp(Mth.normal(...), min, max)`: `Mth.normal` is float arithmetic and
            // `Math.min` passes a NaN on.
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let value = mean + random.next_gaussian() as f32 * deviation;
                if value < min {
                    min
                } else if value.is_nan() || max.is_nan() {
                    f32::NAN
                } else {
                    value.min(max)
                }
            }
            Self::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let plateau_start = (range - plateau) / 2.0;
                let plateau_end = range - plateau_start;
                min + random.next_float() * plateau_end + random.next_float() * plateau_start
            }
        }
    }
}
