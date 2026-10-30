//! Vertical anchors and their generation bounds (`VerticalAnchor`, `WorldGenerationContext`), and
//! Paper's `paper:optionally_flat_bedrock_material_condition`, the condition of the bedrock
//! floor and roof rules Paper ships in place of vanilla's (see `registry::DataPack::vanilla`).
//! Anchors are copied from Horizon's `horizon-world::worldgen::anchor`.

use serde_json::value::RawValue;

use super::codec::{DecodeError, Object, boolean, identifier, int_range, object, required, string};
use crate::util::random::{PositionalRandom, RandomSource};

/// `WorldGenerationContext`: the generator's vertical range within the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldGenerationContext {
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,
}

impl WorldGenerationContext {
    pub const fn new(min_y: i32, height: i32, sea_level: i32) -> Self {
        Self {
            min_y,
            height,
            sea_level,
        }
    }

    /// Vanilla combines the two minima and heights independently: deliberately not the
    /// intersection of the two ranges.
    pub fn with_generator(
        level_min: i32,
        level_height: i32,
        generator_min: i32,
        generator_height: i32,
        generator_sea_level: i32,
    ) -> Self {
        Self::new(
            level_min.max(generator_min),
            level_height.min(generator_height),
            generator_sea_level,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
    RelativeToSeaLevel(i32),
}

impl VerticalAnchor {
    pub const BOTTOM: Self = Self::AboveBottom(0);
    pub const TOP: Self = Self::BelowTop(0);

    /// Runtime anchors take any int, with Java's wrapping.
    pub fn resolve(self, context: WorldGenerationContext) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => context.min_y.wrapping_add(offset),
            Self::BelowTop(offset) => context
                .height
                .wrapping_sub(1)
                .wrapping_add(context.min_y)
                .wrapping_sub(offset),
            Self::RelativeToSeaLevel(offset) => context.sea_level.wrapping_add(offset),
        }
    }

    /// `VerticalAnchor.CODEC`.
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        Self::from_object(&object(raw)?)
    }

    fn from_object(object: &Object) -> Result<Self, DecodeError> {
        let field = |name| {
            required(object, name)
                .and_then(|raw| int_range(raw, name, -2032, 2031))
                .ok()
        };
        // The codec nests exclusive alternatives: an ambiguous inner choice fails that branch,
        // which lets a valid outer branch succeed.
        let exclusive = |left, right| match (left, right) {
            (Some(value), None) | (None, Some(value)) => Some(value),
            _ => None,
        };
        exclusive(
            field("absolute").map(Self::Absolute),
            exclusive(
                field("above_bottom").map(Self::AboveBottom),
                exclusive(
                    field("below_top").map(Self::BelowTop),
                    field("relative_to_sea_level").map(Self::RelativeToSeaLevel),
                ),
            ),
        )
        .ok_or(DecodeError::Field("vertical anchor"))
    }
}

/// `io.papermc.paper.world.worldgen.OptionallyFlatBedrockCondition`: vanilla's
/// `vertical_gradient` with a flag for flat bedrock (paper world config
/// `environment.generate-flat-bedrock`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionallyFlatBedrock {
    pub random_name: String,
    pub true_at_and_below: VerticalAnchor,
    pub false_at_and_above: VerticalAnchor,
    pub is_roof: bool,
}

/// The condition type's id in `MATERIAL_CONDITION_TYPE` (registered by Paper's bootstrap).
pub const OPTIONALLY_FLAT_BEDROCK: &str = "paper:optionally_flat_bedrock_material_condition";

impl OptionallyFlatBedrock {
    /// The condition's fields (the object also holds `"type"`).
    pub fn decode(raw: &RawValue) -> Result<Self, DecodeError> {
        let object = object(raw)?;
        Ok(Self {
            random_name: identifier(&string(required(&object, "random_name")?)?)?,
            true_at_and_below: VerticalAnchor::decode(required(&object, "true_at_and_below")?)?,
            false_at_and_above: VerticalAnchor::decode(required(&object, "false_at_and_above")?)?,
            is_roof: boolean(required(&object, "is_roof")?)?,
        })
    }

