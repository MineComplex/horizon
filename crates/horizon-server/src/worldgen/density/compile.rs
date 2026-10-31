//! `DensityFunctionCompiler` for one seeded level: the graph's functions become nodes of one
//! [`DensityProgram`]. The passes are vanilla's (inline one registry holder, turn `cache` markers
//! into cache cells keyed by Java equality, slice off axes a parent does not depend on) and so are
//! the specializations of literal operands, so results match vanilla bit for bit. Adapted from
//! Horizon's `horizon-core::density::graph::compiler`, which built `Arc<dyn DensitySampler>`
//! trees instead.

use std::collections::HashMap;

use super::end_islands::EndIslands;
use super::graph::{Definition, DensityGraph, FunctionId, GraphBinary, GraphNode, NoiseId};
use super::ops::{Gradient, SliceAxes, UnaryOp};
use super::program::{DensityProgram, Node, NoiseRef, Operand};
use super::spline::SplineProgram;
use super::{BlendedNoise, NodeId, ProgramError, ShiftNoiseKind};
use crate::core::geometry::Axis;
use crate::util::random::{
    LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource,
    XoroshiroPositionalRandom, XoroshiroRandom,
};
use crate::worldgen::noise::NoiseStack;

/// The level's random factory (`RandomState.random`): legacy or Xoroshiro by the noise settings.
#[derive(Clone, Copy, Debug)]
enum Factory {
    Legacy(LegacyPositionalRandom),
    Xoroshiro(XoroshiroPositionalRandom),
}

/// Compiles functions of one graph for one seed into one program. Cache cells are numbered per
/// compiler, as vanilla's cache ids are per `RandomState`.
#[derive(Debug)]
pub struct DensityCompiler {
    graph: DensityGraph,
    seed: i64,
    /// Purpur's `generateEndVoidRings`, for the End islands.
    end_void_rings: bool,
    factory: Factory,
    program: DensityProgram,
    /// The seeded instance of each graph noise, by `NoiseId`.
    noise_refs: Vec<NoiseRef>,
    /// `getSampler`'s results by root.
    roots: HashMap<FunctionId, NodeId>,
    /// Prepared `cache` inputs (by Java equality, through the graph's interning).
    prepared: HashMap<FunctionId, FunctionId>,
    /// Compiled functions: compilation is pure, so equal ids share nodes.
    compiled: HashMap<FunctionId, NodeId>,
}

impl DensityCompiler {
    /// `RandomState.create`'s compile context: every registered noise is instantiated now (from
    /// `fromHashOf(name)`), so the program needs no lazily filled instance map.
    pub fn new(
        graph: DensityGraph,
        seed: i64,
        legacy_random: bool,
        end_void_rings: bool,
    ) -> Result<Self, ProgramError> {
        let factory = if legacy_random {
            Factory::Legacy(LegacyRandom::new(seed).fork_positional())
        } else {
            Factory::Xoroshiro(XoroshiroRandom::new(seed).fork_positional())
        };
        let mut program = DensityProgram {
            nodes: Vec::new(),
            noises: Vec::new(),
            splines: Vec::new(),
            cache_count: 0,
        };
        let mut noise_refs = Vec::with_capacity(graph.noises().len());
        for entry in graph.noises() {
            // The Nether's biome noises bypass the named instances: each lookup makes them from
            // their own legacy seeds (all lookups give the same noise).
            let legacy_offset = match entry.name.as_str() {
                "minecraft:nether/temperature" => Some(0),
                "minecraft:nether/vegetation" => Some(1),
                _ => None,
            };
            let stack = match (legacy_offset, factory) {
                (Some(offset), _) => {
                    entry
                        .definition
                        .create_for_legacy_nether_biome(&mut LegacyRandom::new(
                            seed.wrapping_add(offset),
                        ))?
                }
                (None, Factory::Legacy(factory)) => entry
                    .definition
                    .create(&mut factory.from_hash_of(&entry.name))?,
                (None, Factory::Xoroshiro(factory)) => entry
                    .definition
                    .create(&mut factory.from_hash_of(&entry.name))?,
            };
            noise_refs.push(push_noise(&mut program, stack));
        }
        Ok(Self {
            graph,
            seed,
            end_void_rings,
            factory,
            program,
            noise_refs,
            roots: HashMap::new(),
            prepared: HashMap::new(),
            compiled: HashMap::new(),
        })
    }

