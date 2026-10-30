//! `java.io.DataInput` (a handle over the stream it reads) and `java.io.DataInputStream`.
use super::bytes::{as_u8_mut, check_range, eof_exception, from_io, throwable};
use super::input_stream::{InputStream, stream_handle};
use crate::java::lang::Throwable;
use std::io::{self, Read};

/// `java.io.DataInput`: big-endian primitives read from a stream. Every `DataInput` the server
/// reads from is a stream (`DataInputStream`, `ObjectInputStream`), so this is a handle over one;
/// `DataInputStream` converts into it with `.into()`.
#[derive(Clone)]
pub struct DataInput {
    pub stream: InputStream,
}

impl std::ops::Deref for DataInput {
    type Target = InputStream;
    fn deref(&self) -> &InputStream {
        &self.stream
    }
}

impl DataInput {
    pub fn new(stream: InputStream) -> Self {
        DataInput { stream }
    }
    fn fill(&self, buf: &mut [u8]) -> Result<(), Throwable> {
        let mut g = self.stream.inner.lock();
        let mut got = 0;
        while got < buf.len() {
            let n = g.read(&mut buf[got..]).map_err(from_io)?;
            if n == 0 {
                return Err(eof_exception());
            }
            got += n;
        }
        Ok(())
    }
    fn arr<const N: usize>(&self) -> Result<[u8; N], Throwable> {
        let mut b = [0u8; N];
        self.fill(&mut b)?;
        Ok(b)
    }
    /// `readFully(byte[])`.
    pub fn read_fully(&self, b: &mut [i8]) -> Result<(), Throwable> {
        self.fill(as_u8_mut(b))
    }
    /// `readFully(byte[], int, int)`.
    pub fn read_fully_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<(), Throwable> {
        let (s, e) = check_range(off, len, b.len());
        self.fill(as_u8_mut(&mut b[s..e]))
    }
    /// `skipBytes(int)`: bytes skipped (fewer at the end of the stream).
    pub fn skip_bytes(&self, n: i32) -> Result<i32, Throwable> {
        if n <= 0 {
            return Ok(0);
        }
        Ok(self.stream.inner.lock().skip(n as u64).map_err(from_io)? as i32)
    }
    pub fn read_boolean(&self) -> Result<bool, Throwable> {
        Ok(self.arr::<1>()?[0] != 0)
    }
    pub fn read_byte(&self) -> Result<i8, Throwable> {
        Ok(self.arr::<1>()?[0] as i8)
    }
    pub fn read_unsigned_byte(&self) -> Result<i32, Throwable> {
        Ok(self.arr::<1>()?[0] as i32)
    }
    pub fn read_short(&self) -> Result<i16, Throwable> {
        Ok(i16::from_be_bytes(self.arr()?))
    }
    pub fn read_unsigned_short(&self) -> Result<i32, Throwable> {
        Ok(u16::from_be_bytes(self.arr()?) as i32)
    }
    /// `readChar()`: one UTF-16 unit (a lone surrogate reads as U+FFFD).
    pub fn read_char(&self) -> Result<char, Throwable> {
        Ok(char::from_u32(u16::from_be_bytes(self.arr()?) as u32).unwrap_or('\u{FFFD}'))
    }
    pub fn read_int(&self) -> Result<i32, Throwable> {
        Ok(i32::from_be_bytes(self.arr()?))
    }
    pub fn read_long(&self) -> Result<i64, Throwable> {
        Ok(i64::from_be_bytes(self.arr()?))
    }
    pub fn read_float(&self) -> Result<f32, Throwable> {
        Ok(f32::from_bits(u32::from_be_bytes(self.arr()?)))
    }
    pub fn read_double(&self) -> Result<f64, Throwable> {
        Ok(f64::from_bits(u64::from_be_bytes(self.arr()?)))
    }
    /// `readLine()` (deprecated in Java; bytes as Latin-1 up to `\n`, `\r` or `\r\n`).
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        let mut s = String::new();
        let mut g = self.stream.inner.lock();
        let mut b = [0u8; 1];
        let mut any = false;
        loop {
            if g.read(&mut b).map_err(from_io)? == 0 {
                return Ok(if any { Some(s) } else { None });
            }
            any = true;
            match b[0] {
                b'\n' => return Ok(Some(s)),
                b'\r' => {
                    // Java peeks for '\n' with a pushback stream; a lone '\r' ends the line too.
                    if g.mark_supported() {
                        g.mark(1);
                        if g.read(&mut b).map_err(from_io)? == 1 && b[0] != b'\n' {
                            g.reset().map_err(from_io)?;
                        }
                    }
                    return Ok(Some(s));
                }
                c => s.push(c as char),
            }
        }
    }
    /// `readUTF()`: Java's modified UTF-8 with a 2-byte length.
    pub fn read_utf(&self) -> Result<String, Throwable> {
        let len = self.read_unsigned_short()? as usize;
        let mut buf = vec![0u8; len];
        self.fill(&mut buf)?;
        decode_modified_utf8(&buf)
    }
}

