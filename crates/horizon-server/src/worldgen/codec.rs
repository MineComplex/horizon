//! The worldgen registries' JSON as vanilla's codecs read it (`RegistryDataLoader`: Gson's
//! lazily parsed numbers, then DFU codecs). Numbers stay as their decimal text until the field's
//! width is known, so a float rounds once from the decimal (`Float.parseFloat`) and an int wraps
//! like `BigDecimal.intValue`. Copied from Horizon's `horizon-world::worldgen` decoders.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::value::RawValue;

use super::density::{
    BlendedNoise, CubicSpline, DensityGraph, DistanceMetric, FunctionId, GraphBinary, GraphError,
    GraphNode, GraphUnary, NoiseId, RoundMode, ShiftNoiseKind, SplinePoint, TilingMode, WorldField,
};
use super::noise::{NoiseBuildError, NormalNoise, Normalization};
use crate::core::geometry::Axis;

/// Why a worldgen registry element could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid world generation field: {0}")]
    Field(&'static str),
    #[error(transparent)]
    Noise(#[from] NoiseBuildError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("{0}")]
    Registry(String),
}

pub(crate) type Object = BTreeMap<String, Box<RawValue>>;

pub(crate) fn object(raw: &RawValue) -> Result<Object, DecodeError> {
    Ok(serde_json::from_str(raw.get())?)
}

/// A field, `null` counting as absent (as DFU's optional fields do).
pub(crate) fn optional<'a>(object: &'a Object, name: &str) -> Option<&'a RawValue> {
    object
        .get(name)
        .map(AsRef::as_ref)
        .filter(|raw| raw.get() != "null")
}

pub(crate) fn required<'a>(
    object: &'a Object,
    name: &'static str,
) -> Result<&'a RawValue, DecodeError> {
    optional(object, name).ok_or(DecodeError::Field(name))
}

fn number_text(raw: &RawValue) -> Result<&str, DecodeError> {
    let text = raw.get();
    if text.starts_with('-') || text.starts_with(|c: char| c.is_ascii_digit()) {
        Ok(text)
    } else {
        Err(DecodeError::Field("number"))
    }
}

pub(crate) fn string(raw: &RawValue) -> Result<String, DecodeError> {
    Ok(serde_json::from_str(raw.get())?)
}

pub(crate) fn boolean(raw: &RawValue) -> Result<bool, DecodeError> {
    match raw.get() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(DecodeError::Field("boolean")),
    }
}

pub(crate) fn double(raw: &RawValue) -> Result<f64, DecodeError> {
    number_text(raw)?
        .parse()
        .map_err(|_| DecodeError::Field("number"))
}

pub(crate) fn float(raw: &RawValue) -> Result<f32, DecodeError> {
    number_text(raw)?
        .parse()
        .map_err(|_| DecodeError::Field("float"))
}

/// Gson's lazily parsed numbers use `BigDecimal.intValue` for decimal and exponent forms: truncate
/// toward zero, then keep the low 32 bits. Going through f64 would round large integers and
/// saturate instead of wrapping.
pub(crate) fn integer(raw: &RawValue) -> Result<i32, DecodeError> {
    let text = number_text(raw)?;
    let negative = text.starts_with('-');
    let text = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| DecodeError::Field("integer exponent"))? as i64,
        ),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = whole.bytes().chain(fraction.bytes());
    let length = whole.len() as i64 + exponent;
    if length <= 0 {
        return Ok(0);
    }
    let available = whole.len() as i64 + fraction.len() as i64;
    let zeroes = (length - available).max(0);
    if zeroes >= 32 {
        return Ok(0);
    }
    let mut result = 0u32;
    for digit in digits.take(length as usize) {
        result = result
            .wrapping_mul(10)
            .wrapping_add(u32::from(digit - b'0'));
    }
    for _ in 0..zeroes {
        result = result.wrapping_mul(10);
    }
    Ok(if negative {
        result.wrapping_neg()
    } else {
        result
    } as i32)
}

pub(crate) fn int_range(
    raw: &RawValue,
    name: &'static str,
    low: i32,
    high: i32,
) -> Result<i32, DecodeError> {
    let value = integer(raw)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(DecodeError::Field(name))
    }
}

