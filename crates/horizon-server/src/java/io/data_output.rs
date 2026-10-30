//! `java.io.DataOutput` (an interface in-tree classes implement: a trait) and `DataOutputStream`.
use super::bytes::{as_u8, check_range, from_io, throwable};
use super::data_input::encode_modified_utf8;
use super::output_stream::{OutputSink, OutputStream, out_stream_handle};
use crate::java::lang::Throwable;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

/// `java.io.DataOutput`: big-endian primitives. Implementors give `write(int)` (and `write_3` when
/// they can do better); the rest are Java's semantics as default bodies.
pub trait DataOutput: Send + Sync {
    /// `write(int)`: the low 8 bits.
    fn write(&self, b: i32) -> Result<(), Throwable>;
    /// `write(byte[])`.
    fn write_1(&self, b: &Vec<i8>) -> Result<(), Throwable> {
        self.write_3(b, 0, b.len() as i32)
    }
    /// `write(byte[], int, int)`.
    fn write_3(&self, b: &Vec<i8>, off: i32, len: i32) -> Result<(), Throwable> {
        let (s, e) = check_range(off, len, b.len());
        for &x in &b[s..e] {
            self.write(x as i32)?;
        }
        Ok(())
    }
    fn write_boolean(&self, v: bool) -> Result<(), Throwable> {
        self.write(v as i32)
    }
    fn write_byte(&self, v: i32) -> Result<(), Throwable> {
        self.write(v)
    }
    fn write_short(&self, v: i32) -> Result<(), Throwable> {
        self.write_3(&be(&(v as i16).to_be_bytes()), 0, 2)
    }
    fn write_char(&self, v: i32) -> Result<(), Throwable> {
        self.write_3(&be(&(v as u16).to_be_bytes()), 0, 2)
    }
    fn write_int(&self, v: i32) -> Result<(), Throwable> {
        self.write_3(&be(&v.to_be_bytes()), 0, 4)
    }
    fn write_long(&self, v: i64) -> Result<(), Throwable> {
        self.write_3(&be(&v.to_be_bytes()), 0, 8)
    }
    fn write_float(&self, v: f32) -> Result<(), Throwable> {
        self.write_int(v.to_bits() as i32)
    }
    fn write_double(&self, v: f64) -> Result<(), Throwable> {
        self.write_long(v.to_bits() as i64)
    }
    /// `writeBytes(String)`: the low byte of each UTF-16 unit.
    fn write_bytes(&self, s: &str) -> Result<(), Throwable> {
        let b: Vec<i8> = s.encode_utf16().map(|u| u as u8 as i8).collect();
        self.write_3(&b, 0, b.len() as i32)
    }
    /// `writeChars(String)`: each UTF-16 unit, big-endian.
    fn write_chars(&self, s: &str) -> Result<(), Throwable> {
        let b: Vec<i8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).map(|x| x as i8).collect();
        self.write_3(&b, 0, b.len() as i32)
    }
    /// `writeUTF(String)`: modified UTF-8 with a 2-byte length (`UTFDataFormatException` past 65535).
    fn write_utf(&self, s: &str) -> Result<(), Throwable> {
        let b = utf_bytes(s)?;
        self.write_3(&b, 0, b.len() as i32)
    }
}

fn be(b: &[u8]) -> Vec<i8> {
    b.iter().map(|&x| x as i8).collect()
}

/// `writeUTF`'s bytes: the 2-byte length then the modified UTF-8.
pub fn utf_bytes(s: &str) -> Result<Vec<i8>, Throwable> {
    let enc = encode_modified_utf8(s);
    if enc.len() > 65535 {
        let shown: String = s.chars().take(8).collect();
        return Err(throwable(
            "java.io.UTFDataFormatException",
            format!("encoded string ({shown}...) too long: {} bytes", enc.len()),
        ));
    }
    let mut out = Vec::with_capacity(enc.len() + 2);
    out.extend_from_slice(&(enc.len() as u16).to_be_bytes());
    out.extend_from_slice(&enc);
    Ok(super::bytes::to_i8_vec(out))
}

struct CountingSink {
    out: OutputStream,
    written: Arc<AtomicI32>,
}
impl OutputSink for CountingSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        self.out.inner.lock().write(buf)?;
        let n = buf.len() as i64;
        // Java saturates `written` at Integer.MAX_VALUE.
        let _ = self.written.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |w| {
            Some((w as i64 + n).min(i32::MAX as i64) as i32)
        });
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.inner.lock().flush()
    }
    fn close(&mut self) -> io::Result<()> {
        let mut g = self.out.inner.lock();
        let r = g.flush();
        g.close()?;
        r
    }
}

