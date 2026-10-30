//! `CommandSyntaxException` with Minecraft's messages (`net.minecraft.commands.BrigadierExceptions`):
//! a translation key and its arguments, plus the input and cursor when the error has a position.

use std::fmt;

use super::{StringReader, utf16_offset};
use crate::azalea::chat::{
    FormattedText,
    translatable_component::{PrimitiveOrComponent, TranslatableComponent},
};

/// How many characters of input [`CommandError::context`] shows before the cursor
/// (`CommandSyntaxException.CONTEXT_AMOUNT`).
const CONTEXT_AMOUNT: usize = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct CommandError {
    key: &'static str,
    args: Vec<PrimitiveOrComponent>,
    /// The whole input and the byte cursor of the error, when it has a position (`createWithContext`).
    input: Option<Box<str>>,
    cursor: usize,
}

impl CommandError {
    /// An error whose message is the translation `key` with `args` (`Component.translatableEscape`).
    pub fn new(key: &'static str, args: Vec<PrimitiveOrComponent>) -> Self {
        Self {
            key,
            args,
            input: None,
            cursor: 0,
        }
    }

    /// This error at the reader's position (`createWithContext(reader)`).
    pub fn at(mut self, reader: &StringReader) -> Self {
        self.input = Some(reader.string().into());
        self.cursor = reader.cursor();
        self
    }

    /// The translation key; it also identifies the error's type (vanilla compares exception types).
    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn args(&self) -> &[PrimitiveOrComponent] {
        &self.args
    }

    pub fn input(&self) -> Option<&str> {
        self.input.as_deref()
    }

    /// The byte offset of the error in [`Self::input`].
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The position as Java reports it (UTF-16 units), for messages and packets.
    pub fn cursor_utf16(&self) -> usize {
        self.input
            .as_deref()
            .map_or(self.cursor, |input| utf16_offset(input, self.cursor))
    }

    /// `getRawMessage`: the message as a chat component.
    pub fn message(&self) -> FormattedText {
        TranslatableComponent::new(self.key.to_owned(), self.args.clone()).into()
    }

    /// `getContext`: up to ten characters before the cursor, then `<--[HERE]`.
    pub fn context(&self) -> Option<String> {
        let input = self.input.as_deref()?;
        let cursor = self.cursor.min(input.len());
        let before = &input[..cursor];
        let mut start = cursor;
        let mut units = 0;
        for c in before.chars().rev() {
            if units + c.len_utf16() > CONTEXT_AMOUNT {
                break;
            }
            units += c.len_utf16();
            start -= c.len_utf8();
        }
        let mut out = String::new();
        if utf16_offset(input, cursor) > CONTEXT_AMOUNT {
            out.push_str("...");
        }
        out.push_str(&input[start..cursor]);
        out.push_str("<--[HERE]");
        Some(out)
    }