pub(crate) fn double_range(
    raw: &RawValue,
    name: &'static str,
    low: f64,
    high: f64,
) -> Result<f64, DecodeError> {
    let value = double(raw)?;
    if value.total_cmp(&low).is_ge() && value.total_cmp(&high).is_le() {
        Ok(value)
    } else {
        Err(DecodeError::Field(name))
    }
}

/// `Identifier.CODEC` for a registry reference: `ns:path` (namespace `minecraft` when absent).
pub(crate) fn identifier(name: &str) -> Result<String, DecodeError> {
    crate::registry::parse_identifier(name)
        .ok()
        .filter(|id| !id.ends_with(':'))
        .ok_or_else(|| DecodeError::Registry(format!("invalid registry identifier: {name}")))
}

/// `NormalNoise.NoiseParameters.DIRECT_CODEC`. Unknown fields are ignored, as by the codec.
pub fn decode_noise(raw: &RawValue) -> Result<NormalNoise, DecodeError> {
    let object = object(raw)?;
    let base = int_range(required(&object, "base_octave")?, "base_octave", -32, 32)?;
    let count = optional(&object, "octave_count")
        .map(|raw| int_range(raw, "octave_count", 1, 32))
        .transpose()?
        .unwrap_or(1);
    let amplitude = optional(&object, "base_amplitude")
        .map(|raw| double_range(raw, "base_amplitude", f64::from(1e-5f32), 1e6))
        .transpose()?
        .unwrap_or(1.0);
    let normalize = match optional(&object, "normalize") {
        None => Normalization::Enabled,
        Some(raw) => match raw.get() {
            "true" => Normalization::Enabled,
            "false" => Normalization::Disabled,
            text if text.starts_with('"') && serde_json::from_str::<String>(text)? == "legacy" => {
                Normalization::Legacy
            }
            _ => return Err(DecodeError::Field("normalize")),
        },
    };
    let modifiers: Vec<Box<RawValue>> = optional(&object, "amplitude_modifiers")
        .map(|raw| serde_json::from_str(raw.get()))
        .transpose()?
        .unwrap_or_default();
    if modifiers.len() > 32 || (!modifiers.is_empty() && modifiers.len() != count as usize) {
        return Err(DecodeError::Field("amplitude_modifiers"));
    }
    let mut builder = NormalNoise::builder();
    builder
        .set_base_octave(base)
        .set_base_amplitude(amplitude)
        .set_normalization(normalize);
    builder.set_octave_count(count)?;
    for (index, modifier) in modifiers.iter().enumerate() {
        builder.set_amplitude_modifier(
            index as i32,
            double_range(modifier, "amplitude_modifiers", 0.0, 1e6)?,
        )?;
    }
    Ok(builder.build()?)
}

/// A float density value: `DensityFunction.VALUE_CODEC`'s range.
fn noise_value(raw: &RawValue) -> Result<f32, DecodeError> {
    let value = float(raw)?;
    if (-1e6..=1e6).contains(&value) {
        Ok(value)
    } else {
        Err(DecodeError::Field("density value"))
    }
}

fn axis(raw: &RawValue) -> Result<Axis, DecodeError> {
    match string(raw)?.as_str() {
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "z" => Ok(Axis::Z),
        _ => Err(DecodeError::Field("axis")),
    }
}

/// Decodes density functions into a graph. Named references resolve through registry holders
/// (forward references allowed, cycles refused), as `RegistryDataLoader` binds them.
pub(crate) struct DensityLoader<'a> {
    pub(crate) graph: &'a mut DensityGraph,
    definitions: BTreeMap<String, Box<RawValue>>,
    active: BTreeSet<String>,
}

impl<'a> DensityLoader<'a> {
    pub(crate) fn new(graph: &'a mut DensityGraph) -> Self {
        Self {
            graph,
            definitions: BTreeMap::new(),
            active: BTreeSet::new(),
        }
    }

    /// Registers every element of `worldgen/density_function` (bodies decoded on resolution).
    pub(crate) fn load_registry(
        &mut self,
        elements: impl IntoIterator<Item = (String, Box<RawValue>)>,
    ) -> Result<(), DecodeError> {
        for (name, raw) in elements {
            let name = identifier(&name)?;
            if self.definitions.insert(name.clone(), raw).is_some() {
                return Err(DecodeError::Registry(format!(
                    "duplicate density identifier: {name}"
                )));
            }
        }
        let names: Vec<_> = self.definitions.keys().cloned().collect();
        for name in names {
            self.resolve(&name, 0)?;
        }
        Ok(())
    }

