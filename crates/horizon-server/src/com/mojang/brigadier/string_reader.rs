//! Port of `com.mojang.brigadier.StringReader` (Brigadier 1.3.11).
//!
//! A shared handle, as a Java reference is: clones read and move the same cursor; `new_1(&other)` is
//! Java's copy constructor `new StringReader(other)`. Positions are UTF-16 code units, as in Java.
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::{
    CommandSyntaxException, built_in_exceptions,
};
use crate::com::mojang::brigadier::java_compat::{self, boxed};
use crate::prelude::*;

pub const SYNTAX_ESCAPE: char = '\\';
pub const SYNTAX_DOUBLE_QUOTE: char = '"';
pub const SYNTAX_SINGLE_QUOTE: char = '\'';

pub struct StringReaderData {
    pub string: Arc<str>,
    pub units: Arc<[u16]>,
    pub cursor: Mut<i32>,
}

// ---- class com.mojang.brigadier.StringReader implements ImmutableStringReader
#[derive(Clone)]
pub struct StringReader(pub JRef<StringReaderData>);

impl std::fmt::Debug for StringReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StringReader({:?} @ {})", &*self.0.string, self.get_cursor())
    }
}

impl StringReader {
    /// `new StringReader(String)`.
    pub fn new(string: &str) -> StringReader {
        StringReader(Arc::new(StringReaderData {
            string: Arc::from(string),
            units: string.encode_utf16().collect::<Vec<u16>>().into(),
            cursor: Mut::new(0),
        }))
    }

    /// `new StringReader(StringReader other)`: a copy with its own cursor.
    pub fn new_1(other: &StringReader) -> StringReader {
        StringReader(Arc::new(StringReaderData {
            string: other.0.string.clone(),
            units: other.0.units.clone(),
            cursor: Mut::new(other.0.cursor.get()),
        }))
    }

    /// The string without copying it (Rust convenience for `getString()`).
    pub fn as_str(&self) -> &str {
        &self.0.string
    }

    pub fn get_string(&self) -> String {
        self.0.string.to_string()
    }

    pub fn set_cursor(&self, cursor: i32) {
        self.0.cursor.set(cursor);
    }

    pub fn get_remaining_length(&self) -> i32 {
        self.0.units.len() as i32 - self.get_cursor()
    }

    pub fn get_total_length(&self) -> i32 {
        self.0.units.len() as i32
    }

    pub fn get_cursor(&self) -> i32 {
        self.0.cursor.get()
    }

    fn slice(&self, start: i32, end: i32) -> String {
        String::from_utf16_lossy(&self.0.units[start as usize..end as usize])
    }

    pub fn get_read(&self) -> String {
        self.slice(0, self.get_cursor())
    }

    pub fn get_remaining(&self) -> String {
        self.slice(self.get_cursor(), self.get_total_length())
    }

    /// `canRead(int length)`.
    pub fn can_read_1(&self, length: i32) -> bool {
        self.get_cursor() + length <= self.get_total_length()
    }

    /// `canRead()`.
    pub fn can_read(&self) -> bool {
        self.can_read_1(1)
    }

    fn char_at(&self, index: i32) -> char {
        if index < 0 || index as usize >= self.0.units.len() {
            panic!("StringIndexOutOfBoundsException: index {index}, length {}", self.0.units.len());
        }
        java_compat::unit_to_char(self.0.units[index as usize])
    }

    /// `peek()`.
    pub fn peek(&self) -> char {
        self.char_at(self.get_cursor())
    }

    /// `peek(int offset)`.
    pub fn peek_1(&self, offset: i32) -> char {
        self.char_at(self.get_cursor() + offset)
    }

    pub fn read(&self) -> char {
        let c = self.char_at(self.get_cursor());
        self.0.cursor.set(self.get_cursor() + 1);
        c
    }

    pub fn skip(&self) {
        self.0.cursor.set(self.get_cursor() + 1);
    }

    pub fn is_allowed_number(c: char) -> bool {
        c >= '0' && c <= '9' || c == '.' || c == '-'
    }

    pub fn is_quoted_string_start(c: char) -> bool {
        c == SYNTAX_DOUBLE_QUOTE || c == SYNTAX_SINGLE_QUOTE
    }

    pub fn skip_whitespace(&self) {
        while self.can_read() && java_compat::is_whitespace(self.peek()) {
            self.skip();
        }
    }

    fn read_number_text(&self) -> (i32, String) {
        let start = self.get_cursor();
        while self.can_read() && StringReader::is_allowed_number(self.peek()) {
            self.skip();
        }
        (start, self.slice(start, self.get_cursor()))
    }

