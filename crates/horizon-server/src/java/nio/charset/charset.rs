//! `java.nio.charset`: `Charset`, `StandardCharsets`, `CharsetDecoder`/`CharsetEncoder`,
//! `CodingErrorAction`. Java strings are Rust `String`s, so decoding gives a `String` and encoding
//! takes a `&str`.
use crate::java::io::bytes::throwable;
use crate::java::lang::Throwable;
use crate::java::nio::ByteBuffer;

/// `java.nio.charset.Charset` (the charsets the server meets: UTF-8, US-ASCII, ISO-8859-1, UTF-16*).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Charset {
    name: &'static str,
}

/// `java.nio.charset.StandardCharsets`'s constants (Java `StandardCharsets.UTF_8` ->
/// `standard_charsets::UTF_8`, or `StandardCharsets::UTF_8`).
pub mod standard_charsets {
    use super::Charset;
    pub const US_ASCII: Charset = Charset { name: "US-ASCII" };
    pub const ISO_8859_1: Charset = Charset { name: "ISO-8859-1" };
    pub const UTF_8: Charset = Charset { name: "UTF-8" };
    pub const UTF_16BE: Charset = Charset { name: "UTF-16BE" };
    pub const UTF_16LE: Charset = Charset { name: "UTF-16LE" };
    pub const UTF_16: Charset = Charset { name: "UTF-16" };
}

/// `java.nio.charset.StandardCharsets`.
pub struct StandardCharsets;
impl StandardCharsets {
    pub const US_ASCII: Charset = standard_charsets::US_ASCII;
    pub const ISO_8859_1: Charset = standard_charsets::ISO_8859_1;
    pub const UTF_8: Charset = standard_charsets::UTF_8;
    pub const UTF_16BE: Charset = standard_charsets::UTF_16BE;
    pub const UTF_16LE: Charset = standard_charsets::UTF_16LE;
    pub const UTF_16: Charset = standard_charsets::UTF_16;
}

fn lookup(name: &str) -> Option<Charset> {
    let n: String = name.chars().filter(|c| *c != '-' && *c != '_').collect::<String>().to_ascii_lowercase();
    Some(match n.as_str() {
        "utf8" | "unicode11utf8" => standard_charsets::UTF_8,
        "usascii" | "ascii" | "iso646us" | "646" | "cp367" | "ascii7" | "default" => standard_charsets::US_ASCII,
        "iso88591" | "latin1" | "l1" | "iso885911987" | "cp819" | "ibm819" | "88591" | "isolatin1" => {
            standard_charsets::ISO_8859_1
        }
        "utf16" | "unicode" | "utf16bom" => standard_charsets::UTF_16,
        "utf16be" | "unicodebigunmarked" | "x utf16be" => standard_charsets::UTF_16BE,
        "utf16le" | "unicodelittleunmarked" | "x utf16le" => standard_charsets::UTF_16LE,
        _ => return None,
    })
}

impl Charset {
    /// `Charset.forName(String)`: panics with `UnsupportedCharsetException` as Java throws.
    pub fn for_name(name: &str) -> Charset {
        lookup(name).unwrap_or_else(|| panic!("java.nio.charset.UnsupportedCharsetException: {name}"))
    }
    /// `Charset.forName(String, Charset fallback)`.
    pub fn for_name_2(name: &str, fallback: Option<Charset>) -> Option<Charset> {
        lookup(name).or(fallback)
    }
    pub fn is_supported(name: &str) -> bool {
        lookup(name).is_some()
    }
    /// `Charset.defaultCharset()`: UTF-8 (Java 18+).
    pub fn default_charset() -> Charset {
        standard_charsets::UTF_8
    }
    pub fn name(&self) -> String {
        self.name.to_string()
    }
    pub fn display_name(&self) -> String {
        self.name.to_string()
    }
    pub fn aliases(&self) -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }
    pub fn is_registered(&self) -> bool {
        true
    }
    pub fn can_encode(&self) -> bool {
        true
    }
    pub fn contains(&self, cs: Charset) -> bool {
        self == &cs || self.name.starts_with("UTF")
    }
    pub fn to_string(&self) -> String {
        self.name.to_string()
    }
    pub fn new_decoder(&self) -> CharsetDecoder {
        CharsetDecoder { charset: *self, malformed: CodingErrorAction::REPORT, unmappable: CodingErrorAction::REPORT }
    }
    pub fn new_encoder(&self) -> CharsetEncoder {
        CharsetEncoder { charset: *self, malformed: CodingErrorAction::REPORT, unmappable: CodingErrorAction::REPORT }
    }
    /// `encode(String)`: malformed/unmappable characters become the replacement (`?`).
    pub fn encode(&self, s: &str) -> ByteBuffer {
        ByteBuffer::wrap_bytes(self.encode_str(s))
    }
    /// `decode(ByteBuffer)`: the buffer's remaining bytes (consumed), malformed input replaced.
    pub fn decode(&self, bb: ByteBuffer) -> String {
        let b = bb.take_remaining();
        self.decode_bytes(&b)
    }
    /// `String.getBytes(Charset)`'s body.
    pub fn encode_str(&self, s: &str) -> Vec<u8> {
        encode(*self, s, false).unwrap_or_default()
    }
    /// `new String(byte[], Charset)`'s body.
    pub fn decode_bytes(&self, b: &[u8]) -> String {
        let mut d = StreamDecoder::new(*self, false);
        let mut out = String::new();
        let _ = d.decode(b, true, &mut out);
        out
    }
    /// Decodes, reporting malformed input as `MalformedInputException`.
    pub fn decode_strict(&self, b: &[u8]) -> Result<String, Throwable> {
        let mut d = StreamDecoder::new(*self, true);
        let mut out = String::new();
        d.decode(b, true, &mut out)?;
        Ok(out)
    }
}

