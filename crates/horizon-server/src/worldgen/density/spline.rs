//! `CubicSpline`: the definition (coordinates are graph functions) and its compiled form (a flat
//! arena of multipoint nodes, coordinates as slots of the spline's coordinate list).

use super::interval::{Interval, IntervalError};
use super::ops::{java_max, java_min};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SplineError {
    #[error("a multipoint spline requires at least one point")]
    EmptyPoints,
    #[error("spline nesting exceeds 256 levels")]
    NestingTooDeep,
    #[error("a compiled constant spline cannot be NaN")]
    NanConstant,
}

#[derive(Clone, Debug)]
pub struct SplinePoint<C> {
    pub location: f32,
    pub value: CubicSpline<C>,
    pub derivative: f32,
}

#[derive(Clone, Debug)]
pub struct MultipointSpline<C> {
    pub(super) coordinate: C,
    pub(super) points: Vec<SplinePoint<C>>,
    depth: usize,
}

/// An immutable spline definition. Point order is kept, including NaNs and equal locations; the
/// codec's ordering rules belong to decoding.
#[derive(Clone, Debug)]
pub enum CubicSpline<C> {
    Constant(f32),
    Multipoint(Box<MultipointSpline<C>>),
}

impl<C: Copy> CubicSpline<C> {
    pub fn multipoint(coordinate: C, points: Vec<SplinePoint<C>>) -> Result<Self, SplineError> {
        let depth = points
            .iter()
            .map(|p| match &p.value {
                Self::Constant(_) => 0,
                Self::Multipoint(node) => node.depth,
            })
            .max()
            .ok_or(SplineError::EmptyPoints)?
            + 1;
        if depth > 256 {
            return Err(SplineError::NestingTooDeep);
        }
        Ok(Self::Multipoint(Box::new(MultipointSpline {
            coordinate,
            points,
            depth,
        })))
    }

    /// Vanilla's conservative range analysis, coordinate ranges supplied by the graph. Child
    /// ranges are evaluated in reference order (so errors surface in the same order).
    pub fn try_range<E: From<IntervalError>>(
        &self,
        coordinate: &mut impl FnMut(C) -> Result<Interval, E>,
    ) -> Result<Interval, E> {
        let node = match self {
            Self::Constant(value) => return Interval::exact(*value).map_err(E::from),
            Self::Multipoint(node) => node,
        };
        let input = coordinate(node.coordinate)?;
        if input.is_nai() {
            return Ok(input);
        }
        let mut minimum = f32::INFINITY;
        let mut maximum = f32::NEG_INFINITY;
        let first = &node.points[0];
        let last = node.points.last().expect("a multipoint spline has points");
        let extend = |point: &SplinePoint<C>, input: f32, value: f32| {
            if point.derivative == 0.0 {
                value
            } else {
                value + point.derivative * (input - point.location)
            }
        };
        if input.min() < first.location {
            let value = first.value.try_range(coordinate)?;
            let low = extend(first, input.min(), value.min());
            let high = extend(first, input.min(), value.max());
            minimum = java_min(minimum, java_min(low, high));
            maximum = java_max(maximum, java_max(low, high));
        }
        if input.max() > last.location {
            let value = last.value.try_range(coordinate)?;
            let low = extend(last, input.max(), value.min());
            let high = extend(last, input.max(), value.max());
            minimum = java_min(minimum, java_min(low, high));
            maximum = java_max(maximum, java_max(low, high));
        }
        let ranges = node
            .points
            .iter()
            .map(|point| point.value.try_range(coordinate))
            .collect::<Result<Vec<_>, _>>()?;
        for range in &ranges {
            minimum = java_min(minimum, range.min());
            maximum = java_max(maximum, range.max());
        }
        for (points, ranges) in node.points.windows(2).zip(ranges.windows(2)) {
            let width = points[1].location - points[0].location;
            let left = ranges[0];
            let right = ranges[1];
            let derivative_left = points[0].derivative;
            let derivative_right = points[1].derivative;
            if derivative_left == 0.0 && derivative_right == 0.0 {
                continue;
            }
            let tangent_left = derivative_left * width;
            let tangent_right = derivative_right * width;
            let value_min = java_min(left.min(), right.min());
            let value_max = java_max(left.max(), right.max());
            let low_left = tangent_left - right.max() + left.min();
            let high_left = tangent_left - right.min() + left.max();
            let low_right = -tangent_right + right.min() - left.max();
            let high_right = -tangent_right + right.max() - left.min();
            minimum = java_min(minimum, value_min + 0.25 * java_min(low_left, low_right));
            maximum = java_max(maximum, value_max + 0.25 * java_max(high_left, high_right));
        }
        Interval::new(minimum, maximum).map_err(E::from)
    }

