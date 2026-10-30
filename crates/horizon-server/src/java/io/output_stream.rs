//! `java.io.OutputStream` and the concrete streams the server uses (`ByteArrayOutputStream`,
//! `FileOutputStream`, `BufferedOutputStream`, `FilterOutputStream`).
//!
//! An `OutputStream` is a handle: clones share the same stream. Any byte sink becomes one through
//! [`OutputSink`] (or [`OutputStream::from_write`] for a `std::io::Write`); the concrete classes deref
//! to their `OutputStream` and convert into it with `.into()`.
use super::bytes::{as_u8, check_range, from_io, to_i8_vec};
use crate::java::lang::Throwable;
use parking_lot::Mutex;
use std::io::{self, Write};
use std::sync::Arc;

/// What an `OutputStream` writes to (Java: the methods a subclass overrides).
pub trait OutputSink: Send {
    fn write(&mut self, buf: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Flushes and releases the sink (and closes what it wraps).
    fn close(&mut self) -> io::Result<()> {
        self.flush()
    }
}

/// Any `std::io::Write` as an [`OutputSink`]; closing flushes and drops the writer.
pub struct WriteSink<W: Write + Send>(pub Option<W>);
impl<W: Write + Send> OutputSink for WriteSink<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        match &mut self.0 {
            Some(w) => w.write_all(buf),
            None => Err(io::Error::other("Stream Closed")),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.0 {
            Some(w) => w.flush(),
            None => Ok(()),
        }
    }
    fn close(&mut self) -> io::Result<()> {
        if let Some(mut w) = self.0.take() {
            w.flush()?;
        }
        Ok(())
    }
}

/// `java.io.OutputStream`.
#[derive(Clone)]
pub struct OutputStream {
    pub inner: Arc<Mutex<Box<dyn OutputSink>>>,
}

impl OutputStream {
    /// A stream over `sink` (Java: an `OutputStream` subclass).
    pub fn new(sink: impl OutputSink + 'static) -> Self {
        OutputStream { inner: Arc::new(Mutex::new(Box::new(sink))) }
    }
    /// A stream over a Rust writer.
    pub fn from_write(w: impl Write + Send + 'static) -> Self {
        Self::new(WriteSink(Some(w)))
    }
    /// `OutputStream.nullOutputStream()`.
    pub fn null_output_stream() -> Self {
        Self::from_write(io::sink())
    }
    /// `write(int)`: the low 8 bits of `b`.
    pub fn write(&self, b: i32) -> Result<(), Throwable> {
        self.inner.lock().write(&[b as u8]).map_err(from_io)
    }
    /// `write(byte[])`.
    pub fn write_1(&self, b: &[i8]) -> Result<(), Throwable> {
        self.inner.lock().write(as_u8(b)).map_err(from_io)
    }
    /// `write(byte[], int, int)`.
    pub fn write_3(&self, b: &[i8], off: i32, len: i32) -> Result<(), Throwable> {
        let (s, e) = check_range(off, len, b.len());
        self.inner.lock().write(as_u8(&b[s..e])).map_err(from_io)
    }
    /// Writes Rust bytes.
    pub fn write_u8(&self, b: &[u8]) -> Result<(), Throwable> {
        self.inner.lock().write(b).map_err(from_io)
    }
    pub fn flush(&self) -> Result<(), Throwable> {
        self.inner.lock().flush().map_err(from_io)
    }
    pub fn close(&self) -> Result<(), Throwable> {
        self.inner.lock().close().map_err(from_io)
    }
    /// Identity (Java `==`).
    pub fn same(&self, other: &OutputStream) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Write for OutputStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.lock().write(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().flush()
    }
}
impl Write for &OutputStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.lock().write(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().flush()
    }
}

/// Passes everything to another `OutputStream` (Java `FilterOutputStream`'s behaviour).
pub struct FilterSink(pub OutputStream);
impl OutputSink for FilterSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.inner.lock().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.inner.lock().flush()
    }
    fn close(&mut self) -> io::Result<()> {
        let mut g = self.0.inner.lock();
        let r = g.flush();
        g.close()?;
        r
    }
}

macro_rules! out_stream_handle {
    ($t:ident) => {
        impl std::ops::Deref for $t {
            type Target = OutputStream;
            fn deref(&self) -> &OutputStream {
                &self.base
            }
        }
        impl From<$t> for OutputStream {
            fn from(s: $t) -> OutputStream {
                s.base
            }
        }
        impl std::io::Write for $t {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.base.inner.lock().write(buf)?;
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.base.inner.lock().flush()
            }
        }
    };
}
pub(crate) use out_stream_handle;