impl From<&str> for Charset {
    fn from(s: &str) -> Charset {
        Charset::for_name(s)
    }
}
impl From<&String> for Charset {
    fn from(s: &String) -> Charset {
        Charset::for_name(s)
    }
}
impl From<String> for Charset {
    fn from(s: String) -> Charset {
        Charset::for_name(&s)
    }
}
impl std::fmt::Display for Charset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

fn encode(cs: Charset, s: &str, strict: bool) -> Result<Vec<u8>, Throwable> {
    match cs.name {
        "UTF-8" => Ok(s.as_bytes().to_vec()),
        "US-ASCII" | "ISO-8859-1" => {
            let max = if cs.name == "US-ASCII" { 0x7F } else { 0xFF };
            let mut out = Vec::with_capacity(s.len());
            for c in s.chars() {
                if (c as u32) <= max {
                    out.push(c as u8);
                } else if strict {
                    return Err(throwable(
                        "java.nio.charset.UnmappableCharacterException",
                        "Input length = 1",
                    ));
                } else {
                    out.push(b'?');
                }
            }
            Ok(out)
        }
        "UTF-16BE" => Ok(s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()),
        "UTF-16LE" => Ok(s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()),
        _ => {
            // UTF-16: big-endian with a byte-order mark, as Java writes it.
            let mut out = vec![0xFE, 0xFF];
            out.extend(s.encode_utf16().flat_map(|u| u.to_be_bytes()));
            Ok(out)
        }
    }
}

/// An incremental decoder (what `InputStreamReader` reads through).
pub struct StreamDecoder {
    pub charset: Charset,
    pub strict: bool,
    pending: Vec<u8>,
    big_endian: bool,
    started: bool,
    high: Option<u16>,
}

impl StreamDecoder {
    pub fn new(charset: Charset, strict: bool) -> Self {
        StreamDecoder {
            charset,
            strict,
            pending: Vec::new(),
            big_endian: charset.name != "UTF-16LE",
            started: false,
            high: None,
        }
    }

    fn malformed(&self, n: usize, out: &mut String) -> Result<(), Throwable> {
        if self.strict {
            return Err(throwable("java.nio.charset.MalformedInputException", format!("Input length = {n}")));
        }
        out.push('\u{FFFD}');
        Ok(())
    }