/// `java.io.DataOutputStream`. Its `DataOutput` methods are inherent too (no trait import needed);
/// `as_data_output()` gives the `JRef<dyn DataOutput>` the NBT code takes.
#[derive(Clone)]
pub struct DataOutputStream {
    pub base: OutputStream,
    written: Arc<AtomicI32>,
}
out_stream_handle!(DataOutputStream);

impl DataOutputStream {
    pub fn new(out: OutputStream) -> Self {
        let written = Arc::new(AtomicI32::new(0));
        DataOutputStream { base: OutputStream::new(CountingSink { out, written: written.clone() }), written }
    }
    /// `size()`: bytes written so far.
    pub fn size(&self) -> i32 {
        self.written.load(Ordering::Relaxed)
    }
    pub fn as_data_output(&self) -> crate::java::JRef<dyn DataOutput> {
        Arc::new(self.clone())
    }
    fn put(&self, b: &[u8]) -> Result<(), Throwable> {
        self.base.inner.lock().write(b).map_err(from_io)
    }
    pub fn write(&self, b: i32) -> Result<(), Throwable> {
        self.put(&[b as u8])
    }
    pub fn write_1(&self, b: &[i8]) -> Result<(), Throwable> {
        self.put(as_u8(b))
    }
    pub fn write_3(&self, b: &[i8], off: i32, len: i32) -> Result<(), Throwable> {
        let (s, e) = check_range(off, len, b.len());
        self.put(as_u8(&b[s..e]))
    }
    pub fn write_boolean(&self, v: bool) -> Result<(), Throwable> {
        self.put(&[v as u8])
    }
    pub fn write_byte(&self, v: i32) -> Result<(), Throwable> {
        self.put(&[v as u8])
    }
    pub fn write_short(&self, v: i32) -> Result<(), Throwable> {
        self.put(&(v as u16).to_be_bytes())
    }
    pub fn write_char(&self, v: i32) -> Result<(), Throwable> {
        self.put(&(v as u16).to_be_bytes())
    }
    pub fn write_int(&self, v: i32) -> Result<(), Throwable> {
        self.put(&v.to_be_bytes())
    }
    pub fn write_long(&self, v: i64) -> Result<(), Throwable> {
        self.put(&v.to_be_bytes())
    }
    pub fn write_float(&self, v: f32) -> Result<(), Throwable> {
        self.put(&v.to_bits().to_be_bytes())
    }
    pub fn write_double(&self, v: f64) -> Result<(), Throwable> {
        self.put(&v.to_bits().to_be_bytes())
    }
    pub fn write_bytes(&self, s: &str) -> Result<(), Throwable> {
        let b: Vec<u8> = s.encode_utf16().map(|u| u as u8).collect();
        self.put(&b)
    }
    pub fn write_chars(&self, s: &str) -> Result<(), Throwable> {
        let b: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        self.put(&b)
    }
    pub fn write_utf(&self, s: &str) -> Result<(), Throwable> {
        let b = utf_bytes(s)?;
        self.put(as_u8(&b))
    }
}

impl DataOutput for DataOutputStream {
    fn write(&self, b: i32) -> Result<(), Throwable> {
        DataOutputStream::write(self, b)
    }
    fn write_1(&self, b: &Vec<i8>) -> Result<(), Throwable> {
        DataOutputStream::write_1(self, b)
    }
    fn write_3(&self, b: &Vec<i8>, off: i32, len: i32) -> Result<(), Throwable> {
        DataOutputStream::write_3(self, b, off, len)
    }
    fn write_boolean(&self, v: bool) -> Result<(), Throwable> {
        DataOutputStream::write_boolean(self, v)
    }
    fn write_byte(&self, v: i32) -> Result<(), Throwable> {
        DataOutputStream::write_byte(self, v)
    }
    fn write_short(&self, v: i32) -> Result<(), Throwable> {
        DataOutputStream::write_short(self, v)
    }
    fn write_char(&self, v: i32) -> Result<(), Throwable> {
        DataOutputStream::write_char(self, v)
    }
    fn write_int(&self, v: i32) -> Result<(), Throwable> {
        DataOutputStream::write_int(self, v)
    }
    fn write_long(&self, v: i64) -> Result<(), Throwable> {
        DataOutputStream::write_long(self, v)
    }
    fn write_float(&self, v: f32) -> Result<(), Throwable> {
        DataOutputStream::write_float(self, v)
    }
    fn write_double(&self, v: f64) -> Result<(), Throwable> {
        DataOutputStream::write_double(self, v)
    }
    fn write_bytes(&self, s: &str) -> Result<(), Throwable> {
        DataOutputStream::write_bytes(self, s)
    }
    fn write_chars(&self, s: &str) -> Result<(), Throwable> {
        DataOutputStream::write_chars(self, s)
    }
    fn write_utf(&self, s: &str) -> Result<(), Throwable> {
        DataOutputStream::write_utf(self, s)
    }
}