    /// `compile`: the two bounds for a level. With flat bedrock both are the floor's lower or
    /// the roof's upper bound (minus one), so the gradient disappears.
    pub fn bounds(&self, context: WorldGenerationContext, flat_bedrock: bool) -> FlatBedrockBounds {
        let true_y = self.true_at_and_below.resolve(context);
        let false_y = self.false_at_and_above.resolve(context);
        let flat = if self.is_roof {
            false_y.max(true_y) - 1
        } else {
            false_y.min(true_y)
        };
        if flat_bedrock {
            FlatBedrockBounds {
                true_at_and_below: flat,
                false_at_and_above: flat,
            }
        } else {
            FlatBedrockBounds {
                true_at_and_below: true_y,
                false_at_and_above: false_y,
            }
        }
    }
}

/// A compiled [`OptionallyFlatBedrock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatBedrockBounds {
    pub true_at_and_below: i32,
    pub false_at_and_above: i32,
}

impl FlatBedrockBounds {
    /// The condition at a block; `random` is the level's `getOrCreateRandomFactory(random_name)`.
    pub fn test<P: PositionalRandom>(&self, random: &P, x: i32, y: i32, z: i32) -> bool {
        if y <= self.true_at_and_below {
            return true;
        }
        if y >= self.false_at_and_above {
            return false;
        }
        // Mth.map(y, true, false, 1.0, 0.0), in doubles.
        let (from, to) = (
            f64::from(self.true_at_and_below),
            f64::from(self.false_at_and_above),
        );
        let alpha = (f64::from(y) - from) / (to - from);
        let probability = 1.0 + alpha * (0.0 - 1.0);
        let mut random = random.at(crate::core::position::BlockPos::new(x, y, z));
        f64::from(random.next_float()) < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::random::{LegacyRandom, RandomSource};

    fn raw(text: &str) -> Box<RawValue> {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn anchors_resolve_with_java_wrapping_and_exclusive_alternatives() {
        let context = WorldGenerationContext::new(-64, 384, 63);
        assert_eq!(VerticalAnchor::BOTTOM.resolve(context), -64);
        assert_eq!(VerticalAnchor::TOP.resolve(context), 319);
        assert_eq!(
            VerticalAnchor::decode(&raw(r#"{"below_top":5}"#))
                .unwrap()
                .resolve(context),
            314
        );
        assert!(VerticalAnchor::decode(&raw(r#"{"absolute":1,"below_top":5}"#)).is_err());
        assert!(VerticalAnchor::decode(&raw(r#"{"absolute":5000}"#)).is_err());
    }

    #[test]
    fn paper_bedrock_floor_is_vanillas_gradient_unless_flat() {
        let floor = OptionallyFlatBedrock::decode(&raw(
            r#"{"type":"paper:optionally_flat_bedrock_material_condition","is_roof":false,
                "false_at_and_above":{"above_bottom":5},"random_name":"minecraft:bedrock_floor",
                "true_at_and_below":{"above_bottom":0}}"#,
        ))
        .unwrap();
        assert_eq!(floor.random_name, "minecraft:bedrock_floor");
        let context = WorldGenerationContext::new(-64, 384, 63);
        let gradient = floor.bounds(context, false);
        assert_eq!(
            (gradient.true_at_and_below, gradient.false_at_and_above),
            (-64, -59)
        );
        let flat = floor.bounds(context, true);
        assert_eq!(
            (flat.true_at_and_below, flat.false_at_and_above),
            (-64, -64)
        );
        let random = LegacyRandom::new(1).fork_positional();
        assert!(flat.test(&random, 0, -64, 0));
        assert!(!flat.test(&random, 0, -63, 0));
        let roof = OptionallyFlatBedrock {
            is_roof: true,
            true_at_and_below: VerticalAnchor::BelowTop(5),
            false_at_and_above: VerticalAnchor::BelowTop(0),
            random_name: "minecraft:bedrock_roof".into(),
        };
        let context = WorldGenerationContext::new(0, 128, 32);
        let flat = roof.bounds(context, true);
        assert_eq!(
            (flat.true_at_and_below, flat.false_at_and_above),
            (126, 126)
        );
        // Vanilla's gradient: draw `nextFloat() < map(y, ...)` from the position's random.
        let gradient = roof.bounds(context, false);
        let mut expected = random.at(crate::core::position::BlockPos::new(3, 124, 7));
        assert_eq!(
            gradient.test(&random, 3, 124, 7),
            f64::from(expected.next_float()) < 1.0 - (124.0 - 122.0) / (127.0 - 122.0)
        );
    }
}
