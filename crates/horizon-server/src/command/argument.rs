//! Argument types and the values they parse to.
//!
//! The set of argument types is closed (Minecraft registers them in `ArgumentTypeInfos` and sends their
//! ids in the commands packet), so both the parser and its result are enums: no `Box<dyn ArgumentType>`
//! and no `Any` values cast back at run time. Brigadier's own types are here, and Minecraft's that
//! need no registry or world (positions, ranges, durations...); the others (entity selectors,
//! resources...) join as variants when the commands that use them are ported.

use uuid::Uuid;

use super::coordinates::{Coordinates, WorldCoordinate, suggest_coordinates};
use super::values::{
    Angle, DoubleRange, IntRange, ScoreOperation, Swizzle, TeamColor, parse_time, parse_uuid,
    suggest_matching, suggest_time_units,
};
use super::{CommandError, StringReader, SuggestionsBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StringKind {
    /// `StringArgumentType.word()`: an unquoted word.
    SingleWord,
    /// `string()`: a quoted string or a word.
    QuotablePhrase,
    /// `greedyString()`: the rest of the input.
    GreedyPhrase,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArgumentType {
    Bool,
    Integer {
        min: i32,
        max: i32,
    },
    Long {
        min: i64,
        max: i64,
    },
    Float {
        min: f32,
        max: f32,
    },
    Double {
        min: f64,
        max: f64,
    },
    String(StringKind),
    /// `block_pos`: three integers (absolute) or offsets, or local coordinates.
    BlockPos,
    /// `column_pos`: `x z` integers or offsets.
    ColumnPos,
    /// `vec3`: three numbers or offsets, or local coordinates. `center_correct` (default `true`, the
    /// only form the wire carries) centres whole absolute `x` and `z` in their block.
    Vec3 {
        center_correct: bool,
    },
    /// `vec2`: `x z` numbers or offsets.
    Vec2 {
        center_correct: bool,
    },
    /// `rotation`: `yaw pitch`, each a number or an offset.
    Rotation,
    /// `angle`: one angle or offset in degrees.
    Angle,
    /// `swizzle`: a set of axes, like `xz`.
    Swizzle,
    /// `int_range`: `1`, `1..5`, `..5`, `1..`.
    IntRange,
    /// `float_range`: the same with doubles.
    FloatRange,
    /// `time`: a duration with an optional unit (`d`, `s`, `t`), parsed to ticks no fewer than `min`.
    Time {
        min: i32,
    },
    /// `uuid`: a hyphenated UUID.
    Uuid,
    /// `operation`: a scoreboard operation (`=`, `+=`, `><`...).
    Operation,
    /// `team_color`: a colour name.
    TeamColor,
}

impl ArgumentType {
    pub const fn vec3() -> Self {
        Self::Vec3 {
            center_correct: true,
        }
    }
    pub const fn vec2() -> Self {
        Self::Vec2 {
            center_correct: true,
        }
    }
    pub const fn time() -> Self {
        Self::Time { min: 0 }
    }
    pub const fn integer() -> Self {
        Self::Integer {
            min: i32::MIN,
            max: i32::MAX,
        }
    }
    pub const fn integer_min(min: i32) -> Self {
        Self::Integer { min, max: i32::MAX }
    }
    pub const fn long() -> Self {
        Self::Long {
            min: i64::MIN,
            max: i64::MAX,
        }
    }
    pub const fn float() -> Self {
        Self::Float {
            min: -f32::MAX,
            max: f32::MAX,
        }
    }
    pub const fn double() -> Self {
        Self::Double {
            min: -f64::MAX,
            max: f64::MAX,
        }
    }
    pub const fn word() -> Self {
        Self::String(StringKind::SingleWord)
    }
    pub const fn string() -> Self {
        Self::String(StringKind::QuotablePhrase)
    }
    pub const fn greedy_string() -> Self {
        Self::String(StringKind::GreedyPhrase)
    }

    /// `ArgumentType.parse`. On a bound error the cursor goes back to the argument's start.
    pub fn parse(&self, reader: &mut StringReader) -> Result<ArgumentValue, CommandError> {
        let start = reader.cursor();
        let out_of_range = |reader: &mut StringReader, error: CommandError| {
            reader.set_cursor(start);
            Err(error.at(reader))
        };
        Ok(match *self {
            Self::Bool => ArgumentValue::Bool(reader.read_boolean()?),
            Self::Integer { min, max } => {
                let value = reader.read_int()?;
                if value < min {
                    return out_of_range(reader, CommandError::integer_too_low(value, min));
                }
                if value > max {
                    return out_of_range(reader, CommandError::integer_too_high(value, max));
                }
                ArgumentValue::Integer(value)
            }
            Self::Long { min, max } => {
                let value = reader.read_long()?;
                if value < min {
                    return out_of_range(reader, CommandError::long_too_low(value, min));
                }
                if value > max {
                    return out_of_range(reader, CommandError::long_too_high(value, max));
                }
                ArgumentValue::Long(value)
            }
            // NaN passes both bounds, as in Java.
            Self::Float { min, max } => {
                let value = reader.read_float()?;
                if value < min {
                    return out_of_range(reader, CommandError::float_too_low(value, min));
                }
                if value > max {
                    return out_of_range(reader, CommandError::float_too_high(value, max));
                }
                ArgumentValue::Float(value)
            }
            Self::Double { min, max } => {
                let value = reader.read_double()?;
                if value < min {
                    return out_of_range(reader, CommandError::double_too_low(value, min));
                }
                if value > max {
                    return out_of_range(reader, CommandError::double_too_high(value, max));
                }
                ArgumentValue::Double(value)
            }
            Self::String(StringKind::SingleWord) => {
                ArgumentValue::String(reader.read_unquoted_string().to_owned())
            }
            Self::String(StringKind::QuotablePhrase) => {
                ArgumentValue::String(reader.read_string()?)
            }
            Self::String(StringKind::GreedyPhrase) => {
                let text = reader.remaining().to_owned();
                reader.set_cursor(reader.string().len());
                ArgumentValue::String(text)
            }
            Self::BlockPos => ArgumentValue::Coordinates(Coordinates::parse_block_pos(reader)?),
            Self::Vec3 { center_correct } => {
                ArgumentValue::Coordinates(Coordinates::parse_vec3(reader, center_correct)?)
            }
            Self::ColumnPos => ArgumentValue::Coordinates(Coordinates::parse_column(
                reader,
                WorldCoordinate::parse_int,
            )?),
            Self::Vec2 { center_correct } => {
                ArgumentValue::Coordinates(Coordinates::parse_column(reader, |r| {
                    WorldCoordinate::parse_double(r, center_correct)
                })?)
            }
            Self::Rotation => ArgumentValue::Coordinates(Coordinates::parse_rotation(reader)?),
            Self::Angle => ArgumentValue::Angle(Angle::parse(reader)?),
            Self::Swizzle => ArgumentValue::Swizzle(Swizzle::parse(reader)?),
            Self::IntRange => ArgumentValue::IntRange(IntRange::parse(reader)?),
            Self::FloatRange => ArgumentValue::FloatRange(DoubleRange::parse(reader)?),
            // An `Integer`, as in vanilla.
            Self::Time { min } => ArgumentValue::Integer(parse_time(reader, min)?),
            Self::Uuid => ArgumentValue::Uuid(parse_uuid(reader)?),
            Self::Operation => ArgumentValue::Operation(ScoreOperation::parse(reader)?),
            Self::TeamColor => ArgumentValue::TeamColor(TeamColor::parse(reader)?),
        })
    }

    /// `ArgumentType.listSuggestions`, as for a server's command source.
    pub fn list_suggestions(&self, builder: &mut SuggestionsBuilder) {
        match self {
            Self::Bool => {
                for value in ["true", "false"] {
                    if value.starts_with(builder.remaining_lowercase()) {
                        builder.suggest(value);
                    }
                }
            }
            Self::BlockPos | Self::Vec3 { .. } => suggest_coordinates(self, builder, true),
            Self::ColumnPos | Self::Vec2 { .. } => suggest_coordinates(self, builder, false),
            Self::Time { .. } => suggest_time_units(builder),
            Self::Operation => {
                suggest_matching(builder, ScoreOperation::ALL.iter().map(|op| op.symbol()))
            }
            Self::TeamColor => suggest_matching(builder, TeamColor::ALL.iter().map(|c| c.name())),
            _ => {}
        }
    }

    /// `ArgumentType.getExamples` (for [`super::CommandTree::find_ambiguities`]).
    pub fn examples(&self) -> &'static [&'static str] {
        match self {
            Self::Bool => &["true", "false"],
            Self::Integer { .. } | Self::Long { .. } => &["0", "123", "-123"],
            Self::Float { .. } | Self::Double { .. } => {
                &["0", "1.2", ".5", "-1", "-.5", "-1234.56"]
            }
            Self::String(StringKind::SingleWord) => &["word", "words_with_underscores"],
            Self::String(StringKind::QuotablePhrase) => &["\"quoted phrase\"", "word", "\"\""],
            Self::String(StringKind::GreedyPhrase) => {
                &["word", "words with spaces", "\"and symbols\""]
            }
            Self::BlockPos => &["0 0 0", "~ ~ ~", "^ ^ ^", "^1 ^ ^-5", "~0.5 ~1 ~-5"],
            Self::ColumnPos => &["0 0", "~ ~", "~1 ~-2", "^ ^", "^-1 ^0"],
            Self::Vec3 { .. } => &[
                "0 0 0",
                "~ ~ ~",
                "^ ^ ^",
                "^1 ^ ^-5",
                "0.1 -0.5 .9",
                "~0.5 ~1 ~-5",
            ],
            Self::Vec2 { .. } => &["0 0", "~ ~", "0.1 -0.5", "~1 ~-2"],
            Self::Rotation => &["0 0", "~ ~", "~-5 ~5"],
            Self::Angle => &["0", "~", "~-5"],
            Self::Swizzle => &["xyz", "x"],
            Self::IntRange => &["0..5", "0", "-5", "-100..", "..100"],
            Self::FloatRange => &["0..5.2", "0", "-5.4", "-100.76..", "..100"],
            Self::Time { .. } => &["0d", "0s", "0t", "0"],
            Self::Uuid => &["dd12be42-52a9-4a91-a8a1-11c01849e498"],
            Self::Operation => &["=", ">", "<"],
            Self::TeamColor => &["red", "green"],
        }
    }
}

/// A parsed argument value.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgumentValue {
    Bool(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    /// `block_pos`, `column_pos`, `vec3`, `vec2` and `rotation`.
    Coordinates(Coordinates),
    Angle(Angle),
    Swizzle(Swizzle),
    IntRange(IntRange),
    FloatRange(DoubleRange),
    Uuid(Uuid),
    Operation(ScoreOperation),
    TeamColor(TeamColor),
}

#[cfg(test)]
#[path = "argument_tests.rs"]
mod tests;