    /// A registry element's holder.
    fn resolve(&mut self, name: &str, depth: usize) -> Result<FunctionId, DecodeError> {
        if let Some(id) = self.graph.function(name) {
            return Ok(id);
        }
        if depth > 256 {
            return Err(DecodeError::Field("density nesting"));
        }
        if !self.active.insert(name.to_owned()) {
            return Err(DecodeError::Registry(format!(
                "cyclic density reference: {name}"
            )));
        }
        let raw = self
            .definitions
            .get(name)
            .ok_or_else(|| DecodeError::Registry(format!("unknown density reference: {name}")))?
            .clone();
        let value = self.decode(&raw, depth + 1)?;
        let reference = self.graph.register_function(name.to_owned(), value)?;
        self.active.remove(name);
        Ok(reference)
    }

    fn input(
        &mut self,
        object: &Object,
        name: &'static str,
        depth: usize,
    ) -> Result<FunctionId, DecodeError> {
        self.decode(required(object, name)?, depth + 1)
    }

    fn optional_input(
        &mut self,
        object: &Object,
        name: &'static str,
        fallback: f32,
        depth: usize,
    ) -> Result<FunctionId, DecodeError> {
        match optional(object, name) {
            Some(raw) => self.decode(raw, depth + 1),
            None => Ok(self.graph.insert(GraphNode::Constant(fallback))?),
        }
    }

    fn noise(&self, object: &Object) -> Result<NoiseId, DecodeError> {
        let raw = required(object, "noise")?;
        if !raw.get().starts_with('"') {
            // No vanilla function defines its noise inline.
            return Err(DecodeError::Field("inline noise holder unsupported"));
        }
        let name = identifier(&string(raw)?)?;
        self.graph
            .noise(&name)
            .ok_or_else(|| DecodeError::Registry(format!("unknown noise reference: {name}")))
    }

    /// `DensityFunction.CODEC`: a reference, a constant, or a typed object.
    pub(crate) fn decode(
        &mut self,
        raw: &RawValue,
        depth: usize,
    ) -> Result<FunctionId, DecodeError> {
        if depth > 256 {
            return Err(DecodeError::Field("density nesting"));
        }
        if raw.get().starts_with('"') {
            return self.resolve(&identifier(&string(raw)?)?, depth);
        }
        if !raw.get().starts_with('{') {
            return Ok(self.graph.insert(GraphNode::Constant(noise_value(raw)?))?);
        }
        let object = object(raw)?;
        let kind = identifier(&string(required(&object, "type")?)?)?;
        let unary = match kind.as_str() {
            "minecraft:abs" => Some(GraphUnary::Abs),
            "minecraft:square" => Some(GraphUnary::Square),
            "minecraft:cube" => Some(GraphUnary::Cube),
            "minecraft:sqrt" => Some(GraphUnary::Sqrt),
            "minecraft:half_negative" => Some(GraphUnary::HalfNegative),
            "minecraft:quarter_negative" => Some(GraphUnary::QuarterNegative),
            "minecraft:reciprocal" => Some(GraphUnary::Reciprocal),
            "minecraft:negate" => Some(GraphUnary::Negate),
            "minecraft:squeeze" => Some(GraphUnary::Squeeze),
            "minecraft:log" => Some(GraphUnary::Log),
            "minecraft:sign" => Some(GraphUnary::Sign),
            _ => None,
        };
        let binary = match kind.as_str() {
            "minecraft:add" => Some(GraphBinary::Add),
            "minecraft:sub" => Some(GraphBinary::Subtract),
            "minecraft:mul" => Some(GraphBinary::Multiply),
            "minecraft:div" => Some(GraphBinary::Divide),
            "minecraft:min" => Some(GraphBinary::Minimum),
            "minecraft:max" => Some(GraphBinary::Maximum),
            _ => None,
        };
        let node = if let Some(kind) = unary {
            GraphNode::Unary {
                kind,
                input: self.input(&object, "input", depth)?,
            }
        } else if let Some(kind) = binary {
            GraphNode::Binary {
                kind,
                left: self.input(&object, "left", depth)?,
                right: self.input(&object, "right", depth)?,
            }
        } else {
            self.decode_typed(&kind, &object, depth)?
        };
        Ok(self.graph.insert(node)?)
    }

