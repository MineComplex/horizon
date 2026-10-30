//! Argument types and the values they parse to.
//!
//! The set of argument types is closed (Minecraft registers them in `ArgumentTypeInfos` and sends their
//! ids in the commands packet), so both the parser and its result are enums: no `Box<dyn ArgumentType>`
//! and no `Any` values cast back at run time. Brigadier's own types are here; Minecraft's (entity
//! selectors, positions, resources...) join as variants when the commands that use them are ported.

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
    Integer { min: i32, max: i32 },
    Long { min: i64, max: i64 },
    Float { min: f32, max: f32 },
    Double { min: f64, max: f64 },
    String(StringKind),
}

impl ArgumentType {
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
        })
    }

    /// `ArgumentType.listSuggestions`: only booleans suggest anything by themselves.
    pub fn list_suggestions(&self, builder: &mut SuggestionsBuilder) {
        if let Self::Bool = self {
            for value in ["true", "false"] {
                if value.starts_with(builder.remaining_lowercase()) {
                    builder.suggest(value);
                }
            }
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
}
