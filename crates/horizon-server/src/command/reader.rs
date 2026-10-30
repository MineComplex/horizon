//! Brigadier's `StringReader`: a cursor over one command line.
//!
//! The cursor is a byte offset that always sits on a `char` boundary. Brigadier counts UTF-16 units; the
//! two agree on ASCII, and [`super::utf16_offset`] converts where a position leaves the server (error
//! positions, suggestion ranges).

use super::CommandError;

#[derive(Clone, Copy, Debug)]
pub struct StringReader<'a> {
    string: &'a str,
    cursor: usize,
}

impl<'a> StringReader<'a> {
    pub fn new(string: &'a str) -> Self {
        Self { string, cursor: 0 }
    }

    /// A reader at `cursor` (a byte offset on a char boundary).
    pub fn at(string: &'a str, cursor: usize) -> Self {
        debug_assert!(string.is_char_boundary(cursor));
        Self { string, cursor }
    }

    pub fn string(&self) -> &'a str {
        self.string
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        debug_assert!(self.string.is_char_boundary(cursor));
        self.cursor = cursor;
    }

    pub fn remaining(&self) -> &'a str {
        &self.string[self.cursor..]
    }

    pub fn read_so_far(&self) -> &'a str {
        &self.string[..self.cursor]
    }

    pub fn can_read(&self) -> bool {
        self.cursor < self.string.len()
    }

    /// `canRead(length)`: at least `length` more characters.
    pub fn can_read_n(&self, length: usize) -> bool {
        self.remaining()
            .chars()
            .nth(length.saturating_sub(1))
            .is_some()
            || length == 0
    }

    /// The next character, or `'\0'` at the end (Brigadier would throw; callers check `can_read` first).
    pub fn peek(&self) -> char {
        self.remaining().chars().next().unwrap_or('\0')
    }

    pub fn peek_at(&self, offset: usize) -> char {
        self.remaining().chars().nth(offset).unwrap_or('\0')
    }

    pub fn skip(&mut self) {
        self.cursor += self.peek().len_utf8();
    }

    pub fn read(&mut self) -> char {
        let c = self.peek();
        self.skip();
        c
    }

    /// `skipWhitespace` (`Character.isWhitespace`).
    pub fn skip_whitespace(&mut self) {
        while self.can_read() && is_java_whitespace(self.peek()) {
            self.skip();
        }
    }

    pub fn is_allowed_number(c: char) -> bool {
        c.is_ascii_digit() || c == '.' || c == '-'
    }

    pub fn is_quoted_string_start(c: char) -> bool {
        c == '"' || c == '\''
    }

    pub fn is_allowed_in_unquoted_string(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
    }

    fn read_number_text(&mut self) -> &'a str {
        let start = self.cursor;
        while self.can_read() && Self::is_allowed_number(self.peek()) {
            self.skip();
        }
        &self.string[start..self.cursor]
    }

    /// The four number readers: read `[0-9.-]*`, then parse it the way Java's `Integer.parseInt`,
    /// `Long.parseLong`, `Float.parseFloat` and `Double.parseDouble` would. Over these characters Rust's
    /// `FromStr` accepts exactly the same strings (`"5."`, `".5"`, `"-.5"`; not `"."`, `"-"`, `"1-2"`)
    /// and rounds floats the same way (correctly rounded).
    fn read_number<T: std::str::FromStr>(
        &mut self,
        expected: fn() -> CommandError,
        invalid: fn(&str) -> CommandError,
    ) -> Result<T, CommandError> {
        let start = self.cursor;
        let text = self.read_number_text();
        if text.is_empty() {
            return Err(expected().at(self));
        }
        text.parse().map_err(|_| {
            self.cursor = start;
            invalid(text).at(self)
        })
    }

    pub fn read_int(&mut self) -> Result<i32, CommandError> {
        self.read_number(
            CommandError::reader_expected_int,
            CommandError::reader_invalid_int,
        )
    }

    pub fn read_long(&mut self) -> Result<i64, CommandError> {
        self.read_number(
            CommandError::reader_expected_long,
            CommandError::reader_invalid_long,
        )
    }

    pub fn read_float(&mut self) -> Result<f32, CommandError> {
        self.read_number(
            CommandError::reader_expected_float,
            CommandError::reader_invalid_float,
        )
    }

    pub fn read_double(&mut self) -> Result<f64, CommandError> {
        self.read_number(
            CommandError::reader_expected_double,
            CommandError::reader_invalid_double,
        )
    }

    pub fn read_unquoted_string(&mut self) -> &'a str {
        let start = self.cursor;
        while self.can_read() && Self::is_allowed_in_unquoted_string(self.peek()) {
            self.skip();
        }
        &self.string[start..self.cursor]
    }

    pub fn read_quoted_string(&mut self) -> Result<String, CommandError> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if !Self::is_quoted_string_start(next) {
            return Err(CommandError::reader_expected_start_of_quote().at(self));
        }
        self.skip();
        self.read_string_until(next)
    }

    pub fn read_string_until(&mut self, terminator: char) -> Result<String, CommandError> {
        let mut result = String::new();
        let mut escaped = false;
        while self.can_read() {
            let c = self.read();
            if escaped {
                if c == terminator || c == '\\' {
                    result.push(c);
                    escaped = false;
                } else {
                    self.cursor -= c.len_utf8();
                    return Err(CommandError::reader_invalid_escape(c).at(self));
                }
            } else if c == '\\' {
                escaped = true;
            } else if c == terminator {
                return Ok(result);
            } else {
                result.push(c);
            }
        }
        Err(CommandError::reader_expected_end_of_quote().at(self))
    }

    pub fn read_string(&mut self) -> Result<String, CommandError> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if Self::is_quoted_string_start(next) {
            self.skip();
            return self.read_string_until(next);
        }
        Ok(self.read_unquoted_string().to_owned())
    }

    pub fn read_boolean(&mut self) -> Result<bool, CommandError> {
        let start = self.cursor;
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(CommandError::reader_expected_bool().at(self));
        }
        match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => {
                self.cursor = start;
                Err(CommandError::reader_invalid_bool(&value).at(self))
            }
        }
    }

    pub fn expect(&mut self, c: char) -> Result<(), CommandError> {
        if !self.can_read() || self.peek() != c {
            return Err(CommandError::reader_expected_symbol(c).at(self));
        }
        self.skip();
        Ok(())
    }
}

/// `Character.isWhitespace`: Unicode spaces except the non-breaking ones, plus the ASCII separators.
fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}'
    ) || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
}
