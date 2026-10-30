//! The compiled form of a level's density functions: one [`DensityProgram`] holding every node in
//! a `Vec` (children by index), the seeded noises it samples, and its splines. Evaluation is a
//! `match` per node; per-evaluation state (scratch buffers, cache cells, the chunk's world fields)
//! lives in a caller-owned [`DensityContext`], so one program is shared read-only by every worker.
//!
//! Each node keeps vanilla 26.3's two sampling paths (`sampleValue` for one position,
//! `sampleVolume` for a strided volume) with their own arithmetic, as Horizon's samplers did.

use super::beardifier::Beardifier;
use super::end_islands::EndIslands;
use super::ops::{BinaryOp, DistanceMetric, Gradient, RoundMode, SliceAxes, UnaryOp, java_power};
use super::spline::SplineProgram;
use super::{NodeId, WorldField};
use crate::core::geometry::Axis;
use crate::core::position::BlockPos;
use crate::worldgen::noise::{DensityVolume, NoiseStack};

/// A seeded noise of a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NoiseRef(pub(super) u32);

/// A `lerp` endpoint: literal constants skip a scratch buffer in volumes.
#[derive(Clone, Copy, Debug)]
pub(super) enum Operand {
    Constant(f32),
    Node(NodeId),
}

#[derive(Clone, Debug)]
pub(super) enum Node {
    Constant(f32),
    Gradient(Gradient),
    Noise {
        noise: NoiseRef,
        xz_scale: f64,
        y_scale: f64,
    },
    /// A noise with coordinate shifts; `shift_y` absent is vanilla's XZ form (no `+ 0.0`, which
    /// would change signed zeros).
    ShiftedNoise {
        noise: NoiseRef,
        xz_scale: f64,
        y_scale: f64,
        shift_x: NodeId,
        shift_y: Option<NodeId>,
        shift_z: NodeId,
    },
    /// `shift_b`: the noise at (z / 4, x / 4, 0), times 4.
    ShiftB(NoiseRef),
    Unary {
        input: NodeId,
        op: UnaryOp,
    },
    Binary {
        left: NodeId,
        right: NodeId,
        op: BinaryOp,
    },
    Power {
        base: NodeId,
        exponent: NodeId,
    },
    /// `multiple` absent is the literal-one form.
    Round {
        mode: RoundMode,
        input: NodeId,
        multiple: Option<NodeId>,
    },
    RangeChoice {
        input: NodeId,
        min_inclusive: f32,
        max_exclusive: f32,
        inside: NodeId,
        outside: NodeId,
    },
    Lerp {
        alpha: NodeId,
        first: Operand,
        second: Operand,
    },
    IntervalSelect {
        input: NodeId,
        thresholds: Box<[f32]>,
        branches: Box<[NodeId]>,
    },
    Slice {
        input: NodeId,
        axes: SliceAxes,
    },
    /// A `cache` marker: its value is kept per context in cell `slot`.
    Cache {
        slot: u32,
        input: NodeId,
    },
    Interpolated {
        input: NodeId,
        cell: [i32; 3],
        inverse: [f32; 3],
    },
    /// `find_top_surface`, always reached through a `Slice` at Y 0 (one layer per column).
    TopSurface {
        density: NodeId,
        upper: NodeId,
        lower: i32,
        cell_height: i32,
    },
    Spline {
        spline: u32,
        coordinates: Box<[NodeId]>,
    },
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
    EndIslands(Box<EndIslands>),
    Field(WorldField),
}

/// A level's compiled density functions (see the module docs). Immutable once compiled.
#[derive(Clone, Debug)]
pub struct DensityProgram {
    pub(super) nodes: Vec<Node>,
    pub(super) noises: Vec<NoiseStack>,
    pub(super) splines: Vec<SplineProgram>,
    pub(super) cache_count: usize,
}

/// What a chunk binds to the context-bound fields (`blend_alpha`, `blend_offset`, `beardifier`).
/// Old-world blending is not ported (26.3 worlds have no chunks to blend), so the blend fields
/// always take their defaults.
#[derive(Clone, Debug, Default)]
pub struct WorldFields {
    pub beardifier: Option<Beardifier>,
}