    pub fn graph(&self) -> &DensityGraph {
        &self.graph
    }

    /// `getSampler`: the root's program node, compiled once.
    pub fn compile(&mut self, root: FunctionId) -> Result<NodeId, ProgramError> {
        if let Some(node) = self.roots.get(&root) {
            return Ok(*node);
        }
        let optimized = self.optimize(root)?;
        let node = self.compile_raw(optimized)?;
        self.roots.insert(root, node);
        Ok(node)
    }

    /// The finished program.
    pub fn finish(self) -> DensityProgram {
        self.program
    }

    fn push(&mut self, node: Node) -> NodeId {
        self.program.nodes.push(node);
        NodeId((self.program.nodes.len() - 1) as u32)
    }

    fn rewrite_children(
        &mut self,
        id: FunctionId,
        mut rewrite: impl FnMut(&mut Self, FunctionId) -> Result<FunctionId, ProgramError>,
    ) -> Result<FunctionId, ProgramError> {
        let Some(mut node) = self.graph.node(id).cloned() else {
            return Ok(id);
        };
        if let GraphNode::Reference { input, .. } = node {
            let rewritten = rewrite(self, input)?;
            return Ok(if rewritten == input { id } else { rewritten });
        }
        for child in node.children_mut() {
            *child = rewrite(self, *child)?;
        }
        Ok(self.graph.insert(node)?)
    }

    fn optimize(&mut self, id: FunctionId) -> Result<FunctionId, ProgramError> {
        let prepared = self.prepare(id)?;
        self.slice_uniform_axes(prepared, 7)
    }

    fn prepare(&mut self, mut id: FunctionId) -> Result<FunctionId, ProgramError> {
        // Vanilla's INLINE_REFERENCE unwraps one holder before detecting caches; nested holders
        // follow rewriteChildren's identity rule.
        if let Some(GraphNode::Reference { input, .. }) = self.graph.node(id) {
            id = *input;
        }
        if let Some(GraphNode::Cache(input)) = self.graph.node(id) {
            let input = *input;
            if let Some(prepared) = self.prepared.get(&input) {
                return Ok(*prepared);
            }
            let slot = self.program.cache_count;
            if slot > 65535 {
                return Err(ProgramError::TooManyCaches);
            }
            self.program.cache_count += 1;
            let optimized = self.optimize(input)?;
            let inner = self.compile_raw(optimized)?;
            let node = self.push(Node::Cache {
                slot: slot as u32,
                input: inner,
            });
            let range = self.graph.range(optimized)?;
            let axes = self.graph.domain_axes(optimized)?;
            let prepared = self.graph.bind(node, range, axes);
            self.prepared.insert(input, prepared);
            return Ok(prepared);
        }
        self.rewrite_children(id, Self::prepare)
    }

    fn slice_uniform_axes(
        &mut self,
        id: FunctionId,
        parent_axes: u8,
    ) -> Result<FunctionId, ProgramError> {
        if matches!(
            self.graph.node(id),
            Some(GraphNode::Constant(_) | GraphNode::Gradient { .. })
        ) {
            return Ok(id);
        }
        let axes = self.graph.domain_axes(id)?;
        let mut result = self.rewrite_children(id, |compiler, child| {
            compiler.slice_uniform_axes(child, axes)
        })?;
        if axes == parent_axes {
            return Ok(result);
        }
        let mut existing = 0;
        let mut cursor = result;
        while let Some(GraphNode::Slice { axis, input, .. }) = self.graph.node(cursor) {
            existing |= 1 << *axis as u8;
            cursor = *input;
        }
        let remove = parent_axes & !axes & !existing;
        // X then Z puts these slices next to each other for the XZ form.
        for axis in [Axis::X, Axis::Z, Axis::Y] {
            if remove & (1 << axis as u8) != 0 {
                result = self.graph.insert(GraphNode::Slice {
                    axis,
                    coordinate: 0,
                    input: result,
                })?;
            }
        }
        Ok(result)
    }