    /// The coordinates in traversal order: a node's coordinate, then its points' values.
    pub(super) fn coordinates_mut(&mut self) -> Vec<&mut C> {
        let mut out = Vec::new();
        self.collect_coordinates(&mut out);
        out
    }
    fn collect_coordinates<'a>(&'a mut self, out: &mut Vec<&'a mut C>) {
        if let Self::Multipoint(node) = self {
            out.push(&mut node.coordinate);
            for point in &mut node.points {
                point.value.collect_coordinates(out);
            }
        }
    }

    /// Appends a structural key (for the graph's interning) with `id` mapping coordinates.
    pub(super) fn append_key(&self, key: &mut Vec<u64>, id: &impl Fn(C) -> u64) {
        let bits = |v: f32| u64::from(if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() });
        match self {
            Self::Constant(value) => key.extend([0, bits(*value)]),
            Self::Multipoint(node) => {
                key.extend([1, id(node.coordinate), node.points.len() as u64]);
                for point in &node.points {
                    key.extend([bits(point.location), bits(point.derivative)]);
                    point.value.append_key(key, id);
                }
            }
        }
    }
}

/// A compiled spline value: a constant or a multipoint node of the same [`SplineProgram`].
#[derive(Clone, Copy, Debug)]
enum SplineValue {
    Constant(f32),
    Node(u32),
}

#[derive(Clone, Debug)]
struct ProgramPoint {
    location: f32,
    derivative: f32,
    value: SplineValue,
}

#[derive(Clone, Debug)]
struct ProgramNode {
    /// The coordinate's slot (dense, in first-use order).
    coordinate: u16,
    points: Box<[ProgramPoint]>,
}

/// A compiled spline: multipoint nodes in a flat list, coordinates as dense slots (equal
/// coordinate functions share a slot, so each is evaluated once per position).
#[derive(Clone, Debug)]
pub struct SplineProgram {
    root: SplineValue,
    nodes: Vec<ProgramNode>,
}

impl SplineProgram {
    /// `slot` maps a definition's coordinate to its slot (in first-use order).
    pub(super) fn compile<C: Copy>(
        spline: &CubicSpline<C>,
        slot: &mut impl FnMut(C) -> u16,
    ) -> Result<Self, SplineError> {
        if matches!(spline, CubicSpline::Constant(value) if value.is_nan()) {
            return Err(SplineError::NanConstant);
        }
        let mut program = Self {
            root: SplineValue::Constant(0.0),
            nodes: Vec::new(),
        };
        program.root = program.add(spline, slot);
        Ok(program)
    }

    fn add<C: Copy>(
        &mut self,
        spline: &CubicSpline<C>,
        slot: &mut impl FnMut(C) -> u16,
    ) -> SplineValue {
        match spline {
            CubicSpline::Constant(value) => SplineValue::Constant(*value),
            CubicSpline::Multipoint(node) => {
                let coordinate = slot(node.coordinate);
                let points = node
                    .points
                    .iter()
                    .map(|point| ProgramPoint {
                        location: point.location,
                        derivative: point.derivative,
                        value: self.add(&point.value, slot),
                    })
                    .collect();
                self.nodes.push(ProgramNode { coordinate, points });
                SplineValue::Node((self.nodes.len() - 1) as u32)
            }
        }
    }