    /// Decodes `input` (after any bytes held from the last call) into `out`; `eof` flushes what is held.
    pub fn decode(&mut self, input: &[u8], eof: bool, out: &mut String) -> Result<(), Throwable> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(input);
        match self.charset.name {
            "UTF-8" => {
                let mut rest: &[u8] = &data;
                loop {
                    match std::str::from_utf8(rest) {
                        Ok(s) => {
                            out.push_str(s);
                            break;
                        }
                        Err(e) => {
                            let good = e.valid_up_to();
                            out.push_str(std::str::from_utf8(&rest[..good]).unwrap());
                            match e.error_len() {
                                Some(n) => {
                                    self.malformed(n, out)?;
                                    rest = &rest[good + n..];
                                }
                                None => {
                                    if eof {
                                        self.malformed(rest.len() - good, out)?;
                                    } else {
                                        self.pending = rest[good..].to_vec();
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            "US-ASCII" => {
                for &b in &data {
                    if b < 0x80 {
                        out.push(b as char);
                    } else {
                        self.malformed(1, out)?;
                    }
                }
            }
            "ISO-8859-1" => out.extend(data.iter().map(|&b| b as char)),
            _ => {
                let mut i = 0;
                if self.charset.name == "UTF-16" && !self.started && data.len() >= 2 {
                    match (data[0], data[1]) {
                        (0xFE, 0xFF) => {
                            self.big_endian = true;
                            i = 2;
                        }
                        (0xFF, 0xFE) => {
                            self.big_endian = false;
                            i = 2;
                        }
                        _ => {}
                    }
                }
                if data.len() >= 2 {
                    self.started = true;
                }
                while i + 1 < data.len() {
                    let u = if self.big_endian {
                        u16::from_be_bytes([data[i], data[i + 1]])
                    } else {
                        u16::from_le_bytes([data[i], data[i + 1]])
                    };
                    i += 2;
                    if let Some(h) = self.high.take() {
                        if (0xDC00..0xE000).contains(&u) {
                            let c = 0x10000 + (((h as u32) - 0xD800) << 10) + ((u as u32) - 0xDC00);
                            out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                            continue;
                        }
                        self.malformed(2, out)?;
                    }
                    if (0xD800..0xDC00).contains(&u) {
                        self.high = Some(u);
                    } else if (0xDC00..0xE000).contains(&u) {
                        self.malformed(2, out)?;
                    } else {
                        out.push(char::from_u32(u as u32).unwrap_or('\u{FFFD}'));
                    }
                }
                if i < data.len() {
                    if eof {
                        self.malformed(1, out)?;
                    } else {
                        self.pending = data[i..].to_vec();
                    }
                }
                if eof && self.high.take().is_some() {
                    self.malformed(2, out)?;
                }
            }
        }
        Ok(())
    }
}

/// `java.nio.charset.CodingErrorAction`.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CodingErrorAction {
    IGNORE,
    REPLACE,
    REPORT,
}

/// `java.nio.charset.CharsetDecoder` (a value: its builder methods return the updated decoder).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CharsetDecoder {
    pub charset: Charset,
    pub malformed: CodingErrorAction,
    pub unmappable: CodingErrorAction,
}
impl CharsetDecoder {
    pub fn charset(&self) -> Charset {
        self.charset
    }
    pub fn on_malformed_input(mut self, action: CodingErrorAction) -> Self {
        self.malformed = action;
        self
    }
    pub fn on_unmappable_character(mut self, action: CodingErrorAction) -> Self {
        self.unmappable = action;
        self
    }
    pub fn malformed_input_action(&self) -> CodingErrorAction {
        self.malformed
    }
    pub fn unmappable_character_action(&self) -> CodingErrorAction {
        self.unmappable
    }
    pub fn reset(self) -> Self {
        self
    }
    /// Whether malformed input throws (`REPORT`) rather than being replaced.
    pub fn is_strict(&self) -> bool {
        self.malformed == CodingErrorAction::REPORT
    }
    /// `decode(ByteBuffer)`: the remaining bytes (consumed); `CharacterCodingException` on `REPORT`.
    pub fn decode(&self, bb: ByteBuffer) -> Result<String, Throwable> {
        let b = bb.take_remaining();
        let mut d = StreamDecoder::new(self.charset, self.is_strict());
        let mut out = String::new();
        d.decode(&b, true, &mut out)?;
        Ok(out)
    }
}
impl From<Charset> for CharsetDecoder {
    fn from(c: Charset) -> Self {
        CharsetDecoder { charset: c, malformed: CodingErrorAction::REPLACE, unmappable: CodingErrorAction::REPLACE }
    }
}
impl From<&str> for CharsetDecoder {
    fn from(c: &str) -> Self {
        Charset::for_name(c).into()
    }
}

/// `java.nio.charset.CharsetEncoder`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CharsetEncoder {
    pub charset: Charset,
    pub malformed: CodingErrorAction,
    pub unmappable: CodingErrorAction,
}
impl CharsetEncoder {
    pub fn charset(&self) -> Charset {
        self.charset
    }
    pub fn on_malformed_input(mut self, action: CodingErrorAction) -> Self {
        self.malformed = action;
        self
    }
    pub fn on_unmappable_character(mut self, action: CodingErrorAction) -> Self {
        self.unmappable = action;
        self
    }
    pub fn can_encode(&self, c: char) -> bool {
        match self.charset.name {
            "US-ASCII" => (c as u32) < 0x80,
            "ISO-8859-1" => (c as u32) < 0x100,
            _ => true,
        }
    }
    pub fn can_encode_str(&self, s: &str) -> bool {
        s.chars().all(|c| self.can_encode(c))
    }
    pub fn is_strict(&self) -> bool {
        self.unmappable == CodingErrorAction::REPORT
    }
    /// `encode(CharBuffer)`.
    pub fn encode(&self, s: &str) -> Result<ByteBuffer, Throwable> {
        Ok(ByteBuffer::wrap_bytes(self.encode_bytes(s)?))
    }
    pub fn encode_bytes(&self, s: &str) -> Result<Vec<u8>, Throwable> {
        encode(self.charset, s, self.is_strict())
    }
}
impl From<Charset> for CharsetEncoder {
    fn from(c: Charset) -> Self {
        CharsetEncoder { charset: c, malformed: CodingErrorAction::REPLACE, unmappable: CodingErrorAction::REPLACE }
    }
}
impl From<&str> for CharsetEncoder {
    fn from(c: &str) -> Self {
        Charset::for_name(c).into()
    }
}
