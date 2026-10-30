//! The arithmetic of density nodes, with Java's float semantics: `Math.min`/`max` (NaN and signed
//! zeros), `Math.pow` (HotSpot's, through `jmath`), `Math.round(float)`, and the separate bulk
//! forms vanilla's volume loops use (plain comparisons for min/max). Copied from Horizon's
//! `horizon-core::density` (checked against the 26.3 jar).

use crate::core::geometry::Axis;
use crate::util::jmath;

pub(crate) fn java_power(base: f64, exponent: f64) -> f64 {
    // Native libm can propagate a signaling NaN base even for exponent zero;
    // Java returns one for every base in this case.
    if exponent == 0.0 {
        1.0
    } else if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        f64::NAN
    } else {
        jmath::pow(base, exponent)
    }
}

/// `Math.min(float, float)`.
pub(crate) fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`.
pub(crate) fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        f32::from_bits(a.to_bits() & b.to_bits())
    } else if a >= b {
        a
    } else {
        b
    }
}

/// `Mth.clamp(float, float, float)`.
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// One-input operations, including the constant-operand forms the compiler makes of binary nodes.
#[derive(Clone, Copy, Debug)]
pub enum UnaryOp {
    Abs,
    Negate,
    Square,
    Cube,
    Sqrt,
    Log,
    PowerBase(f64),
    PowerExponent(f64),
    Reciprocal,
    Sign,
    Squeeze,
    LeakyRelu(f32),
    Add(f32),
    SubtractFrom(f32),
    Multiply(f32),
    DivideInto(f32),
    Minimum(f32),
    Maximum(f32),
    ConstantRangeChoice {
        min_inclusive: f32,
        max_exclusive: f32,
        inside: f32,
        outside: f32,
    },
    Clamp {
        min: f32,
        max: f32,
    },
}

// One expression definition for scalar dispatch and specialized bulk loops.
macro_rules! unary_apply {
    ($operation:expr, $value:expr, $bulk:expr) => {{
        let value = $value;
        let bulk = $bulk;
        match $operation {
            UnaryOp::Abs => value.abs(),
            UnaryOp::Negate => -value,
            UnaryOp::Square => value * value,
            UnaryOp::Cube => value * value * value,
            UnaryOp::Sqrt => (value as f64).sqrt() as f32,
            UnaryOp::Log => jmath::log(value as f64) as f32,
            UnaryOp::PowerBase(base) => java_power(base, value as f64) as f32,
            UnaryOp::PowerExponent(exponent) => java_power(value as f64, exponent) as f32,
            UnaryOp::Reciprocal => 1.0 / value,
            UnaryOp::Sign => {
                if value == 0.0 || value.is_nan() {
                    value
                } else {
                    value.signum()
                }
            }
            UnaryOp::Squeeze => {
                let v = clamp(value, -1.0, 1.0);
                v / 2.0 - v * v * v / 24.0
            }
            UnaryOp::LeakyRelu(factor) => {
                if value > 0.0 {
                    value
                } else {
                    value * factor
                }
            }
            UnaryOp::Add(right) => value + right,
            UnaryOp::SubtractFrom(left) => left - value,
            UnaryOp::Multiply(right) => value * right,
            UnaryOp::DivideInto(left) => left / value,
            UnaryOp::Minimum(right) => {
                if bulk {
                    if right < value { right } else { value }
                } else {
                    java_min(value, right)
                }
            }
            UnaryOp::Maximum(right) => {
                if bulk {
                    if right > value { right } else { value }
                } else {
                    java_max(value, right)
                }
            }
            UnaryOp::Clamp { min, max } => clamp(value, min, max),
            UnaryOp::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                if value >= min_inclusive && value < max_exclusive {
                    inside
                } else {
                    outside
                }
            }
        }
    }};
}

impl UnaryOp {
    /// `bulk` selects the volume loops' min/max (plain comparisons) over `Math.min`/`max`.
    pub(crate) fn apply(self, value: f32, bulk: bool) -> f32 {
        unary_apply!(self, value, bulk)
    }

    /// The volume form: one fixed operation per loop, so LLVM can vectorize it.
    pub(crate) fn apply_bulk(self, buffer: &mut [f32]) {
        // Specialization can reverse which NaN operand supplies the payload.
        // Keep the plain dispatch for non-finite constant operands.
        let non_finite = match self {
            Self::PowerBase(value) | Self::PowerExponent(value) => !value.is_finite(),
            Self::LeakyRelu(value)
            | Self::Add(value)
            | Self::SubtractFrom(value)
            | Self::Multiply(value)
            | Self::DivideInto(value)
            | Self::Minimum(value)
            | Self::Maximum(value) => !value.is_finite(),
            Self::Clamp { min, max } => !min.is_finite() || !max.is_finite(),
            Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                !min_inclusive.is_finite()
                    || !max_exclusive.is_finite()
                    || !inside.is_finite()
                    || !outside.is_finite()
            }
            _ => false,
        };
        if non_finite {
            for value in buffer {
                *value = self.apply(*value, true);
            }
            return;
        }
        macro_rules! apply {
            ($operation:expr) => {
                for value in buffer {
                    *value = unary_apply!($operation, *value, true);
                }
            };
        }
        match self {
            Self::Abs => apply!(Self::Abs),
            Self::Negate => apply!(Self::Negate),
            Self::Square => apply!(Self::Square),
            Self::Cube => apply!(Self::Cube),
            Self::Sqrt => apply!(Self::Sqrt),
            Self::Log => apply!(Self::Log),
            Self::PowerBase(base) => apply!(Self::PowerBase(base)),
            Self::PowerExponent(exponent) => apply!(Self::PowerExponent(exponent)),
            Self::Reciprocal => apply!(Self::Reciprocal),
            Self::Sign => apply!(Self::Sign),
            Self::Squeeze => apply!(Self::Squeeze),
            Self::LeakyRelu(factor) => apply!(Self::LeakyRelu(factor)),
            Self::Add(right) => apply!(Self::Add(right)),
            Self::SubtractFrom(left) => apply!(Self::SubtractFrom(left)),
            Self::Multiply(right) => apply!(Self::Multiply(right)),
            Self::DivideInto(left) => apply!(Self::DivideInto(left)),
            Self::Minimum(right) => apply!(Self::Minimum(right)),
            Self::Maximum(right) => apply!(Self::Maximum(right)),
            Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => apply!(Self::ConstantRangeChoice {
                min_inclusive,
                max_exclusive,
                inside,
                outside
            }),
            Self::Clamp { min, max } => apply!(Self::Clamp { min, max }),
        }
    }
}

/// Two-input operations. The min/max bounds are the right side's range ends that vanilla's
/// compiled samplers use to skip the right side in scalar sampling.
#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Minimum { right_min: f32 },
    Maximum { right_max: f32 },
}

impl BinaryOp {
    pub(crate) fn apply(self, left: f32, right: f32) -> f32 {
        match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => left / right,
            Self::Minimum { .. } => java_min(left, right),
            Self::Maximum { .. } => java_max(left, right),
        }
    }

    /// The scalar form's early exit on the left value, if any.
    pub(crate) fn short_circuit(self, left: f32) -> Option<f32> {
        match self {
            Self::Multiply | Self::Divide if left == 0.0 => Some(0.0),
            Self::Minimum { right_min } if left <= right_min => Some(left),
            Self::Maximum { right_max } if left >= right_max => Some(left),
            _ => None,
        }
    }

    pub(crate) fn apply_bulk(self, buffer: &mut [f32], right: &[f32]) {
        macro_rules! apply {
            ($operation:expr) => {
                for (left, &right) in buffer.iter_mut().zip(right) {
                    *left = ($operation)(*left, right);
                }
            };
        }
        match self {
            Self::Add => apply!(|left, right| left + right),
            Self::Subtract => apply!(|left, right| left - right),
            Self::Multiply => apply!(|left, right| left * right),
            Self::Divide => apply!(|left, right| left / right),
            Self::Minimum { .. } => apply!(|left, right| if right < left { right } else { left }),
            Self::Maximum { .. } => apply!(|left, right| if right > left { right } else { left }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoundMode {
    Floor,
    Round,
    Ceil,
    Truncate,
}

impl RoundMode {
    pub(crate) fn integer(self, value: f32) -> f32 {
        match self {
            Self::Floor => (value as f64).floor() as f32,
            Self::Ceil => (value as f64).ceil() as f32,
            Self::Truncate => {
                if value > 0.0 {
                    (value as f64).floor() as f32
                } else {
                    (value as f64).ceil() as f32
                }
            }
            Self::Round => {
                // Math.round(float): ties go toward positive infinity, NaNs
                // become zero, and the result saturates to Java's int domain.
                let bits = value.to_bits() as i32;
                let shift = 149 - ((bits >> 23) & 0xff);
                let rounded = if (0..32).contains(&shift) {
                    let mut significand = (bits & 0x7f_ffff) | 0x80_0000;
                    if bits < 0 {
                        significand = -significand;
                    }
                    ((significand >> shift) + 1) >> 1
                } else {
                    value as i32
                };
                rounded as f32
            }
        }
    }

    pub(crate) fn apply(self, input: f32, multiple: f32) -> f32 {
        if multiple == 0.0 {
            input
        } else {
            self.integer(input / multiple) * multiple
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DistanceMetric {
    Euclidean,
    EuclideanSquared,
    Manhattan,
    Chebyshev,
}

impl DistanceMetric {
    pub fn compute2(self, x: f32, y: f32) -> f32 {
        match self {
            Self::Euclidean => ((x * x + y * y) as f64).sqrt() as f32,
            Self::EuclideanSquared => x * x + y * y,
            Self::Manhattan => x.abs() + y.abs(),
            Self::Chebyshev => java_max(x.abs(), y.abs()),
        }
    }
    pub fn compute3(self, x: f32, y: f32, z: f32) -> f32 {
        match self {
            Self::Euclidean => ((x * x + y * y + z * z) as f64).sqrt() as f32,
            Self::EuclideanSquared => x * x + y * y + z * z,
            Self::Manhattan => x.abs() + y.abs() + z.abs(),
            Self::Chebyshev => java_max(java_max(x.abs(), y.abs()), z.abs()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TilingMode {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

/// `GradientFunction`'s compiled form: a linear ramp along one axis.
#[derive(Clone, Copy, Debug)]
pub struct Gradient {
    pub(crate) axis: Axis,
    tiling: TilingMode,
    from: i32,
    to: i32,
    period: i32,
    value: f32,
    factor: f32,
}

impl Gradient {
    /// `from != to` (the codec refuses equal coordinates), so the period is never zero.
    pub fn new(
        axis: Axis,
        tiling: TilingMode,
        from: i32,
        to: i32,
        from_value: f32,
        to_value: f32,
    ) -> Self {
        let period = to.wrapping_sub(from);
        Self {
            axis,
            tiling,
            from,
            to,
            period,
            value: from_value,
            factor: (to_value - from_value) / period as f32,
        }
    }

    pub fn compute(&self, coordinate: i32) -> f32 {
        let distance = match self.tiling {
            TilingMode::ClampToEdge => coordinate
                .clamp(self.from.min(self.to), self.from.max(self.to))
                .wrapping_sub(self.from),
            TilingMode::Repeat | TilingMode::MirroredRepeat => {
                let delta = coordinate.wrapping_sub(self.from);
                // Java floorDiv/floorMod wrap MIN / -1, and round toward -infinity.
                let mut cycle = delta.wrapping_div(self.period);
                let mut remainder = delta.wrapping_rem(self.period);
                if remainder != 0 && (delta ^ self.period) < 0 {
                    cycle = cycle.wrapping_sub(1);
                    remainder = remainder.wrapping_add(self.period);
                }
                if self.tiling == TilingMode::MirroredRepeat && cycle & 1 != 0 {
                    self.period.wrapping_sub(remainder)
                } else {
                    remainder
                }
            }
        };
        self.value + distance as f32 * self.factor
    }
}

/// Coordinates fixed by a compiled single-axis or combined XZ slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliceAxes {
    X(i32),
    Y(i32),
    Z(i32),
    Xz { x: i32, z: i32 },
}

impl SliceAxes {
    pub(crate) fn coordinates(self) -> [Option<i32>; 3] {
        match self {
            Self::X(x) => [Some(x), None, None],
            Self::Y(y) => [None, Some(y), None],
            Self::Z(z) => [None, None, Some(z)],
            Self::Xz { x, z } => [Some(x), None, Some(z)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    fn scalar_reference(operation: UnaryOp, value: f32) -> f32 {
        operation.apply(value, true)
    }

    fn edge_values() -> Vec<f32> {
        let mut values: Vec<f32> = [
            0,
            0x8000_0000,
            1,
            0x8000_0001,
            0x007f_ffff,
            0x0080_0000,
            0x3f00_0000,
            0xbf00_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7fa0_0001,
            0xffc0_1234,
        ]
        .into_iter()
        .map(f32::from_bits)
        .collect();
        let mut bits = 0x484f_5249u32;
        for _ in 0..4096 {
            bits ^= bits << 13;
            bits ^= bits >> 17;
            bits ^= bits << 5;
            values.push(f32::from_bits(bits));
        }
        values
    }

    #[test]
    fn fixed_operation_loops_preserve_float_bits() {
        let mut operations = vec![
            UnaryOp::Abs,
            UnaryOp::Negate,
            UnaryOp::Square,
            UnaryOp::Cube,
            UnaryOp::Sqrt,
            UnaryOp::Log,
            UnaryOp::Reciprocal,
            UnaryOp::Sign,
            UnaryOp::Squeeze,
        ];
        for factor in [0.0, -0.0, 0.5, -1.0, f32::INFINITY, f32::NAN] {
            operations.extend([
                UnaryOp::LeakyRelu(factor),
                UnaryOp::Add(factor),
                UnaryOp::SubtractFrom(factor),
                UnaryOp::Multiply(factor),
                UnaryOp::DivideInto(factor),
                UnaryOp::Minimum(factor),
                UnaryOp::Maximum(factor),
                UnaryOp::Clamp {
                    min: -factor,
                    max: factor,
                },
                UnaryOp::ConstantRangeChoice {
                    min_inclusive: -factor,
                    max_exclusive: factor,
                    inside: -0.0,
                    outside: 17.0,
                },
            ]);
        }
        for exponent in [0.0, -0.0, 0.5, -1.0, 2.0, 3.0, f64::INFINITY, f64::NAN] {
            operations.extend([
                UnaryOp::PowerBase(exponent),
                UnaryOp::PowerExponent(exponent),
            ]);
        }
        let values = edge_values();
        for operation in operations {
            let mut actual = values.clone();
            operation.apply_bulk(&mut actual);
            for (i, (&input, actual)) in values.iter().zip(actual).enumerate() {
                let expected = scalar_reference(operation, input);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "{operation:?}, index {i}, input {input:?}"
                );
            }
        }
    }

    #[test]
    fn scalar_and_bulk_min_differ_only_where_java_does() {
        // Math.min(NaN, 1) is NaN; the bulk loop's `right < left ? right : left` keeps the left.
        assert!(UnaryOp::Minimum(1.0).apply(f32::NAN, false).is_nan());
        assert!(UnaryOp::Minimum(1.0).apply(f32::NAN, true).is_nan());
        assert_eq!(UnaryOp::Minimum(f32::NAN).apply(1.0, true), 1.0);
        assert!(UnaryOp::Minimum(f32::NAN).apply(1.0, false).is_nan());
        assert_eq!(java_min(0.0, -0.0).to_bits(), (-0.0f32).to_bits());
        assert_eq!(java_max(-0.0, 0.0).to_bits(), 0);
        assert_eq!(RoundMode::Round.integer(-2.5), -2.0);
        assert_eq!(RoundMode::Round.integer(2.5), 3.0);
        assert_eq!(RoundMode::Round.integer(f32::NAN), 0.0);
        assert_eq!(RoundMode::Truncate.integer(-1.5), -1.0);
    }

    #[test]
    fn gradients_tile_with_java_floor_division() {
        let ramp = Gradient::new(Axis::Y, TilingMode::Repeat, 0, 4, 0.0, 1.0);
        assert_eq!(ramp.compute(5), 0.25);
        assert_eq!(ramp.compute(-1), 0.75);
        let mirrored = Gradient::new(Axis::Y, TilingMode::MirroredRepeat, 0, 4, 0.0, 1.0);
        assert_eq!(mirrored.compute(5), 0.75);
        assert_eq!(mirrored.compute(-1), 0.25);
        let clamped = Gradient::new(Axis::Y, TilingMode::ClampToEdge, 4, 0, 1.0, 0.0);
        assert_eq!(clamped.compute(100), 1.0);
        assert_eq!(clamped.compute(-100), 0.0);
    }
}