    pub fn constant(&self) -> Option<f32> {
        match self.root {
            SplineValue::Constant(value) => Some(value),
            SplineValue::Node(_) => None,
        }
    }

    /// The spline at the position whose coordinate values `coordinate` gives (by slot).
    pub fn sample(&self, coordinate: &mut impl FnMut(usize) -> f32) -> f32 {
        self.sample_value(self.root, coordinate)
    }

    fn sample_value(&self, value: SplineValue, coordinate: &mut impl FnMut(usize) -> f32) -> f32 {
        let node = match value {
            SplineValue::Constant(value) => return value,
            SplineValue::Node(index) => &self.nodes[index as usize],
        };
        let input = coordinate(usize::from(node.coordinate));
        let points = &node.points;
        // Vanilla's binarySearch finds the first strictly greater location; kept even for
        // unsorted or NaN locations.
        let mut first = 0;
        let mut remaining = points.len();
        while remaining != 0 {
            let half = remaining / 2;
            let middle = first + half;
            if input < points[middle].location {
                remaining = half;
            } else {
                first = middle + 1;
                remaining -= half + 1;
            }
        }
        if first == 0 || first == points.len() {
            let point = &points[first.saturating_sub(1)];
            let value = self.sample_value(point.value, coordinate);
            return if point.derivative == 0.0 {
                value
            } else {
                value + point.derivative * (input - point.location)
            };
        }
        let left = &points[first - 1];
        let right = &points[first];
        let width = right.location - left.location;
        let alpha = (input - left.location) / width;
        let left_value = self.sample_value(left.value, coordinate);
        let right_value = self.sample_value(right.value, coordinate);
        let left_correction = left.derivative * width - (right_value - left_value);
        let right_correction = -right.derivative * width + (right_value - left_value);
        let linear = left_value + alpha * (right_value - left_value);
        let correction = left_correction + alpha * (right_correction - left_correction);
        linear + (alpha * (1.0 - alpha)) * correction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(location: f32, value: CubicSpline<u8>, derivative: f32) -> SplinePoint<u8> {
        SplinePoint {
            location,
            value,
            derivative,
        }
    }

    #[test]
    fn hermite_segments_and_linear_extension() {
        let inner = CubicSpline::multipoint(
            1u8,
            vec![
                point(-1.0, CubicSpline::Constant(0.0), 0.0),
                point(1.0, CubicSpline::Constant(2.0), 0.0),
            ],
        )
        .unwrap();
        let spline = CubicSpline::multipoint(
            0u8,
            vec![
                point(0.0, CubicSpline::Constant(1.0), 1.0),
                point(1.0, inner, 0.0),
            ],
        )
        .unwrap();
        let mut slots = Vec::new();
        let program = SplineProgram::compile(&spline, &mut |c| {
            slots.push(c);
            (slots.len() - 1) as u16
        })
        .unwrap();
        assert_eq!(slots, [0, 1]);
        // Below the first point: the value extended by its derivative.
        assert_eq!(program.sample(&mut |slot| [-1.0, 0.0][slot]), 0.0);
        // Between points 0 and 1, the inner spline at 0.0 is 1.0: linear part only at alpha 0.5.
        let value = program.sample(&mut |slot| [0.5, 0.0][slot]);
        let (width, left, right) = (1.0f32, 1.0f32, 1.0f32);
        let left_correction = 1.0 * width - (right - left);
        let right_correction = -0.0 * width + (right - left);
        let expected = left
            + 0.5 * (right - left)
            + (0.5 * (1.0 - 0.5)) * (left_correction + 0.5 * (right_correction - left_correction));
        assert_eq!(value, expected);
        assert!(matches!(
            SplineProgram::compile(&CubicSpline::<u8>::Constant(f32::NAN), &mut |_| 0),
            Err(SplineError::NanConstant)
        ));
        assert!(matches!(
            CubicSpline::<u8>::multipoint(0, Vec::new()),
            Err(SplineError::EmptyPoints)
        ));
    }
}
