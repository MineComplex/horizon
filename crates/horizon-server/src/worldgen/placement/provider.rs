//! `IntProvider` (`net.minecraft.util.valueproviders`) and `HeightProvider`
//! (`levelgen.heightproviders`) with the weighted list both use. Decoded from the registries'
//! JSON as the codecs read it; sampling consumes the random draws vanilla's does, in its order.
//! Float providers are not here: only carvers use them.

use serde_json::value::RawValue;

use crate::util::random::{InvalidBound, RandomSource};
use crate::worldgen::anchor::{VerticalAnchor, WorldGenerationContext};
use crate::worldgen::codec::{
    DecodeError, Object, float, identifier, int_range, integer, object, optional, required, string,
};

/// The `type` of a dispatched codec object, as an `ns:path` id.
pub(crate) fn type_of(fields: &Object) -> Result<String, DecodeError> {
    identifier(&string(required(fields, "type")?)?)
}

/// `WeightedList<T>`: entries with non-negative weights and a positive total.
#[derive(Clone, Debug)]
pub struct WeightedList<T> {
    entries: Vec<(T, i32)>,
    total: i32,
}

impl<T> WeightedList<T> {
    /// The codec's list of `{"data": ..., "weight": n}`.
    pub(crate) fn decode(
        raw: &RawValue,
        mut data: impl FnMut(&RawValue) -> Result<T, DecodeError>,
    ) -> Result<Self, DecodeError> {
        let items: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
        let mut entries = Vec::with_capacity(items.len());
        let mut total = 0_i32;
        for item in &items {
            let item = object(item)?;
            let weight = int_range(required(&item, "weight")?, "weight", 0, i32::MAX)?;
            total = total
                .checked_add(weight)
                .ok_or(DecodeError::Field("total weight"))?;
            entries.push((data(required(&item, "data")?)?, weight));
        }
        if total == 0 {
            return Err(DecodeError::Field("empty weighted list"));
        }
        Ok(Self { entries, total })
    }

    /// `getRandom`: one bounded draw, then the entry whose cumulative weight passes it.
    pub(crate) fn pick(&self, random: &mut impl RandomSource) -> Result<&T, InvalidBound> {
        let mut index = random.next_int_bound(self.total)?;
        for (value, weight) in &self.entries {
            index -= weight;
            if index < 0 {
                return Ok(value);
            }
        }
        unreachable!("the draw is below the total weight")
    }
}

#[derive(Clone, Debug)]
pub enum IntProvider {
    Constant(i32),
    Uniform(i32, i32),
    BiasedToBottom(i32, i32),
    VeryBiasedToBottom(i32, i32),
    Trapezoid {
        min: i32,
        max: i32,
        plateau: i32,
    },
    Clamped {
        source: Box<Self>,
        min: i32,
        max: i32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: i32,
        max: i32,
    },
    Weighted(WeightedList<Self>),
}