    pub fn read_int(&self) -> Result<i32, CommandSyntaxException> {
        let (start, number) = self.read_number_text();
        if number.is_empty() {
            return Err(built_in_exceptions().reader_expected_int().create_with_context(self));
        }
        match number.parse::<i32>() {
            Ok(v) => Ok(v),
            Err(_) => {
                self.set_cursor(start);
                Err(built_in_exceptions().reader_invalid_int().create_with_context(self, boxed(number)))
            }
        }
    }

    pub fn read_long(&self) -> Result<i64, CommandSyntaxException> {
        let (start, number) = self.read_number_text();
        if number.is_empty() {
            return Err(built_in_exceptions().reader_expected_long().create_with_context(self));
        }
        match number.parse::<i64>() {
            Ok(v) => Ok(v),
            Err(_) => {
                self.set_cursor(start);
                Err(built_in_exceptions().reader_invalid_long().create_with_context(self, boxed(number)))
            }
        }
    }

    pub fn read_double(&self) -> Result<f64, CommandSyntaxException> {
        let (start, number) = self.read_number_text();
        if number.is_empty() {
            return Err(built_in_exceptions().reader_expected_double().create_with_context(self));
        }
        match number.parse::<f64>() {
            Ok(v) => Ok(v),
            Err(_) => {
                self.set_cursor(start);
                Err(built_in_exceptions().reader_invalid_double().create_with_context(self, boxed(number)))
            }
        }
    }

    pub fn read_float(&self) -> Result<f32, CommandSyntaxException> {
        let (start, number) = self.read_number_text();
        if number.is_empty() {
            return Err(built_in_exceptions().reader_expected_float().create_with_context(self));
        }
        match number.parse::<f32>() {
            Ok(v) => Ok(v),
            Err(_) => {
                self.set_cursor(start);
                Err(built_in_exceptions().reader_invalid_float().create_with_context(self, boxed(number)))
            }
        }
    }

    pub fn is_allowed_in_unquoted_string(c: char) -> bool {
        c >= '0' && c <= '9'
            || c >= 'A' && c <= 'Z'
            || c >= 'a' && c <= 'z'
            || c == '_'
            || c == '-'
            || c == '.'
            || c == '+'
    }

    pub fn read_unquoted_string(&self) -> String {
        let start = self.get_cursor();
        while self.can_read() && StringReader::is_allowed_in_unquoted_string(self.peek()) {
            self.skip();
        }
        self.slice(start, self.get_cursor())
    }

    pub fn read_quoted_string(&self) -> Result<String, CommandSyntaxException> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if !StringReader::is_quoted_string_start(next) {
            return Err(built_in_exceptions().reader_expected_start_of_quote().create_with_context(self));
        }
        self.skip();
        self.read_string_until(next)
    }

    pub fn read_string_until(&self, terminator: char) -> Result<String, CommandSyntaxException> {
        let mut result: Vec<u16> = Vec::new();
        let mut escaped = false;
        while self.can_read() {
            let unit = self.0.units[self.get_cursor() as usize];
            let c = self.read();
            if escaped {
                if c == terminator || c == SYNTAX_ESCAPE {
                    result.push(unit);
                    escaped = false;
                } else {
                    self.set_cursor(self.get_cursor() - 1);
                    return Err(built_in_exceptions()
                        .reader_invalid_escape()
                        .create_with_context(self, boxed(c.to_string())));
                }
            } else if c == SYNTAX_ESCAPE {
                escaped = true;
            } else if c == terminator {
                return Ok(String::from_utf16_lossy(&result));
            } else {
                result.push(unit);
            }
        }
        Err(built_in_exceptions().reader_expected_end_of_quote().create_with_context(self))
    }

    pub fn read_string(&self) -> Result<String, CommandSyntaxException> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if StringReader::is_quoted_string_start(next) {
            self.skip();
            return self.read_string_until(next);
        }
        Ok(self.read_unquoted_string())
    }

    pub fn read_boolean(&self) -> Result<bool, CommandSyntaxException> {
        let start = self.get_cursor();
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(built_in_exceptions().reader_expected_bool().create_with_context(self));
        }
        if value == "true" {
            Ok(true)
        } else if value == "false" {
            Ok(false)
        } else {
            self.set_cursor(start);
            Err(built_in_exceptions().reader_invalid_bool().create_with_context(self, boxed(value)))
        }
    }

    pub fn expect(&self, c: char) -> Result<(), CommandSyntaxException> {
        if !self.can_read() || self.peek() != c {
            return Err(built_in_exceptions()
                .reader_expected_symbol()
                .create_with_context(self, boxed(c.to_string())));
        }
        self.skip();
        Ok(())
    }

    /// Whether the units `[start, start + text.length())` equal `text` (Java:
    /// `getString().substring(start, end).equals(text)`).
    pub fn region_matches(&self, start: i32, text: &str) -> bool {
        let units: Vec<u16> = text.encode_utf16().collect();
        let end = start as usize + units.len();
        start >= 0 && end <= self.0.units.len() && self.0.units[start as usize..end] == units[..]
    }
}