#[derive(Debug)]
struct CacheCell {
    volume: Option<DensityVolume>,
    buffer: Vec<f32>,
    value_key: i64,
    value: f32,
}

impl Default for CacheCell {
    fn default() -> Self {
        Self {
            volume: None,
            buffer: Vec::new(),
            value_key: 0,
            value: f32::NAN,
        }
    }
}

/// A worker's evaluation state for one program: scratch buffers, `cache` cells (when caching is
/// on, as in a `NoiseChunk`) and the bound world fields. Owned by the worker, never shared.
#[derive(Debug, Default)]
pub struct DensityContext {
    scratch: Vec<Vec<f32>>,
    caches: Option<Vec<CacheCell>>,
    fields: WorldFields,
}

impl DensityContext {
    /// `SamplerContext.EMPTY_UNCACHED`'s behaviour: `cache` nodes pass through.
    pub fn uncached() -> Self {
        Self::default()
    }
    /// A context with `cache` cells (`SamplerContext.builder().enableCaches()`).
    pub fn cached() -> Self {
        Self {
            caches: Some(Vec::new()),
            ..Self::default()
        }
    }
    /// Starts an independent evaluation (another chunk): cached values and fields are dropped,
    /// scratch buffers kept.
    pub fn reset(&mut self) {
        if let Some(caches) = &mut self.caches {
            caches.clear();
        }
        self.fields = WorldFields::default();
    }
    /// Binds a chunk's fields, dropping cached values computed with the previous ones.
    pub fn set_fields(&mut self, fields: WorldFields) {
        if let Some(caches) = &mut self.caches {
            caches.clear();
        }
        self.fields = fields;
    }

    fn with_buffer<T>(
        &mut self,
        size: usize,
        sample: impl FnOnce(&mut Self, &mut [f32]) -> T,
    ) -> T {
        // Every node writes its whole output, so a reused buffer's old values never show.
        let mut buffer = self.scratch.pop().unwrap_or_default();
        buffer.resize(size, 0.0);
        let result = sample(self, &mut buffer);
        self.scratch.push(buffer);
        result
    }

    fn cache_cell(&mut self, slot: u32, count: usize) -> Option<&mut CacheCell> {
        let caches = self.caches.as_mut()?;
        if caches.len() < count {
            caches.resize_with(count, CacheCell::default);
        }
        Some(&mut caches[slot as usize])
    }
}