impl IntProvider {
    /// `IntProvider.CODEC`: a number is a constant.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        if let Ok(value) = integer(raw) {
            return Ok(Self::Constant(value));
        }
        let fields = object(raw)?;
        let kind = type_of(&fields)?;
        let int = |name| required(&fields, name).and_then(integer);
        match kind.as_str() {
            "minecraft:constant" => return Ok(Self::Constant(int("value")?)),
            "minecraft:weighted_list" => {
                return Ok(Self::Weighted(WeightedList::decode(
                    required(&fields, "distribution")?,
                    Self::decode,
                )?));
            }
            _ => {}
        }
        let trapezoid = kind == "minecraft:trapezoid";
        let min = int(if trapezoid { "min" } else { "min_inclusive" })?;
        let max = int(if trapezoid { "max" } else { "max_inclusive" })?;
        if max < min {
            return Err(DecodeError::Field("integer provider bounds"));
        }
        match kind.as_str() {
            "minecraft:uniform" => Ok(Self::Uniform(min, max)),
            "minecraft:biased_to_bottom" => Ok(Self::BiasedToBottom(min, max)),
            "minecraft:very_biased_to_bottom" => Ok(Self::VeryBiasedToBottom(min, max)),
            "minecraft:trapezoid" => {
                let plateau = int("plateau")?;
                if plateau > max.wrapping_sub(min) {
                    return Err(DecodeError::Field("trapezoid plateau"));
                }
                Ok(Self::Trapezoid { min, max, plateau })
            }
            "minecraft:clamped" => Ok(Self::Clamped {
                source: Box::new(Self::decode(required(&fields, "source")?)?),
                min,
                max,
            }),
            "minecraft:clamped_normal" => Ok(Self::ClampedNormal {
                mean: required(&fields, "mean").and_then(float)?,
                deviation: required(&fields, "deviation").and_then(float)?,
                min,
                max,
            }),
            _ => Err(DecodeError::Field("integer provider type")),
        }
    }

    /// `getMinValue()` and `getMaxValue()`.
    pub fn bounds(&self) -> [i32; 2] {
        match self {
            Self::Constant(value) => [*value; 2],
            Self::Uniform(min, max)
            | Self::BiasedToBottom(min, max)
            | Self::VeryBiasedToBottom(min, max)
            | Self::Trapezoid { min, max, .. }
            | Self::ClampedNormal { min, max, .. } => [*min, *max],
            Self::Clamped { source, min, max } => {
                let inner = source.bounds();
                [(*min).max(inner[0]), (*max).min(inner[1])]
            }
            Self::Weighted(list) => {
                list.entries
                    .iter()
                    .fold([i32::MAX, i32::MIN], |bounds, (provider, _)| {
                        let next = provider.bounds();
                        [bounds[0].min(next[0]), bounds[1].max(next[1])]
                    })
            }
        }
    }

    /// `IntProvider.codec(min, max)`: the provider's whole range inside `min..=max`.
    pub fn decode_within(raw: &RawValue, min: i32, max: i32) -> Result<Self, DecodeError> {
        let provider = Self::decode(raw)?;
        let [low, high] = provider.bounds();
        if low < min || high > max {
            return Err(DecodeError::Field("integer provider outside its range"));
        }
        Ok(provider)
    }

    pub fn sample(&self, random: &mut impl RandomSource) -> Result<i32, InvalidBound> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Uniform(min, max) => random.next_int_between_inclusive(*min, *max),
            Self::BiasedToBottom(min, max) | Self::VeryBiasedToBottom(min, max) => {
                let mut value = random.next_int_bound(max.wrapping_sub(*min).wrapping_add(1))?;
                value = random.next_int_bound(value.wrapping_add(1))?;
                if matches!(self, Self::VeryBiasedToBottom(..)) {
                    value = random.next_int_bound(value.wrapping_add(1))?;
                }
                Ok(min.wrapping_add(value))
            }
            Self::Trapezoid { min, max, plateau } => {
                if *plateau == 0 && *max == min.wrapping_neg() {
                    let left = random.next_int_bound(max.wrapping_add(1))?;
                    let right = random.next_int_bound(max.wrapping_add(1))?;
                    return Ok(left.wrapping_sub(right));
                }
                let range = max.wrapping_sub(*min);
                if *plateau == range {
                    return random.next_int_between_inclusive(*min, *max);
                }
                trapezoid(random, *min, range, *plateau)
            }
            Self::Clamped { source, min, max } => Ok(source.sample(random)?.clamp(*min, *max)),
            // `Mth.normal` in float arithmetic, `Mth.clamp` on floats, then the saturating cast.
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let value = mean + random.next_gaussian() as f32 * deviation;
                let (min, max) = (*min as f32, *max as f32);
                Ok((if value < min { min } else { value.min(max) }) as i32)
            }
            Self::Weighted(list) => list.pick(random)?.sample(random),
        }
    }
}

/// The sloped sum of two uniform draws both trapezoids use.
fn trapezoid(
    random: &mut impl RandomSource,
    min: i32,
    range: i32,
    plateau: i32,
) -> Result<i32, InvalidBound> {
    let slope = range.wrapping_sub(plateau) / 2;
    let middle = range.wrapping_sub(slope);
    let left = random.next_int_between_inclusive(0, middle)?;
    let right = random.next_int_between_inclusive(0, slope)?;
    Ok(min.wrapping_add(left).wrapping_add(right))
}