// ---- ByteArrayOutputStream
struct SharedVecSink(Arc<Mutex<Vec<u8>>>);
impl OutputSink for SharedVecSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.lock().extend_from_slice(buf);
        Ok(())
    }
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `java.io.ByteArrayOutputStream`.
#[derive(Clone)]
pub struct ByteArrayOutputStream {
    pub base: OutputStream,
    /// The bytes written so far (Java's protected `buf`/`count`).
    pub buf: Arc<Mutex<Vec<u8>>>,
}
out_stream_handle!(ByteArrayOutputStream);
impl ByteArrayOutputStream {
    pub fn new() -> Self {
        Self::new_1(32)
    }
    pub fn new_1(size: i32) -> Self {
        if size < 0 {
            panic!("java.lang.IllegalArgumentException: Negative initial size: {size}");
        }
        let buf = Arc::new(Mutex::new(Vec::with_capacity(size as usize)));
        ByteArrayOutputStream { base: OutputStream::new(SharedVecSink(buf.clone())), buf }
    }
    /// `toByteArray()`: a copy of the bytes written.
    pub fn to_byte_array(&self) -> Vec<i8> {
        to_i8_vec(self.buf.lock().clone())
    }
    /// The bytes written, as Rust bytes (a copy).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.buf.lock().clone()
    }
    pub fn size(&self) -> i32 {
        self.buf.lock().len() as i32
    }
    pub fn reset(&self) {
        self.buf.lock().clear()
    }
    /// `toString()`: the bytes decoded as UTF-8 (the platform charset).
    pub fn to_string(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock()).into_owned()
    }
    /// `toString(Charset)` / `toString(String)`.
    pub fn to_string_1(&self, charset: impl Into<crate::java::nio::charset::Charset>) -> String {
        charset.into().decode_bytes(&self.buf.lock())
    }
    /// `writeTo(OutputStream)`.
    pub fn write_to(&self, out: OutputStream) -> Result<(), Throwable> {
        let data = self.buf.lock().clone();
        out.write_u8(&data)
    }
    /// `writeBytes(byte[])`.
    pub fn write_bytes(&self, b: &[i8]) {
        self.buf.lock().extend_from_slice(as_u8(b));
    }
}
impl Default for ByteArrayOutputStream {
    fn default() -> Self {
        Self::new()
    }
}

// ---- FileOutputStream
/// `java.io.FileOutputStream`.
#[derive(Clone)]
pub struct FileOutputStream {
    pub base: OutputStream,
}
out_stream_handle!(FileOutputStream);
impl FileOutputStream {
    /// `new FileOutputStream(File)` / `(String)`: truncates the file.
    pub fn new(file: impl AsRef<std::path::Path>) -> Result<Self, Throwable> {
        Self::new_2(file, false)
    }
    /// `new FileOutputStream(File, boolean append)`.
    pub fn new_2(file: impl AsRef<std::path::Path>, append: bool) -> Result<Self, Throwable> {
        let p = file.as_ref();
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true);
        if append {
            o.append(true);
        } else {
            o.truncate(true);
        }
        let f = o.open(p).map_err(|e| {
            super::bytes::throwable("java.io.FileNotFoundException", format!("{} ({})", p.display(), e))
        })?;
        Ok(Self::from_file(f))
    }
    pub fn from_file(f: std::fs::File) -> Self {
        FileOutputStream { base: OutputStream::from_write(f) }
    }
}

// ---- BufferedOutputStream
struct BufferedSink {
    inner: OutputStream,
    buf: Vec<u8>,
    size: usize,
}
impl BufferedSink {
    fn flush_buffer(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.inner.inner.lock().write(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }
}
impl OutputSink for BufferedSink {
    fn write(&mut self, b: &[u8]) -> io::Result<()> {
        if b.len() >= self.size {
            self.flush_buffer()?;
            return self.inner.inner.lock().write(b);
        }
        if b.len() > self.size - self.buf.len() {
            self.flush_buffer()?;
        }
        self.buf.extend_from_slice(b);
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flush_buffer()?;
        self.inner.inner.lock().flush()
    }
    fn close(&mut self) -> io::Result<()> {
        let r = self.flush();
        self.inner.inner.lock().close()?;
        r
    }
}

/// `java.io.BufferedOutputStream`.
#[derive(Clone)]
pub struct BufferedOutputStream {
    pub base: OutputStream,
}
out_stream_handle!(BufferedOutputStream);
impl BufferedOutputStream {
    pub fn new(out: OutputStream) -> Self {
        Self::new_2(out, 8192)
    }
    pub fn new_2(out: OutputStream, size: i32) -> Self {
        if size <= 0 {
            panic!("java.lang.IllegalArgumentException: Buffer size <= 0");
        }
        BufferedOutputStream {
            base: OutputStream::new(BufferedSink { inner: out, buf: Vec::new(), size: size as usize }),
        }
    }
}

/// `java.io.FilterOutputStream`.
#[derive(Clone)]
pub struct FilterOutputStream {
    pub base: OutputStream,
    /// The wrapped stream (Java's protected `out` field).
    pub out: OutputStream,
}
out_stream_handle!(FilterOutputStream);
impl FilterOutputStream {
    pub fn new(out: OutputStream) -> Self {
        FilterOutputStream { base: OutputStream::new(FilterSink(out.clone())), out }
    }
}