impl DensityProgram {
    /// The number of `cache` cells a context holds for this program.
    pub fn cache_count(&self) -> usize {
        self.cache_count
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// `sampleValue`: the node's value at one block position.
    pub fn sample(&self, context: &mut DensityContext, id: NodeId, position: [i32; 3]) -> f32 {
        let [x, y, z] = position;
        match &self.nodes[id.index()] {
            Node::Constant(value) => *value,
            Node::Gradient(gradient) => gradient.compute(position[gradient.axis as usize]),
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => self.noise(*noise).get3(
                f64::from(x) * xz_scale,
                f64::from(y) * y_scale,
                f64::from(z) * xz_scale,
            ),
            Node::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let sx =
                    f64::from(x) * xz_scale + f64::from(self.sample(context, *shift_x, position));
                let mut sy = f64::from(y) * y_scale;
                if let Some(shift) = shift_y {
                    sy += f64::from(self.sample(context, *shift, position));
                }
                let sz =
                    f64::from(z) * xz_scale + f64::from(self.sample(context, *shift_z, position));
                self.noise(*noise).get3(sx, sy, sz)
            }
            Node::ShiftB(noise) => {
                self.noise(*noise)
                    .get3(f64::from(z) * 0.25, f64::from(x) * 0.25, 0.0)
                    * 4.0
            }
            Node::Unary { input, op } => op.apply(self.sample(context, *input, position), false),
            Node::Binary { left, right, op } => {
                let left = self.sample(context, *left, position);
                match op.short_circuit(left) {
                    Some(value) => value,
                    None => op.apply(left, self.sample(context, *right, position)),
                }
            }
            Node::Power { base, exponent } => {
                let base = self.sample(context, *base, position);
                let exponent = self.sample(context, *exponent, position);
                java_power(f64::from(base), f64::from(exponent)) as f32
            }
            Node::Round {
                mode,
                input,
                multiple,
            } => {
                let input = self.sample(context, *input, position);
                match multiple {
                    Some(multiple) => mode.apply(input, self.sample(context, *multiple, position)),
                    None => mode.integer(input),
                }
            }
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                let value = self.sample(context, *input, position);
                if value >= *min_inclusive && value < *max_exclusive {
                    self.sample(context, *inside, position)
                } else {
                    self.sample(context, *outside, position)
                }
            }
            Node::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = self.sample(context, *alpha, position);
                if alpha == 0.0 {
                    return self.operand(context, *first, position);
                }
                if alpha == 1.0 {
                    return self.operand(context, *second, position);
                }
                let first = self.operand(context, *first, position);
                let second = self.operand(context, *second, position);
                interpolate(alpha, first, second)
            }
            Node::IntervalSelect {
                input,
                thresholds,
                branches,
            } => {
                let value = self.sample(context, *input, position);
                self.sample(context, branches[select(thresholds, value)], position)
            }
            Node::Slice { input, axes } => {
                let mut position = position;
                for (axis, fixed) in axes.coordinates().into_iter().enumerate() {
                    if let Some(coordinate) = fixed {
                        position[axis] = coordinate;
                    }
                }
                self.sample(context, *input, position)
            }
            Node::Cache { slot, input } => self.sample_cached(context, *slot, *input, position),
            Node::Interpolated {
                input,
                cell,
                inverse: _,
            } => self.sample_interpolated(context, *input, *cell, position),
            Node::TopSurface {
                density,
                upper,
                lower,
                cell_height,
            } => {
                let upper = self.sample(context, *upper, position);
                self.find_surface(context, *density, *lower, *cell_height, x, z, upper) as f32
            }
            Node::Spline {
                spline,
                coordinates,
            } => self.sample_spline(context, *spline, coordinates, position),
            Node::DistanceToPoint { point, metric } => {
                let delta: [f32; 3] =
                    std::array::from_fn(|i| point[i].wrapping_sub(position[i]) as f32);
                metric.compute3(delta[0], delta[1], delta[2])
            }
            Node::EndIslands(islands) => islands.density(x, z),
            Node::Field(field) => match (field, &context.fields.beardifier) {
                (WorldField::Beardifier, Some(beardifier)) => beardifier.sample(position),
                _ => field.fallback(),
            },
        }
    }

    /// `sampleVolume`: the node over `volume` into `out` (Z, X, Y order, Y innermost), which must
    /// hold exactly `volume.size()` values.
    pub fn fill(
        &self,
        context: &mut DensityContext,
        id: NodeId,
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        assert_eq!(out.len(), volume.size(), "a volume-sized buffer");
        self.fill_exact(context, id, out, volume);
    }

    fn fill_exact(
        &self,
        context: &mut DensityContext,
        id: NodeId,
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        match &self.nodes[id.index()] {
            Node::Constant(value) => out.fill(*value),
            Node::Gradient(gradient) => fill_gradient(gradient, out, volume),
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                out.fill(0.0);
                self.noise(*noise)
                    .add_to_volume(out, volume, *xz_scale, *y_scale, 1.0)
                    .expect("a volume-sized buffer");
            }
            Node::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                self.fill_exact(context, *shift_x, out, volume);
                let noise = self.noise(*noise);
                let shifted = |out: &mut [f32], shift_y: Option<&[f32]>, shift_z: &[f32]| {
                    visit(volume, |index, [x, y, z]| {
                        let sx = f64::from(x) * xz_scale + f64::from(out[index]);
                        let mut sy = f64::from(y) * y_scale;
                        if let Some(shift) = shift_y {
                            sy += f64::from(shift[index]);
                        }
                        let sz = f64::from(z) * xz_scale + f64::from(shift_z[index]);
                        out[index] = noise.get3(sx, sy, sz);
                    });
                };
                match shift_y {
                    Some(shift_y) => context.with_buffer(volume.size(), |context, ys| {
                        self.fill_exact(context, *shift_y, ys, volume);
                        context.with_buffer(volume.size(), |context, zs| {
                            self.fill_exact(context, *shift_z, zs, volume);
                            shifted(out, Some(ys), zs);
                        });
                    }),
                    None => context.with_buffer(volume.size(), |context, zs| {
                        self.fill_exact(context, *shift_z, zs, volume);
                        shifted(out, None, zs);
                    }),
                }
            }
            Node::ShiftB(noise) => {
                let [sx, sy, sz] = volume.sizes();
                let [mx, _, mz] = volume.minimum();
                let [dx, _, dz] = volume.steps();
                let transposed =
                    DensityVolume::new([sz as i32, sx as i32, 1], [mz, mx, 0], [dz, dx, 1])
                        .expect("the transposed plane of a valid volume");
                context.with_buffer(transposed.size(), |_, plane| {
                    plane.fill(0.0);
                    self.noise(*noise)
                        .add_to_volume(plane, transposed, 0.25, 0.25, 4.0)
                        .expect("a plane-sized buffer");
                    for z in 0..sz {
                        for x in 0..sx {
                            let start = (x + z * sx) * sy;
                            out[start..start + sy].fill(plane[x + z * sx]);
                        }
                    }
                });
            }
            Node::Unary { input, op } => {
                self.fill_exact(context, *input, out, volume);
                op.apply_bulk(out);
            }
            Node::Binary { left, right, op } => {
                self.fill_exact(context, *left, out, volume);
                context.with_buffer(volume.size(), |context, rights| {
                    self.fill_exact(context, *right, rights, volume);
                    op.apply_bulk(out, rights);
                });
            }
            Node::Power { base, exponent } => {
                self.fill_exact(context, *base, out, volume);
                context.with_buffer(volume.size(), |context, exponents| {
                    self.fill_exact(context, *exponent, exponents, volume);
                    for (base, exponent) in out.iter_mut().zip(exponents.iter()) {
                        *base = java_power(f64::from(*base), f64::from(*exponent)) as f32;
                    }
                });
            }
            Node::Round {
                mode,
                input,
                multiple,
            } => {
                self.fill_exact(context, *input, out, volume);
                match multiple {
                    Some(multiple) => context.with_buffer(volume.size(), |context, multiples| {
                        self.fill_exact(context, *multiple, multiples, volume);
                        for (input, multiple) in out.iter_mut().zip(multiples.iter()) {
                            *input = mode.apply(*input, *multiple);
                        }
                    }),
                    None => out.iter_mut().for_each(|v| *v = mode.integer(*v)),
                }
            }
            Node::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                inside,
                outside,
            } => {
                // Vanilla evaluates the in-range branch before the selector in bulk.
                self.fill_exact(context, *inside, out, volume);
                context.with_buffer(volume.size(), |context, inputs| {
                    self.fill_exact(context, *input, inputs, volume);
                    context.with_buffer(volume.size(), |context, outsides| {
                        self.fill_exact(context, *outside, outsides, volume);
                        for i in 0..out.len() {
                            if !(inputs[i] >= *min_inclusive && inputs[i] < *max_exclusive) {
                                out[i] = outsides[i];
                            }
                        }
                    });
                });
            }
            Node::Lerp {
                alpha,
                first,
                second,
            } => {
                self.fill_exact(context, *alpha, out, volume);
                self.with_operand(context, *first, volume, |context, first| {
                    self.with_operand(context, *second, volume, |_, second| {
                        for (index, alpha) in out.iter_mut().enumerate() {
                            *alpha = interpolate(*alpha, first.get(index), second.get(index));
                        }
                    });
                });
            }
            Node::IntervalSelect {
                input,
                thresholds,
                branches,
            } => {
                self.fill_exact(context, *input, out, volume);
                let size = volume.size();
                // One contiguous scratch for every branch; each still gets a volume-sized slice.
                context.with_buffer(size * branches.len(), |context, values| {
                    for (branch, output) in branches.iter().zip(values.chunks_exact_mut(size)) {
                        self.fill_exact(context, *branch, output, volume);
                    }
                    // Every selection waits until all branch volumes are done.
                    for (index, input) in out.iter_mut().enumerate() {
                        *input = values[select(thresholds, *input) * size + index];
                    }
                });
            }
            Node::Slice { input, axes } => self.fill_slice(context, *input, *axes, out, volume),
            Node::Cache { slot, input } => self.fill_cached(context, *slot, *input, out, volume),
            Node::Interpolated {
                input,
                cell,
                inverse,
            } => self.fill_interpolated(context, *input, *cell, *inverse, out, volume),
            Node::TopSurface {
                density,
                upper,
                lower,
                cell_height,
            } => {
                debug_assert_eq!(volume.sizes()[1], 1, "a surface search is sliced at Y 0");
                self.fill_exact(context, *upper, out, volume);
                visit(volume, |index, [x, _, z]| {
                    out[index] = self.find_surface(
                        context,
                        *density,
                        *lower,
                        *cell_height,
                        x,
                        z,
                        out[index],
                    ) as f32;
                });
            }
            Node::Spline {
                spline,
                coordinates,
            } => self.fill_spline(context, *spline, coordinates, out, volume),
            Node::DistanceToPoint { .. } => {
                visit(volume, |index, position| {
                    out[index] = self.sample(context, id, position);
                });
            }
            Node::EndIslands(islands) => islands.fill(out, volume),
            Node::Field(field) => match (field, &context.fields.beardifier) {
                (WorldField::Beardifier, Some(beardifier)) => beardifier.fill(out, volume),
                _ => out.fill(field.fallback()),
            },
        }
    }

    fn noise(&self, noise: NoiseRef) -> &NoiseStack {
        &self.noises[noise.0 as usize]
    }

    fn operand(&self, context: &mut DensityContext, operand: Operand, position: [i32; 3]) -> f32 {
        match operand {
            Operand::Constant(value) => value,
            Operand::Node(node) => self.sample(context, node, position),
        }
    }

    fn with_operand(
        &self,
        context: &mut DensityContext,
        operand: Operand,
        volume: DensityVolume,
        use_values: impl FnOnce(&mut DensityContext, Values<'_>),
    ) {
        match operand {
            Operand::Constant(value) => use_values(context, Values::Constant(value)),
            Operand::Node(node) => context.with_buffer(volume.size(), |context, buffer| {
                self.fill_exact(context, node, buffer, volume);
                use_values(context, Values::Buffer(buffer));
            }),
        }
    }

    fn sample_cached(
        &self,
        context: &mut DensityContext,
        slot: u32,
        input: NodeId,
        position: [i32; 3],
    ) -> f32 {
        let key = BlockPos::new(position[0], position[1], position[2]).pack();
        let Some(cell) = context.cache_cell(slot, self.cache_count) else {
            return self.sample(context, input, position);
        };
        if cell.value_key == key && !cell.value.is_nan() {
            return cell.value;
        }
        if let Some(index) = cell
            .volume
            .and_then(|volume| volume.index_of_block(position))
        {
            return cell.buffer[index];
        }
        let value = self.sample(context, input, position);
        let cell = context
            .cache_cell(slot, self.cache_count)
            .expect("a cached context");
        cell.value_key = key;
        cell.value = value;
        value
    }

    fn fill_cached(
        &self,
        context: &mut DensityContext,
        slot: u32,
        input: NodeId,
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        let Some(cell) = context.cache_cell(slot, self.cache_count) else {
            return self.fill_exact(context, input, out, volume);
        };
        if cell.volume == Some(volume) {
            out.copy_from_slice(&cell.buffer);
            return;
        }
        let mut cached = std::mem::take(&mut cell.buffer);
        cached.clear();
        cached.resize(volume.size(), 0.0);
        cell.volume = Some(volume);
        self.fill_exact(context, input, &mut cached, volume);
        out.copy_from_slice(&cached);
        context
            .cache_cell(slot, self.cache_count)
            .expect("a cached context")
            .buffer = cached;
    }

    fn fill_slice(
        &self,
        context: &mut DensityContext,
        input: NodeId,
        axes: SliceAxes,
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        let fixed = axes.coordinates();
        let mut sizes = volume.sizes().map(|size| size as i32);
        let mut minimum = volume.minimum();
        for (axis, coordinate) in fixed.into_iter().enumerate() {
            if let Some(coordinate) = coordinate {
                sizes[axis] = 1;
                minimum[axis] = coordinate;
            }
        }
        // The original steps stay, even on fixed axes: cached children see the whole volume
        // descriptor.
        let reduced =
            DensityVolume::new(sizes, minimum, volume.steps()).expect("a slice of a valid volume");
        if reduced == volume {
            return self.fill_exact(context, input, out, volume);
        }
        context.with_buffer(reduced.size(), |context, samples| {
            self.fill_exact(context, input, samples, reduced);
            let [sx, sy, sz] = volume.sizes();
            let [rx, ry, _] = reduced.sizes();
            for z in 0..sz {
                let source_z = if fixed[2].is_some() { 0 } else { z };
                for x in 0..sx {
                    let source_x = if fixed[0].is_some() { 0 } else { x };
                    let source = (source_x + source_z * rx) * ry;
                    let target = (x + z * sx) * sy;
                    let column = &mut out[target..target + sy];
                    if fixed[1].is_some() {
                        column.fill(samples[source]);
                    } else {
                        column.copy_from_slice(&samples[source..source + sy]);
                    }
                }
            }
        });
    }

    fn sample_interpolated(
        &self,
        context: &mut DensityContext,
        input: NodeId,
        cell: [i32; 3],
        position: [i32; 3],
    ) -> f32 {
        let remainder: [i32; 3] = std::array::from_fn(|i| position[i].rem_euclid(cell[i]));
        if remainder == [0; 3] {
            return self.sample(context, input, position);
        }
        let volume = DensityVolume::new(
            [2; 3],
            std::array::from_fn(|i| position[i].wrapping_sub(remainder[i])),
            cell,
        )
        .expect("a 2x2x2 corner volume");
        context.with_buffer(8, |context, corners| {
            self.fill_exact(context, input, corners, volume);
            let alpha: [f32; 3] = std::array::from_fn(|i| remainder[i] as f32 / cell[i] as f32);
            // Samples use Y-innermost storage; interpolation blends X, Y, Z.
            let xy0 = lerp(
                alpha[1],
                lerp(alpha[0], corners[0], corners[2]),
                lerp(alpha[0], corners[1], corners[3]),
            );
            let xy1 = lerp(
                alpha[1],
                lerp(alpha[0], corners[4], corners[6]),
                lerp(alpha[0], corners[5], corners[7]),
            );
            lerp(alpha[2], xy0, xy1)
        })
    }

    fn fill_interpolated(
        &self,
        context: &mut DensityContext,
        input: NodeId,
        cell: [i32; 3],
        inverse: [f32; 3],
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        if (0..3).all(|i| {
            (volume.steps()[i] == cell[i] || volume.sizes()[i] == 1)
                && volume.minimum()[i].rem_euclid(cell[i]) == 0
        }) {
            return self.fill_exact(context, input, out, volume);
        }
        if volume.steps() == [1; 3] {
            return self.fill_block_volume(context, input, cell, inverse, out, volume);
        }
        // Vanilla evaluates the whole block-resolution bounding volume before picking the
        // strided samples, trailing unsampled blocks included.
        let dense = DensityVolume::new(
            std::array::from_fn(|i| (volume.sizes()[i] as i32).wrapping_mul(volume.steps()[i])),
            volume.minimum(),
            [1; 3],
        )
        .expect("the dense bounds of a valid volume");
        context.with_buffer(dense.size(), |context, samples| {
            self.fill_block_volume(context, input, cell, inverse, samples, dense);
            let [sx, sy, sz] = volume.sizes();
            for z in 0..sz {
                for x in 0..sx {
                    for y in 0..sy {
                        let source = dense
                            .index(std::array::from_fn(|i| {
                                [x, y, z][i] * volume.steps()[i] as usize
                            }))
                            .expect("a sample inside the dense volume");
                        out[(x + z * sx) * sy + y] = samples[source];
                    }
                }
            }
        });
    }

    fn fill_block_volume(
        &self,
        context: &mut DensityContext,
        input: NodeId,
        cell: [i32; 3],
        inverse: [f32; 3],
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        let minimum = volume.minimum();
        let maximum = volume.maximum();
        let first: [i32; 3] = std::array::from_fn(|i| minimum[i].div_euclid(cell[i]));
        let counts: [i32; 3] = std::array::from_fn(|i| {
            maximum[i]
                .div_euclid(cell[i])
                .wrapping_sub(first[i])
                .wrapping_add(1)
        });
        let sizes = std::array::from_fn(|i| {
            counts[i].wrapping_add(i32::from(maximum[i].rem_euclid(cell[i]) != 0))
        });
        let coarse = DensityVolume::new(
            sizes,
            std::array::from_fn(|i| first[i].wrapping_mul(cell[i])),
            cell,
        )
        .expect("the coarse grid of a valid volume");
        context.with_buffer(coarse.size(), |context, samples| {
            self.fill_exact(context, input, samples, coarse);
            let coarse_sizes = coarse.sizes();
            for z in 0..counts[2] {
                for x in 0..counts[0] {
                    for y in 0..counts[1] {
                        let at = [x as usize, y as usize, z as usize];
                        let corners = std::array::from_fn(|corner| {
                            let position = std::array::from_fn(|axis| {
                                (at[axis] + ((corner >> axis) & 1)).min(coarse_sizes[axis] - 1)
                            });
                            samples[coarse.index(position).expect("a coarse corner")]
                        });
                        fill_cell(out, volume, coarse, cell, inverse, at, corners);
                    }
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)] // The search's inputs, as `FindTopSurface` holds them.
    fn find_surface(
        &self,
        context: &mut DensityContext,
        density: NodeId,
        lower: i32,
        cell_height: i32,
        x: i32,
        z: i32,
        upper: f32,
    ) -> i32 {
        let quotient = upper / cell_height as f32;
        // Mth.floor(float) widens to double, calls Math.floor, then converts with saturation
        // (NaN becomes zero).
        let floor = (f64::from(quotient)).floor() as i32;
        let mut y = floor.wrapping_mul(cell_height);
        if y <= lower {
            return lower;
        }
        while y >= lower {
            if self.sample(context, density, [x, y, z]) > 0.0 {
                return y;
            }
            // Java would wrap past i32::MIN and keep searching from the top; only a cell height
            // near 2^31 (which no data uses) gets there, and the search ends instead.
            match y.checked_sub(cell_height) {
                Some(next) => y = next,
                None => break,
            }
        }
        lower
    }

    fn sample_spline(
        &self,
        context: &mut DensityContext,
        spline: u32,
        coordinates: &[NodeId],
        position: [i32; 3],
    ) -> f32 {
        let spline = &self.splines[spline as usize];
        // NaN marks a coordinate not computed yet (and a NaN result is computed again, as in
        // vanilla).
        let sample = |values: &mut [f32], context: &mut DensityContext| {
            values.fill(f32::NAN);
            spline.sample(&mut |slot| {
                if values[slot].is_nan() {
                    values[slot] = self.sample(context, coordinates[slot], position);
                }
                values[slot]
            })
        };
        if coordinates.len() <= 8 {
            sample(&mut [0.0; 8][..coordinates.len()], context)
        } else {
            let mut values = vec![0.0; coordinates.len()];
            sample(&mut values, context)
        }
    }

    fn fill_spline(
        &self,
        context: &mut DensityContext,
        spline: u32,
        coordinates: &[NodeId],
        out: &mut [f32],
        volume: DensityVolume,
    ) {
        let spline = &self.splines[spline as usize];
        if let Some(value) = spline.constant() {
            out.fill(value);
            return;
        }
        // Each coordinate's volume is computed the first time an output needs it.
        let mut buffers: Vec<Option<Vec<f32>>> = vec![None; coordinates.len()];
        for (index, value) in out.iter_mut().enumerate() {
            *value = spline.sample(&mut |slot| {
                let buffer = buffers[slot].get_or_insert_with(|| {
                    let mut buffer = context.scratch.pop().unwrap_or_default();
                    buffer.resize(volume.size(), 0.0);
                    self.fill_exact(context, coordinates[slot], &mut buffer, volume);
                    buffer
                });
                buffer[index]
            });
        }
        context.scratch.extend(buffers.into_iter().flatten());
    }
}

#[derive(Clone, Copy)]
enum Values<'a> {
    Constant(f32),
    Buffer(&'a [f32]),
}

impl Values<'_> {
    fn get(self, index: usize) -> f32 {
        match self {
            Self::Constant(value) => value,
            Self::Buffer(buffer) => buffer[index],
        }
    }
}

fn interpolate(alpha: f32, first: f32, second: f32) -> f32 {
    if alpha == 0.0 {
        first
    } else if alpha == 1.0 {
        second
    } else {
        first + alpha * (second - first)
    }
}

fn lerp(alpha: f32, first: f32, second: f32) -> f32 {
    first + alpha * (second - first)
}

/// The branch of an interval selection: the first threshold above the value, else the last.
fn select(thresholds: &[f32], value: f32) -> usize {
    thresholds
        .iter()
        .position(|threshold| value < *threshold)
        .unwrap_or(thresholds.len())
}

/// Every lattice position of a volume in storage order.
fn visit(volume: DensityVolume, mut cell: impl FnMut(usize, [i32; 3])) {
    let [sx, sy, sz] = volume.sizes();
    let [mx, my, mz] = volume.minimum();
    let [dx, dy, dz] = volume.steps();
    let mut index = 0;
    for z in 0..sz {
        let bz = mz.wrapping_add((z as i32).wrapping_mul(dz));
        for x in 0..sx {
            let bx = mx.wrapping_add((x as i32).wrapping_mul(dx));
            for y in 0..sy {
                cell(
                    index,
                    [bx, my.wrapping_add((y as i32).wrapping_mul(dy)), bz],
                );
                index += 1;
            }
        }
    }
}

fn fill_gradient(gradient: &Gradient, out: &mut [f32], volume: DensityVolume) {
    let [sx, sy, sz] = volume.sizes();
    let axis = gradient.axis;
    for coordinate in 0..volume.sizes()[axis as usize] {
        let value = gradient.compute(volume.block(axis, coordinate).expect("inside the volume"));
        match axis {
            Axis::X => {
                for z in 0..sz {
                    let start = (coordinate + z * sx) * sy;
                    out[start..start + sy].fill(value);
                }
            }
            Axis::Y => {
                for z in 0..sz {
                    for x in 0..sx {
                        out[(x + z * sx) * sy + coordinate] = value;
                    }
                }
            }
            Axis::Z => {
                let start = coordinate * sx * sy;
                out[start..start + sx * sy].fill(value);
            }
        }
    }
}

/// One coarse cell of a block-resolution interpolation.
fn fill_cell(
    out: &mut [f32],
    volume: DensityVolume,
    coarse: DensityVolume,
    cell_size: [i32; 3],
    inverse: [f32; 3],
    cell: [usize; 3],
    corners: [f32; 8],
) {
    let offset: [i32; 3] = std::array::from_fn(|i| {
        coarse.minimum()[i]
            .wrapping_add((cell[i] as i32).wrapping_mul(cell_size[i]))
            .wrapping_sub(volume.minimum()[i])
    });
    let start = offset.map(|offset| offset.wrapping_neg().max(0));
    let end: [i32; 3] = std::array::from_fn(|i| {
        cell_size[i]
            .min((volume.sizes()[i] as i32).wrapping_sub(offset[i]))
            .wrapping_sub(1)
    });
    for z in start[2]..=end[2] {
        let alpha_z = z as f32 * inverse[2];
        let z00 = lerp(alpha_z, corners[0], corners[4]);
        let z01 = lerp(alpha_z, corners[2], corners[6]);
        let z10 = lerp(alpha_z, corners[1], corners[5]);
        let z11 = lerp(alpha_z, corners[3], corners[7]);
        for x in start[0]..=end[0] {
            let alpha_x = x as f32 * inverse[0];
            let low = lerp(alpha_x, z00, z10);
            let high = lerp(alpha_x, z01, z11);
            let increment = (high - low) * inverse[1];
            let mut value = low + increment * start[1] as f32;
            if end[1] < start[1] {
                continue;
            }
            let index = volume
                .index([
                    offset[0].wrapping_add(x) as usize,
                    offset[1].wrapping_add(start[1]) as usize,
                    offset[2].wrapping_add(z) as usize,
                ])
                .expect("a cell inside the volume");
            let length = (end[1] - start[1] + 1) as usize;
            for output in &mut out[index..index + length] {
                *output = value;
                // Repeated addition is observable: multiplying by the Y offset rounds
                // differently.
                value += increment;
            }
        }
    }
}