/// The guarded `Mth.randomBetweenInclusive` of the very biased provider: no draw when the range
/// is empty.
fn between(random: &mut impl RandomSource, low: i32, high: i32) -> Result<i32, InvalidBound> {
    if low >= high {
        Ok(low)
    } else {
        random.next_int_between_inclusive(low, high)
    }
}

#[derive(Clone, Debug)]
pub enum HeightProvider {
    Constant(VerticalAnchor),
    Uniform(VerticalAnchor, VerticalAnchor),
    BiasedToBottom {
        min: VerticalAnchor,
        max: VerticalAnchor,
        inner: i32,
    },
    VeryBiasedToBottom {
        min: VerticalAnchor,
        max: VerticalAnchor,
        inner: i32,
    },
    Trapezoid {
        min: VerticalAnchor,
        max: VerticalAnchor,
        plateau: i32,
    },
    Weighted(WeightedList<Self>),
}

impl HeightProvider {
    /// `HeightProvider.CODEC`: a vertical anchor is a constant.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        if let Ok(anchor) = VerticalAnchor::decode(raw) {
            return Ok(Self::Constant(anchor));
        }
        let fields = object(raw)?;
        let kind = type_of(&fields)?;
        let anchor = |name| VerticalAnchor::decode(required(&fields, name)?);
        match kind.as_str() {
            "minecraft:constant" => return Ok(Self::Constant(anchor("value")?)),
            "minecraft:weighted_list" => {
                return Ok(Self::Weighted(WeightedList::decode(
                    required(&fields, "distribution")?,
                    Self::decode,
                )?));
            }
            _ => {}
        }
        let (min, max) = (anchor("min_inclusive")?, anchor("max_inclusive")?);
        let inner =
            || optional(&fields, "inner").map_or(Ok(1), |r| int_range(r, "inner", 1, i32::MAX));
        match kind.as_str() {
            "minecraft:uniform" => Ok(Self::Uniform(min, max)),
            "minecraft:biased_to_bottom" => Ok(Self::BiasedToBottom {
                min,
                max,
                inner: inner()?,
            }),
            "minecraft:very_biased_to_bottom" => Ok(Self::VeryBiasedToBottom {
                min,
                max,
                inner: inner()?,
            }),
            "minecraft:trapezoid" => Ok(Self::Trapezoid {
                min,
                max,
                plateau: optional(&fields, "plateau").map_or(Ok(0), integer)?,
            }),
            _ => Err(DecodeError::Field("height provider type")),
        }
    }

    pub fn sample(
        &self,
        random: &mut impl RandomSource,
        context: WorldGenerationContext,
    ) -> Result<i32, InvalidBound> {
        let (min, max) = match self {
            Self::Constant(anchor) => return Ok(anchor.resolve(context)),
            Self::Weighted(list) => return list.pick(random)?.sample(random, context),
            Self::Uniform(min, max)
            | Self::BiasedToBottom { min, max, .. }
            | Self::VeryBiasedToBottom { min, max, .. }
            | Self::Trapezoid { min, max, .. } => (min.resolve(context), max.resolve(context)),
        };
        match self {
            Self::Uniform(..) => {
                if min > max {
                    Ok(min)
                } else {
                    random.next_int_between_inclusive(min, max)
                }
            }
            Self::BiasedToBottom { inner, .. } => {
                let range = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
                if range <= 0 {
                    return Ok(min);
                }
                let first = random.next_int_bound(range)?;
                Ok(random
                    .next_int_bound(first.wrapping_add(*inner))?
                    .wrapping_add(min))
            }
            Self::VeryBiasedToBottom { inner, .. } => {
                let range = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
                if range <= 0 {
                    return Ok(min);
                }
                let first = between(random, min.wrapping_add(*inner), max)?;
                let second = between(random, min, first.wrapping_sub(1))?;
                between(random, min, second.wrapping_sub(1).wrapping_add(*inner))
            }
            Self::Trapezoid { plateau, .. } => {
                if min > max {
                    return Ok(min);
                }
                let range = max.wrapping_sub(min);
                if *plateau >= range {
                    return random.next_int_between_inclusive(min, max);
                }
                trapezoid(random, min, range, *plateau)
            }
            Self::Constant(_) | Self::Weighted(_) => unreachable!("returned above"),
        }
    }
}