    pub fn double_too_low(found: f64, min: f64) -> Self {
        Self::new(
            "argument.double.low",
            vec![min.into_arg(), found.into_arg()],
        )
    }
    pub fn double_too_high(found: f64, max: f64) -> Self {
        Self::new(
            "argument.double.big",
            vec![max.into_arg(), found.into_arg()],
        )
    }
    pub fn float_too_low(found: f32, min: f32) -> Self {
        Self::new("argument.float.low", vec![min.into_arg(), found.into_arg()])
    }
    pub fn float_too_high(found: f32, max: f32) -> Self {
        Self::new("argument.float.big", vec![max.into_arg(), found.into_arg()])
    }
    pub fn integer_too_low(found: i32, min: i32) -> Self {
        Self::new(
            "argument.integer.low",
            vec![min.into_arg(), found.into_arg()],
        )
    }
    pub fn integer_too_high(found: i32, max: i32) -> Self {
        Self::new(
            "argument.integer.big",
            vec![max.into_arg(), found.into_arg()],
        )
    }
    pub fn long_too_low(found: i64, min: i64) -> Self {
        Self::new("argument.long.low", vec![min.into_arg(), found.into_arg()])
    }
    pub fn long_too_high(found: i64, max: i64) -> Self {
        Self::new("argument.long.big", vec![max.into_arg(), found.into_arg()])
    }
    pub fn literal_incorrect(expected: &str) -> Self {
        Self::new("argument.literal.incorrect", vec![expected.into_arg()])
    }
    pub fn reader_expected_start_of_quote() -> Self {
        Self::new("parsing.quote.expected.start", Vec::new())
    }
    pub fn reader_expected_end_of_quote() -> Self {
        Self::new("parsing.quote.expected.end", Vec::new())
    }
    pub fn reader_invalid_escape(character: char) -> Self {
        Self::new("parsing.quote.escape", vec![character.into_arg()])
    }
    pub fn reader_invalid_bool(value: &str) -> Self {
        Self::new("parsing.bool.invalid", vec![value.into_arg()])
    }
    pub fn reader_invalid_int(value: &str) -> Self {
        Self::new("parsing.int.invalid", vec![value.into_arg()])
    }
    pub fn reader_expected_int() -> Self {
        Self::new("parsing.int.expected", Vec::new())
    }
    pub fn reader_invalid_long(value: &str) -> Self {
        Self::new("parsing.long.invalid", vec![value.into_arg()])
    }
    pub fn reader_expected_long() -> Self {
        Self::new("parsing.long.expected", Vec::new())
    }
    pub fn reader_invalid_double(value: &str) -> Self {
        Self::new("parsing.double.invalid", vec![value.into_arg()])
    }
    pub fn reader_expected_double() -> Self {
        Self::new("parsing.double.expected", Vec::new())
    }
    pub fn reader_invalid_float(value: &str) -> Self {
        Self::new("parsing.float.invalid", vec![value.into_arg()])
    }
    pub fn reader_expected_float() -> Self {
        Self::new("parsing.float.expected", Vec::new())
    }
    pub fn reader_expected_bool() -> Self {
        Self::new("parsing.bool.expected", Vec::new())
    }
    pub fn reader_expected_symbol(symbol: char) -> Self {
        Self::new("parsing.expected", vec![symbol.into_arg()])
    }
    pub fn dispatcher_unknown_command() -> Self {
        Self::new("command.unknown.command", Vec::new())
    }
    pub fn dispatcher_unknown_argument() -> Self {
        Self::new("command.unknown.argument", Vec::new())
    }
    pub fn dispatcher_expected_argument_separator() -> Self {
        Self::new("command.expected.separator", Vec::new())
    }
    pub fn dispatcher_parse_exception(message: &str) -> Self {
        Self::new("command.exception", vec![message.into_arg()])
    }

    // Minecraft's argument types.
    pub fn pos_missing_double() -> Self {
        Self::new("argument.pos.missing.double", Vec::new())
    }
    pub fn pos_missing_int() -> Self {
        Self::new("argument.pos.missing.int", Vec::new())
    }
    pub fn pos_mixed() -> Self {
        Self::new("argument.pos.mixed", Vec::new())
    }
    pub fn pos3d_incomplete() -> Self {
        Self::new("argument.pos3d.incomplete", Vec::new())
    }
    pub fn pos2d_incomplete() -> Self {
        Self::new("argument.pos2d.incomplete", Vec::new())
    }
    pub fn rotation_incomplete() -> Self {
        Self::new("argument.rotation.incomplete", Vec::new())
    }
    pub fn time_tick_count_too_low(ticks: i32, min: i32) -> Self {
        Self::new(
            "argument.time.tick_count_too_low",
            vec![min.into_arg(), ticks.into_arg()],
        )
    }
    pub fn color_invalid(name: &str) -> Self {
        Self::new("argument.color.invalid", vec![name.into_arg()])
    }
    /// `HexColorArgument.ERROR_INVALID_HEX`.
    pub fn hex_color_invalid(value: &str) -> Self {
        Self::new("argument.hexcolor.invalid", vec![value.into_arg()])
    }
}

/// `getMessage`: the translated message, then ` at position N: <context>` when it has a position.
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())?;
        if let Some(context) = self.context() {
            write!(f, " at position {}: {context}", self.cursor_utf16())?;
        }
        Ok(())
    }
}

impl std::error::Error for CommandError {}

/// A translation argument as `translatableEscape` keeps it: numbers and strings stay primitives (shown
/// with Java's `String.valueOf`), a `char` becomes a one-character string.
trait IntoArg {
    fn into_arg(self) -> PrimitiveOrComponent;
}
impl IntoArg for i32 {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::Integer(self)
    }
}
impl IntoArg for i64 {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::Long(self)
    }
}
impl IntoArg for f32 {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::Float(self)
    }
}
impl IntoArg for f64 {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::Double(self)
    }
}
impl IntoArg for &str {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::String(self.to_owned())
    }
}
impl IntoArg for char {
    fn into_arg(self) -> PrimitiveOrComponent {
        PrimitiveOrComponent::String(self.to_string())
    }
}