    fn constant(&self, id: FunctionId) -> Option<f32> {
        match self.graph.node(id) {
            Some(GraphNode::Constant(value)) => Some(*value),
            _ => None,
        }
    }

    fn noise_ref(&self, noise: NoiseId) -> NoiseRef {
        self.noise_refs[noise.0 as usize]
    }

    /// Compiles exactly this definition, without rewriting. `cache` markers must have been
    /// prepared by [`Self::compile`].
    fn compile_raw(&mut self, id: FunctionId) -> Result<NodeId, ProgramError> {
        if let Some(node) = self.compiled.get(&id) {
            return Ok(*node);
        }
        let node = match &self.graph.entry(id)?.definition {
            Definition::Bound { node, .. } => *node,
            Definition::Node(node) => {
                let node = node.clone();
                self.compile_node(node)?
            }
        };
        self.compiled.insert(id, node);
        Ok(node)
    }

    fn compile_node(&mut self, node: GraphNode) -> Result<NodeId, ProgramError> {
        Ok(match node {
            GraphNode::Constant(value) => self.push(Node::Constant(value)),
            GraphNode::Reference { input, .. } => self.compile_raw(input)?,
            GraphNode::WorldField(field) => self.push(Node::Field(field)),
            // Without old-world blending (not ported) `blend_density` is its input.
            GraphNode::BlendDensity(input) => self.compile_raw(input)?,
            GraphNode::EndIslands => self.push(Node::EndIslands(Box::new(EndIslands::new(
                self.seed,
                self.end_void_rings,
            )))),
            GraphNode::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let noise = self.noise_ref(noise);
                let zero = |id: FunctionId| matches!(self.graph.node(id), Some(GraphNode::Constant(v)) if v.to_bits() == 0);
                if zero(shift_x) && zero(shift_y) && zero(shift_z) {
                    self.push(Node::Noise {
                        noise,
                        xz_scale,
                        y_scale,
                    })
                } else {
                    // Vanilla compiles X, then Z, then the optional Y (not the sampling order).
                    let y_is_zero = zero(shift_y);
                    let shift_x = self.compile_raw(shift_x)?;
                    let shift_z = self.compile_raw(shift_z)?;
                    let shift_y = if y_is_zero {
                        None
                    } else {
                        Some(self.compile_raw(shift_y)?)
                    };
                    self.push(Node::ShiftedNoise {
                        noise,
                        xz_scale,
                        y_scale,
                        shift_x,
                        shift_y,
                        shift_z,
                    })
                }
            }
            GraphNode::OffsetNoise { noise, kind } => {
                let noise = self.noise_ref(noise);
                if kind == ShiftNoiseKind::ShiftB {
                    self.push(Node::ShiftB(noise))
                } else {
                    let input = self.push(Node::Noise {
                        noise,
                        xz_scale: 0.25,
                        y_scale: if kind == ShiftNoiseKind::ShiftA {
                            0.0
                        } else {
                            0.25
                        },
                    });
                    self.push(Node::Unary {
                        input,
                        op: UnaryOp::Multiply(4.0),
                    })
                }
            }
            GraphNode::BlendedNoise(definition) => self.compile_blended(definition)?,
            GraphNode::Spline(spline) => {
                let mut coordinates: Vec<(FunctionId, NodeId)> = Vec::new();
                let mut failure = None;
                // Each distinct coordinate is compiled, then its range checked, in first-use
                // order (vanilla validates even coordinates sampling never reaches).
                let program = SplineProgram::compile(&spline, &mut |id| {
                    if let Some(slot) = coordinates.iter().position(|(c, _)| *c == id) {
                        return slot as u16;
                    }
                    if failure.is_none() {
                        match self
                            .compile_raw(id)
                            .and_then(|node| self.graph.range(id).map(|_| node).map_err(Into::into))
                        {
                            Ok(node) => coordinates.push((id, node)),
                            Err(error) => {
                                failure = Some(error);
                                coordinates.push((id, NodeId(0)));
                            }
                        }
                    } else {
                        coordinates.push((id, NodeId(0)));
                    }
                    (coordinates.len() - 1) as u16
                })?;
                if let Some(error) = failure {
                    return Err(error);
                }
                self.program.splines.push(program);
                let spline = (self.program.splines.len() - 1) as u32;
                self.push(Node::Spline {
                    spline,
                    coordinates: coordinates.into_iter().map(|(_, node)| node).collect(),
                })
            }
            GraphNode::DistanceToPoint { point, metric } => {
                self.push(Node::DistanceToPoint { point, metric })
            }
            GraphNode::IntervalSelect {
                input,
                thresholds,
                branches,
            } => {
                let input = self.compile_raw(input)?;
                let branches = branches
                    .iter()
                    .map(|id| self.compile_raw(*id))
                    .collect::<Result<_, _>>()?;
                self.push(Node::IntervalSelect {
                    input,
                    thresholds,
                    branches,
                })
            }
            GraphNode::Gradient {
                axis,
                tiling,
                from,
                to,
                low,
                high,
            } => self.push(Node::Gradient(Gradient::new(
                axis, tiling, from, to, low, high,
            ))),
            GraphNode::Unary { kind, input } => {
                let input = self.compile_raw(input)?;
                self.push(Node::Unary {
                    input,
                    op: kind.operation(),
                })
            }
            GraphNode::Round {
                mode,
                input,
                multiple,
            } => {
                let input = self.compile_raw(input)?;
                let multiple = if self.constant(multiple) == Some(1.0) {
                    None
                } else {
                    Some(self.compile_raw(multiple)?)
                };
                self.push(Node::Round {
                    mode,
                    input,
                    multiple,
                })
            }
            GraphNode::Power { base, exponent } => {
                // Both children compile before the literal forms are picked, even when one of
                // them is then unused.
                let compiled_base = self.compile_raw(base)?;
                let compiled_exponent = self.compile_raw(exponent)?;
                if let Some(value) = self.constant(base) {
                    self.push(Node::Unary {
                        input: compiled_exponent,
                        op: UnaryOp::PowerBase(f64::from(value)),
                    })
                } else if let Some(value) = self.constant(exponent) {
                    self.constant_exponent(compiled_base, value)
                } else {
                    self.push(Node::Power {
                        base: compiled_base,
                        exponent: compiled_exponent,
                    })
                }
            }
            GraphNode::Binary { kind, left, right } => self.compile_binary(kind, left, right)?,
            GraphNode::Clamp { input, min, max } => {
                let input = self.compile_raw(input)?;
                self.push(Node::Unary {
                    input,
                    op: UnaryOp::Clamp { min, max },
                })
            }
            GraphNode::Choice {
                input,
                min,
                max,
                inside,
                outside,
            } => {
                let input = self.compile_raw(input)?;
                if let (Some(inside), Some(outside)) =
                    (self.constant(inside), self.constant(outside))
                {
                    self.push(Node::Unary {
                        input,
                        op: UnaryOp::ConstantRangeChoice {
                            min_inclusive: min,
                            max_exclusive: max,
                            inside,
                            outside,
                        },
                    })
                } else {
                    let inside = self.compile_raw(inside)?;
                    let outside = self.compile_raw(outside)?;
                    self.push(Node::RangeChoice {
                        input,
                        min_inclusive: min,
                        max_exclusive: max,
                        inside,
                        outside,
                    })
                }
            }
            GraphNode::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = self.compile_raw(alpha)?;
                let first_node = self.compile_raw(first)?;
                let second_node = self.compile_raw(second)?;
                let (first, second) = if let Some(value) = self.constant(first) {
                    (Operand::Constant(value), Operand::Node(second_node))
                } else if let Some(value) = self.constant(second) {
                    (Operand::Node(first_node), Operand::Constant(value))
                } else {
                    (Operand::Node(first_node), Operand::Node(second_node))
                };
                self.push(Node::Lerp {
                    alpha,
                    first,
                    second,
                })
            }
            GraphNode::Slice {
                axis,
                coordinate,
                input,
            } => {
                let mut axes = match axis {
                    Axis::X => SliceAxes::X(coordinate),
                    Axis::Y => SliceAxes::Y(coordinate),
                    Axis::Z => SliceAxes::Z(coordinate),
                };
                let mut child = input;
                if let Some(GraphNode::Slice {
                    axis: inner_axis,
                    coordinate: inner_coordinate,
                    input: inner,
                }) = self.graph.node(input)
                {
                    if axis == Axis::X && *inner_axis == Axis::Z {
                        axes = SliceAxes::Xz {
                            x: coordinate,
                            z: *inner_coordinate,
                        };
                        child = *inner;
                    } else if axis == Axis::Z && *inner_axis == Axis::X {
                        axes = SliceAxes::Xz {
                            x: *inner_coordinate,
                            z: coordinate,
                        };
                        child = *inner;
                    }
                }
                let input = self.compile_raw(child)?;
                self.push(Node::Slice { input, axes })
            }
            GraphNode::Cache(_) => return Err(ProgramError::UnpreparedCache),
            GraphNode::Interpolated { input, xz, y } => {
                if xz <= 0 || y <= 0 {
                    return Err(ProgramError::NonpositiveCell);
                }
                let input = self.compile_raw(input)?;
                let cell = [xz, y, xz];
                self.push(Node::Interpolated {
                    input,
                    cell,
                    inverse: cell.map(|size| 1.0 / size as f32),
                })
            }
            GraphNode::Surface {
                density,
                upper,
                lower,
                height,
            } => {
                if height <= 0 {
                    return Err(ProgramError::NonpositiveCell);
                }
                let density = self.compile_raw(density)?;
                let upper = self.compile_raw(upper)?;
                let input = self.push(Node::TopSurface {
                    density,
                    upper,
                    lower,
                    cell_height: height,
                });
                self.push(Node::Slice {
                    input,
                    axes: SliceAxes::Y(0),
                })
            }
        })
    }

    /// Only sqrt/identity/square/cube get the negative-exponent reciprocal; other exponents use
    /// vanilla's signed fallback directly.
    fn constant_exponent(&mut self, base: NodeId, exponent: f32) -> NodeId {
        let positive = match exponent.abs() {
            0.5 => self.push(Node::Unary {
                input: base,
                op: UnaryOp::Sqrt,
            }),
            1.0 => base,
            2.0 => self.push(Node::Unary {
                input: base,
                op: UnaryOp::Square,
            }),
            3.0 => self.push(Node::Unary {
                input: base,
                op: UnaryOp::Cube,
            }),
            _ => {
                return self.push(Node::Unary {
                    input: base,
                    op: UnaryOp::PowerExponent(f64::from(exponent)),
                });
            }
        };
        if exponent < 0.0 {
            self.push(Node::Unary {
                input: positive,
                op: UnaryOp::Reciprocal,
            })
        } else {
            positive
        }
    }

    fn compile_binary(
        &mut self,
        kind: GraphBinary,
        left_id: FunctionId,
        right_id: FunctionId,
    ) -> Result<NodeId, ProgramError> {
        use super::ops::BinaryOp;
        let left = self.compile_raw(left_id)?;
        let right = self.compile_raw(right_id)?;
        let bounds = if matches!(kind, GraphBinary::Minimum | GraphBinary::Maximum) {
            let a = self.graph.range(left_id)?;
            let b = self.graph.range(right_id)?;
            if (kind == GraphBinary::Minimum && a.max() < b.min())
                || (kind == GraphBinary::Maximum && a.min() > b.max())
            {
                return Ok(left);
            }
            if (kind == GraphBinary::Minimum && b.max() < a.min())
                || (kind == GraphBinary::Maximum && b.min() > a.max())
            {
                return Ok(right);
            }
            Some(b)
        } else {
            None
        };
        if let Some(value) = self.constant(left_id) {
            let op = match kind {
                GraphBinary::Add => UnaryOp::Add(value),
                GraphBinary::Subtract => UnaryOp::SubtractFrom(value),
                GraphBinary::Multiply => UnaryOp::Multiply(value),
                GraphBinary::Divide => UnaryOp::DivideInto(value),
                GraphBinary::Minimum => UnaryOp::Minimum(value),
                GraphBinary::Maximum => UnaryOp::Maximum(value),
            };
            return Ok(self.push(Node::Unary { input: right, op }));
        }
        if let Some(value) = self.constant(right_id) {
            let op = match kind {
                GraphBinary::Add => UnaryOp::Add(value),
                GraphBinary::Subtract => UnaryOp::Add(-value),
                GraphBinary::Multiply => UnaryOp::Multiply(value),
                GraphBinary::Divide => UnaryOp::Multiply(1.0 / value),
                GraphBinary::Minimum => UnaryOp::Minimum(value),
                GraphBinary::Maximum => UnaryOp::Maximum(value),
            };
            return Ok(self.push(Node::Unary { input: left, op }));
        }
        let op = match (kind, bounds) {
            (GraphBinary::Add, _) => BinaryOp::Add,
            (GraphBinary::Subtract, _) => BinaryOp::Subtract,
            (GraphBinary::Multiply, _) => BinaryOp::Multiply,
            (GraphBinary::Divide, _) => BinaryOp::Divide,
            (GraphBinary::Minimum, Some(bounds)) => BinaryOp::Minimum {
                right_min: bounds.min(),
            },
            (GraphBinary::Maximum, Some(bounds)) => BinaryOp::Maximum {
                right_max: bounds.max(),
            },
            (GraphBinary::Minimum | GraphBinary::Maximum, None) => {
                unreachable!("min and max have bounds")
            }
        };
        Ok(self.push(Node::Binary { left, right, op }))
    }

    /// `BlendedNoise.compileSampler`: lerp(clamp(main + 0.5, 0, 1), min_limit, max_limit) over
    /// three smeared octave stacks.
    fn compile_blended(&mut self, definition: BlendedNoise) -> Result<NodeId, ProgramError> {
        let layers = match self.factory {
            Factory::Legacy(_) => definition.create_fbm_set(&mut LegacyRandom::new(self.seed))?,
            Factory::Xoroshiro(factory) => {
                definition.create_fbm_set(&mut factory.from_hash_of("minecraft:terrain"))?
            }
        };
        let xz_scale = 684.412 * definition.xz_scale;
        let y_scale = 684.412 * definition.y_scale;
        let min_limit = push_noise(&mut self.program, layers.min_limit);
        let max_limit = push_noise(&mut self.program, layers.max_limit);
        let main = push_noise(&mut self.program, layers.main);
        let min_limit = self.push(Node::Noise {
            noise: min_limit,
            xz_scale,
            y_scale,
        });
        let max_limit = self.push(Node::Noise {
            noise: max_limit,
            xz_scale,
            y_scale,
        });
        let main = self.push(Node::Noise {
            noise: main,
            xz_scale: xz_scale / definition.xz_factor,
            y_scale: y_scale / definition.y_factor,
        });
        let shifted = self.push(Node::Unary {
            input: main,
            op: UnaryOp::Add(0.5),
        });
        let alpha = self.push(Node::Unary {
            input: shifted,
            op: UnaryOp::Clamp { min: 0.0, max: 1.0 },
        });
        Ok(self.push(Node::Lerp {
            alpha,
            first: Operand::Node(min_limit),
            second: Operand::Node(max_limit),
        }))
    }
}

fn push_noise(program: &mut DensityProgram, stack: NoiseStack) -> NoiseRef {
    program.noises.push(stack);
    NoiseRef((program.noises.len() - 1) as u32)
}