    fn decode_typed(
        &mut self,
        kind: &str,
        object: &Object,
        depth: usize,
    ) -> Result<GraphNode, DecodeError> {
        let scale = |name: &'static str, low: f64, high: f64| {
            double_range(required(object, name)?, name, low, high)
        };
        Ok(match kind {
            "minecraft:constant" => GraphNode::Constant(noise_value(required(object, "value")?)?),
            "minecraft:blend_alpha" => GraphNode::WorldField(WorldField::BlendAlpha),
            "minecraft:blend_offset" => GraphNode::WorldField(WorldField::BlendOffset),
            "minecraft:beardifier" => GraphNode::WorldField(WorldField::Beardifier),
            "minecraft:end_outer_islands" => GraphNode::EndIslands,
            "minecraft:cache" => GraphNode::Cache(self.input(object, "input", depth)?),
            "minecraft:blend_density" => {
                GraphNode::BlendDensity(self.input(object, "input", depth)?)
            }
            "minecraft:pow" => GraphNode::Power {
                base: self.input(object, "base", depth)?,
                exponent: self.input(object, "exponent", depth)?,
            },
            "minecraft:floor" | "minecraft:round" | "minecraft:ceil" | "minecraft:truncate" => {
                GraphNode::Round {
                    mode: match kind {
                        "minecraft:floor" => RoundMode::Floor,
                        "minecraft:round" => RoundMode::Round,
                        "minecraft:ceil" => RoundMode::Ceil,
                        _ => RoundMode::Truncate,
                    },
                    input: self.input(object, "input", depth)?,
                    multiple: self.optional_input(object, "multiple", 1.0, depth)?,
                }
            }
            "minecraft:gradient" => {
                let from = integer(required(object, "from_coordinate")?)?;
                let to = integer(required(object, "to_coordinate")?)?;
                if from == to {
                    return Err(DecodeError::Field("gradient coordinates"));
                }
                GraphNode::Gradient {
                    axis: axis(required(object, "axis")?)?,
                    tiling: match optional(object, "tiling")
                        .map(string)
                        .transpose()?
                        .as_deref()
                        .unwrap_or("clamp_to_edge")
                    {
                        "clamp_to_edge" => TilingMode::ClampToEdge,
                        "repeat" => TilingMode::Repeat,
                        "mirrored_repeat" => TilingMode::MirroredRepeat,
                        _ => return Err(DecodeError::Field("tiling")),
                    },
                    from,
                    to,
                    low: noise_value(required(object, "from_value")?)?,
                    high: noise_value(required(object, "to_value")?)?,
                }
            }
            "minecraft:noise" => GraphNode::Noise {
                noise: self.noise(object)?,
                xz_scale: double(required(object, "xz_scale")?)?,
                y_scale: double(required(object, "y_scale")?)?,
                shift_x: self.optional_input(object, "shift_x", 0.0, depth)?,
                shift_y: self.optional_input(object, "shift_y", 0.0, depth)?,
                shift_z: self.optional_input(object, "shift_z", 0.0, depth)?,
            },
            "minecraft:shift" | "minecraft:shift_a" | "minecraft:shift_b" => {
                GraphNode::OffsetNoise {
                    noise: self.noise(object)?,
                    kind: match kind {
                        "minecraft:shift" => ShiftNoiseKind::Shift,
                        "minecraft:shift_a" => ShiftNoiseKind::ShiftA,
                        _ => ShiftNoiseKind::ShiftB,
                    },
                }
            }
            "minecraft:old_blended_noise" => GraphNode::BlendedNoise(BlendedNoise {
                xz_scale: scale("xz_scale", 0.001, 1000.0)?,
                y_scale: scale("y_scale", 0.001, 1000.0)?,
                xz_factor: scale("xz_factor", 0.001, 1000.0)?,
                y_factor: scale("y_factor", 0.001, 1000.0)?,
                smear_scale_multiplier: scale("smear_scale_multiplier", 1.0, 8.0)?,
            }),
            "minecraft:slice" => GraphNode::Slice {
                axis: axis(required(object, "axis")?)?,
                coordinate: integer(required(object, "coordinate")?)?,
                input: self.input(object, "input", depth)?,
            },
            "minecraft:interpolated" => GraphNode::Interpolated {
                input: self.input(object, "input", depth)?,
                xz: int_range(
                    required(object, "cell_size_xz")?,
                    "cell_size_xz",
                    1,
                    i32::MAX,
                )?,
                y: int_range(required(object, "cell_size_y")?, "cell_size_y", 1, i32::MAX)?,
            },
            "minecraft:find_top_surface" => GraphNode::Surface {
                density: self.input(object, "density", depth)?,
                upper: self.input(object, "upper_bound", depth)?,
                lower: int_range(required(object, "lower_bound")?, "lower_bound", -4064, 4062)?,
                height: int_range(required(object, "cell_height")?, "cell_height", 1, i32::MAX)?,
            },
            "minecraft:clamp" => {
                let min = noise_value(required(object, "min")?)?;
                let max = noise_value(required(object, "max")?)?;
                if max < min {
                    return Err(DecodeError::Field("clamp bounds"));
                }
                GraphNode::Clamp {
                    input: self.input(object, "input", depth)?,
                    min,
                    max,
                }
            }
            "minecraft:range_choice" => GraphNode::Choice {
                input: self.input(object, "input", depth)?,
                min: noise_value(required(object, "min_inclusive")?)?,
                max: noise_value(required(object, "max_exclusive")?)?,
                inside: self.input(object, "when_in_range", depth)?,
                outside: self.input(object, "when_out_of_range", depth)?,
            },
            "minecraft:lerp" => GraphNode::Lerp {
                alpha: self.input(object, "alpha", depth)?,
                first: self.input(object, "first", depth)?,
                second: self.input(object, "second", depth)?,
            },
            "minecraft:interval_select" => {
                let thresholds: Vec<Box<RawValue>> =
                    serde_json::from_str(required(object, "thresholds")?.get())?;
                let thresholds: Vec<f32> = thresholds
                    .iter()
                    .map(|raw| noise_value(raw))
                    .collect::<Result<_, _>>()?;
                let functions: Vec<Box<RawValue>> =
                    serde_json::from_str(required(object, "functions")?.get())?;
                if functions.len() < 2
                    || functions.len() > 256
                    || functions.len() != thresholds.len() + 1
                    || thresholds
                        .windows(2)
                        .any(|pair| pair[0].total_cmp(&pair[1]).is_gt())
                {
                    return Err(DecodeError::Field("interval selection"));
                }
                let input = self.input(object, "input", depth)?;
                let branches = functions
                    .iter()
                    .map(|raw| self.decode(raw, depth + 1))
                    .collect::<Result<_, _>>()?;
                GraphNode::IntervalSelect {
                    input,
                    thresholds: thresholds.into(),
                    branches,
                }
            }
            "minecraft:distance_to_point" => {
                let raw: Vec<Box<RawValue>> =
                    serde_json::from_str(required(object, "point")?.get())?;
                if raw.len() != 3 {
                    return Err(DecodeError::Field("point"));
                }
                let point = [integer(&raw[0])?, integer(&raw[1])?, integer(&raw[2])?];
                let metric = match string(required(object, "metric")?)?.as_str() {
                    "euclidean" => DistanceMetric::Euclidean,
                    "euclidean_squared" => DistanceMetric::EuclideanSquared,
                    "manhattan" => DistanceMetric::Manhattan,
                    "chebyshev" => DistanceMetric::Chebyshev,
                    _ => return Err(DecodeError::Field("metric")),
                };
                GraphNode::DistanceToPoint { point, metric }
            }
            "minecraft:spline" => {
                GraphNode::Spline(self.spline(required(object, "spline")?, depth + 1)?)
            }
            _ => {
                return Err(DecodeError::Registry(format!(
                    "unknown density type: {kind}"
                )));
            }
        })
    }

    fn spline(
        &mut self,
        raw: &RawValue,
        depth: usize,
    ) -> Result<CubicSpline<FunctionId>, DecodeError> {
        if depth > 256 {
            return Err(DecodeError::Field("spline nesting"));
        }
        if !raw.get().starts_with('{') {
            return Ok(CubicSpline::Constant(float(raw)?));
        }
        let object = object(raw)?;
        let coordinate = self.input(&object, "coordinate", depth)?;
        let points: Vec<Box<RawValue>> = serde_json::from_str(required(&object, "points")?.get())?;
        let points = points
            .iter()
            .map(|raw| {
                let point = self::object(raw)?;
                Ok(SplinePoint {
                    location: float(required(&point, "location")?)?,
                    derivative: float(required(&point, "derivative")?)?,
                    value: self.spline(required(&point, "value")?, depth + 1)?,
                })
            })
            .collect::<Result<_, DecodeError>>()?;
        CubicSpline::multipoint(coordinate, points).map_err(|e| GraphError::from(e).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Box<RawValue> {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn decimal_integers_wrap_without_floating_point_conversion() {
        for (text, expected) in [
            ("4294967295", -1),
            ("4294967296.5", 0),
            ("-4294967297.9", -1),
            ("1.9999999999999999999", 1),
            ("12e1", 120),
            ("123e-1", 12),
            ("-123e-3", 0),
            ("123e99", 0),
        ] {
            assert_eq!(integer(&raw(text)).unwrap(), expected, "{text}");
        }
    }

    #[test]
    fn floats_round_once_from_the_decimal() {
        // Through f64 this would round twice, to 1.0.
        assert_eq!(
            float(&raw("1.0000000596046447753906250000000001"))
                .unwrap()
                .to_bits(),
            0x3f80_0001
        );
        assert_eq!(float(&raw("-0")).unwrap().to_bits(), 0x8000_0000);
    }

    #[test]
    fn noise_codec_defaults_match_explicit_parameters() {
        let a = decode_noise(&raw(r#"{"base_octave":-7}"#)).unwrap();
        let b = decode_noise(&raw(r#"{"base_octave":-7,"base_amplitude":1,"octave_count":1,"normalize":true,"amplitude_modifiers":[],"unknown":42}"#)).unwrap();
        assert_eq!(a.range(), b.range());
        assert_eq!(a.normalization_factor(), b.normalization_factor());
        assert_eq!(a.octaves().len(), b.octaves().len());
        for bad in [
            r#"{"base_octave":-7,"octave_count":0}"#,
            r#"{"base_octave":-7,"normalize":0}"#,
            r#"{"base_octave":-7,"amplitude_modifiers":[-0]}"#,
            r#"{"base_octave":-7,"amplitude_modifiers":[-0.0]}"#,
            r#"{"base_octave":-7,"octave_count":2,"amplitude_modifiers":[1]}"#,
        ] {
            assert!(decode_noise(&raw(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn registry_references_resolve_forward_and_refuse_cycles() {
        let mut graph = DensityGraph::new();
        let mut loader = DensityLoader::new(&mut graph);
        loader
            .load_registry([
                (
                    "minecraft:a".to_owned(),
                    raw(r#"{"type":"add","left":"z","right":"minecraft:z"}"#),
                ),
                ("minecraft:z".to_owned(), raw("0.5")),
            ])
            .unwrap();
        assert!(graph.function("minecraft:a").is_some());
        let mut graph = DensityGraph::new();
        let cyclic = DensityLoader::new(&mut graph).load_registry([
            ("minecraft:a".to_owned(), raw(r#""b""#)),
            ("minecraft:b".to_owned(), raw(r#""a""#)),
        ]);
        assert!(cyclic.is_err());
        let mut graph = DensityGraph::new();
        let missing = DensityLoader::new(&mut graph)
            .load_registry([("minecraft:a".to_owned(), raw(r#""b""#))]);
        assert!(missing.is_err());
    }

    #[test]
    fn codec_validation_refuses_bad_gradients_intervals_and_splines() {
        for json in [
            r#"{"type":"gradient","axis":"y","from_coordinate":0,"to_coordinate":0,"from_value":0,"to_value":1}"#,
            r#"{"type":"interval_select","input":0,"thresholds":[0,-0],"functions":[0,1,2]}"#,
            r#"{"type":"interpolated","input":0,"cell_size_xz":0,"cell_size_y":8}"#,
            r#"{"type":"spline","spline":{"coordinate":0,"points":[]}}"#,
            r#"{"type":"clamp","input":0,"min":1,"max":-1}"#,
            r#"{"type":"constant","value":1e7}"#,
            r#"{"type":"no_such_type"}"#,
        ] {
            let mut graph = DensityGraph::new();
            let result = DensityLoader::new(&mut graph)
                .load_registry([("minecraft:a".to_owned(), raw(json))]);
            assert!(result.is_err(), "{json}");
        }
    }
}