/// Decodes Java's modified UTF-8 (`DataInput.readUTF`'s body).
pub fn decode_modified_utf8(b: &[u8]) -> Result<String, Throwable> {
    let bad = |i: usize| throwable("java.io.UTFDataFormatException", format!("malformed input around byte {i}"));
    let mut units: Vec<u16> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as u32;
        match c >> 4 {
            0..=7 => {
                units.push(c as u16);
                i += 1;
            }
            12 | 13 => {
                if i + 1 >= b.len() {
                    return Err(throwable("java.io.UTFDataFormatException", "malformed input: partial character at end"));
                }
                let c2 = b[i + 1] as u32;
                if c2 & 0xC0 != 0x80 {
                    return Err(bad(i + 1));
                }
                units.push((((c & 0x1F) << 6) | (c2 & 0x3F)) as u16);
                i += 2;
            }
            14 => {
                if i + 2 >= b.len() {
                    return Err(throwable("java.io.UTFDataFormatException", "malformed input: partial character at end"));
                }
                let c2 = b[i + 1] as u32;
                let c3 = b[i + 2] as u32;
                if c2 & 0xC0 != 0x80 || c3 & 0xC0 != 0x80 {
                    return Err(bad(i + 1));
                }
                units.push((((c & 0x0F) << 12) | ((c2 & 0x3F) << 6) | (c3 & 0x3F)) as u16);
                i += 3;
            }
            _ => return Err(bad(i)),
        }
    }
    Ok(String::from_utf16_lossy(&units))
}

/// Encodes `s` as Java's modified UTF-8 (no length prefix).
pub fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for u in s.encode_utf16() {
        let c = u as u32;
        if (1..=0x7F).contains(&c) {
            out.push(c as u8);
        } else if c <= 0x7FF {
            out.push((0xC0 | ((c >> 6) & 0x1F)) as u8);
            out.push((0x80 | (c & 0x3F)) as u8);
        } else {
            out.push((0xE0 | ((c >> 12) & 0x0F)) as u8);
            out.push((0x80 | ((c >> 6) & 0x3F)) as u8);
            out.push((0x80 | (c & 0x3F)) as u8);
        }
    }
    out
}

/// `java.io.DataInputStream`: derefs to its [`DataInput`] (and on to the `InputStream`).
#[derive(Clone)]
pub struct DataInputStream {
    pub base: InputStream,
    pub data: DataInput,
}
stream_handle!(DataInputStream);
impl DataInputStream {
    pub fn new(in_: InputStream) -> Self {
        let base = InputStream::new(super::input_stream::FilterSource(in_));
        DataInputStream { data: DataInput::new(base.clone()), base }
    }
    /// The stream as a `DataInput` (Java: the same object).
    pub fn as_data_input(&self) -> DataInput {
        self.data.clone()
    }
    pub fn read_fully(&self, b: &mut [i8]) -> Result<(), Throwable> {
        self.data.read_fully(b)
    }
    pub fn read_fully_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<(), Throwable> {
        self.data.read_fully_3(b, off, len)
    }
    pub fn skip_bytes(&self, n: i32) -> Result<i32, Throwable> {
        self.data.skip_bytes(n)
    }
    pub fn read_boolean(&self) -> Result<bool, Throwable> {
        self.data.read_boolean()
    }
    pub fn read_byte(&self) -> Result<i8, Throwable> {
        self.data.read_byte()
    }
    pub fn read_unsigned_byte(&self) -> Result<i32, Throwable> {
        self.data.read_unsigned_byte()
    }
    pub fn read_short(&self) -> Result<i16, Throwable> {
        self.data.read_short()
    }
    pub fn read_unsigned_short(&self) -> Result<i32, Throwable> {
        self.data.read_unsigned_short()
    }
    pub fn read_char(&self) -> Result<char, Throwable> {
        self.data.read_char()
    }
    pub fn read_int(&self) -> Result<i32, Throwable> {
        self.data.read_int()
    }
    pub fn read_long(&self) -> Result<i64, Throwable> {
        self.data.read_long()
    }
    pub fn read_float(&self) -> Result<f32, Throwable> {
        self.data.read_float()
    }
    pub fn read_double(&self) -> Result<f64, Throwable> {
        self.data.read_double()
    }
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        self.data.read_line()
    }
    pub fn read_utf(&self) -> Result<String, Throwable> {
        self.data.read_utf()
    }
    /// `DataInputStream.readUTF(DataInput)`.
    pub fn read_utf_1(input: DataInput) -> Result<String, Throwable> {
        input.read_utf()
    }
}
impl From<DataInputStream> for DataInput {
    fn from(s: DataInputStream) -> DataInput {
        s.data
    }
}
impl From<InputStream> for DataInput {
    fn from(s: InputStream) -> DataInput {
        DataInput::new(s)
    }
}

