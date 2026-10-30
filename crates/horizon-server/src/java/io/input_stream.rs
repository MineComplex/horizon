//! `java.io.InputStream` and the concrete streams the server uses (`ByteArrayInputStream`,
//! `FileInputStream`, `BufferedInputStream`, `FilterInputStream`, `PushbackInputStream`).
//!
//! An `InputStream` is a handle: clones share the same stream (Java's references). Any byte source
//! becomes one through [`InputSource`] (or [`InputStream::from_read`] for a `std::io::Read`); the
//! concrete classes deref to their `InputStream` and convert into it with `.into()`.
use super::bytes::{as_i8, as_u8_mut, check_range, from_io, throwable, to_i8_vec};
use super::output_stream::OutputStream;
use crate::java::lang::Throwable;
use parking_lot::Mutex;
use std::io::{self, Read};
use std::sync::Arc;

/// What an `InputStream` reads from (Java: the abstract methods a subclass overrides).
pub trait InputSource: Send {
    /// Reads up to `buf.len()` bytes; `Ok(0)` (with a non-empty `buf`) is the end of the stream.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// Bytes readable without blocking (Java `available()`).
    fn available(&mut self) -> io::Result<usize> {
        Ok(0)
    }
    /// Skips up to `n` bytes, returning how many were skipped.
    fn skip(&mut self, n: u64) -> io::Result<u64> {
        let mut left = n;
        let mut buf = [0u8; 2048];
        while left > 0 {
            let k = self.read(&mut buf[..left.min(2048) as usize])?;
            if k == 0 {
                break;
            }
            left -= k as u64;
        }
        Ok(n - left)
    }
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn mark_supported(&self) -> bool {
        false
    }
    fn mark(&mut self, _read_limit: usize) {}
    fn reset(&mut self) -> io::Result<()> {
        Err(io::Error::other("mark/reset not supported"))
    }
}

/// Any `std::io::Read` as an [`InputSource`]; closing drops the reader.
pub struct ReadSource<R: Read + Send>(pub Option<R>);
impl<R: Read + Send> InputSource for ReadSource<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.0 {
            Some(r) => loop {
                match r.read(buf) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    x => return x,
                }
            },
            None => Err(io::Error::other("Stream closed")),
        }
    }
    fn close(&mut self) -> io::Result<()> {
        self.0 = None;
        Ok(())
    }
}

/// `java.io.InputStream`.
#[derive(Clone)]
pub struct InputStream {
    pub inner: Arc<Mutex<Box<dyn InputSource>>>,
}

impl InputStream {
    /// A stream over `source` (Java: an `InputStream` subclass).
    pub fn new(source: impl InputSource + 'static) -> Self {
        InputStream { inner: Arc::new(Mutex::new(Box::new(source))) }
    }
    /// A stream over a Rust reader.
    pub fn from_read(r: impl Read + Send + 'static) -> Self {
        Self::new(ReadSource(Some(r)))
    }
    /// `InputStream.nullInputStream()`.
    pub fn null_input_stream() -> Self {
        Self::from_read(io::empty())
    }
    /// `read()`: the next byte (0..=255) or -1 at the end.
    pub fn read(&self) -> Result<i32, Throwable> {
        let mut b = [0u8; 1];
        let n = self.inner.lock().read(&mut b).map_err(from_io)?;
        Ok(if n == 0 { -1 } else { b[0] as i32 })
    }
    /// `read(byte[])`.
    pub fn read_1(&self, b: &mut Vec<i8>) -> Result<i32, Throwable> {
        let len = b.len() as i32;
        self.read_3(b, 0, len)
    }
    /// `read(byte[], int, int)`: bytes read, or -1 at the end.
    pub fn read_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<i32, Throwable> {
        let (s, e) = check_range(off, len, b.len());
        if len == 0 {
            return Ok(0);
        }
        let n = self.inner.lock().read(as_u8_mut(&mut b[s..e])).map_err(from_io)?;
        Ok(if n == 0 { -1 } else { n as i32 })
    }
    /// `readAllBytes()`.
    pub fn read_all_bytes(&self) -> Result<Vec<i8>, Throwable> {
        let mut out = Vec::new();
        let mut g = self.inner.lock();
        let mut buf = vec![0u8; 8192];
        loop {
            let n = g.read(&mut buf).map_err(from_io)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(to_i8_vec(out))
    }
    /// `readNBytes(int)`.
    pub fn read_n_bytes(&self, len: i32) -> Result<Vec<i8>, Throwable> {
        if len < 0 {
            panic!("java.lang.IllegalArgumentException: len < 0");
        }
        let mut out = vec![0u8; len as usize];
        let mut got = 0;
        let mut g = self.inner.lock();
        while got < out.len() {
            let n = g.read(&mut out[got..]).map_err(from_io)?;
            if n == 0 {
                break;
            }
            got += n;
        }
        out.truncate(got);
        Ok(to_i8_vec(out))
    }
    /// `readNBytes(byte[], int, int)`: reads until `len` bytes or the end.
    pub fn read_n_bytes_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<i32, Throwable> {
        let (s, e) = check_range(off, len, b.len());
        let buf = as_u8_mut(&mut b[s..e]);
        let mut got = 0;
        let mut g = self.inner.lock();
        while got < buf.len() {
            let n = g.read(&mut buf[got..]).map_err(from_io)?;
            if n == 0 {
                break;
            }
            got += n;
        }
        Ok(got as i32)
    }
    /// `skip(long)`.
    pub fn skip(&self, n: i64) -> Result<i64, Throwable> {
        if n <= 0 {
            return Ok(0);
        }
        Ok(self.inner.lock().skip(n as u64).map_err(from_io)? as i64)
    }
    /// `skipNBytes(long)`: throws `EOFException` when the stream ends first.
    pub fn skip_n_bytes(&self, n: i64) -> Result<(), Throwable> {
        if n <= 0 {
            return Ok(());
        }
        let k = self.inner.lock().skip(n as u64).map_err(from_io)?;
        if (k as i64) < n {
            return Err(throwable("java.io.EOFException", ""));
        }
        Ok(())
    }
    pub fn available(&self) -> Result<i32, Throwable> {
        Ok(self.inner.lock().available().map_err(from_io)?.min(i32::MAX as usize) as i32)
    }
    pub fn close(&self) -> Result<(), Throwable> {
        self.inner.lock().close().map_err(from_io)
    }
    pub fn mark(&self, read_limit: i32) {
        self.inner.lock().mark(read_limit.max(0) as usize)
    }
    pub fn reset(&self) -> Result<(), Throwable> {
        self.inner.lock().reset().map_err(from_io)
    }
    pub fn mark_supported(&self) -> bool {
        self.inner.lock().mark_supported()
    }
    /// `transferTo(OutputStream)`: copies the rest of this stream, returning the byte count.
    pub fn transfer_to(&self, out: OutputStream) -> Result<i64, Throwable> {
        let mut total = 0i64;
        let mut buf = vec![0u8; 8192];
        loop {
            let n = self.inner.lock().read(&mut buf).map_err(from_io)?;
            if n == 0 {
                return Ok(total);
            }
            out.write_3(as_i8(&buf[..n]), 0, n as i32)?;
            total += n as i64;
        }
    }
    /// Identity (Java `==`).
    pub fn same(&self, other: &InputStream) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Read for InputStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.lock().read(buf)
    }
}
impl Read for &InputStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.lock().read(buf)
    }
}

/// Passes everything to another `InputStream` (Java `FilterInputStream`'s behaviour).
pub struct FilterSource(pub InputStream);
impl InputSource for FilterSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.inner.lock().read(buf)
    }
    fn available(&mut self) -> io::Result<usize> {
        self.0.inner.lock().available()
    }
    fn skip(&mut self, n: u64) -> io::Result<u64> {
        self.0.inner.lock().skip(n)
    }
    fn close(&mut self) -> io::Result<()> {
        self.0.inner.lock().close()
    }
    fn mark_supported(&self) -> bool {
        self.0.inner.lock().mark_supported()
    }
    fn mark(&mut self, l: usize) {
        self.0.inner.lock().mark(l)
    }
    fn reset(&mut self) -> io::Result<()> {
        self.0.inner.lock().reset()
    }
}

macro_rules! stream_handle {
    ($t:ident) => {
        impl std::ops::Deref for $t {
            type Target = InputStream;
            fn deref(&self) -> &InputStream {
                &self.base
            }
        }
        impl From<$t> for InputStream {
            fn from(s: $t) -> InputStream {
                s.base
            }
        }
        impl Read for $t {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.base.inner.lock().read(buf)
            }
        }
    };
}
pub(crate) use stream_handle;

// ---- ByteArrayInputStream
struct ByteArraySource {
    data: Vec<u8>,
    pos: usize,
    mark: usize,
}
impl InputSource for ByteArraySource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn available(&mut self) -> io::Result<usize> {
        Ok(self.data.len() - self.pos)
    }
    fn skip(&mut self, n: u64) -> io::Result<u64> {
        let k = (n as usize).min(self.data.len() - self.pos);
        self.pos += k;
        Ok(k as u64)
    }
    fn mark_supported(&self) -> bool {
        true
    }
    fn mark(&mut self, _l: usize) {
        self.mark = self.pos;
    }
    fn reset(&mut self) -> io::Result<()> {
        self.pos = self.mark;
        Ok(())
    }
}

/// `java.io.ByteArrayInputStream` (reads a copy of the array).
#[derive(Clone)]
pub struct ByteArrayInputStream {
    pub base: InputStream,
}
stream_handle!(ByteArrayInputStream);
impl ByteArrayInputStream {
    pub fn new(buf: &[i8]) -> Self {
        Self::from_bytes(super::bytes::as_u8(buf).to_vec())
    }
    pub fn new_3(buf: &[i8], offset: i32, length: i32) -> Self {
        let off = offset.max(0) as usize;
        let end = (offset as i64 + length as i64).min(buf.len() as i64).max(off as i64) as usize;
        Self::from_bytes(super::bytes::as_u8(&buf[off.min(buf.len())..end]).to_vec())
    }
    /// From Rust bytes (taken, not copied).
    pub fn from_bytes(data: Vec<u8>) -> Self {
        ByteArrayInputStream { base: InputStream::new(ByteArraySource { data, pos: 0, mark: 0 }) }
    }
}

// ---- FileInputStream
/// `java.io.FileInputStream`.
#[derive(Clone)]
pub struct FileInputStream {
    pub base: InputStream,
}
stream_handle!(FileInputStream);
struct FileSource(Option<std::fs::File>);
impl InputSource for FileSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.0 {
            Some(f) => f.read(buf),
            None => Err(io::Error::other("Stream Closed")),
        }
    }
    fn available(&mut self) -> io::Result<usize> {
        use std::io::Seek;
        match &mut self.0 {
            Some(f) => {
                let len = f.metadata()?.len();
                let pos = f.stream_position()?;
                Ok(len.saturating_sub(pos) as usize)
            }
            None => Err(io::Error::other("Stream Closed")),
        }
    }
    fn skip(&mut self, n: u64) -> io::Result<u64> {
        use std::io::{Seek, SeekFrom};
        match &mut self.0 {
            Some(f) => {
                let pos = f.stream_position()?;
                let end = f.seek(SeekFrom::Current(n as i64))?;
                Ok(end - pos)
            }
            None => Err(io::Error::other("Stream Closed")),
        }
    }
    fn close(&mut self) -> io::Result<()> {
        self.0 = None;
        Ok(())
    }
}
impl FileInputStream {
    /// `new FileInputStream(File)` / `new FileInputStream(String)`.
    pub fn new(file: impl AsRef<std::path::Path>) -> Result<Self, Throwable> {
        let p = file.as_ref();
        let f = std::fs::File::open(p).map_err(|e| {
            throwable("java.io.FileNotFoundException", format!("{} ({})", p.display(), e))
        })?;
        Ok(Self::from_file(f))
    }
    pub fn from_file(f: std::fs::File) -> Self {
        FileInputStream { base: InputStream::new(FileSource(Some(f))) }
    }
}

// ---- BufferedInputStream
struct BufferedSource {
    inner: InputStream,
    buf: Vec<u8>,
    pos: usize,
    count: usize,
    mark: Option<usize>,
    mark_limit: usize,
    size: usize,
}
impl BufferedSource {
    fn fill(&mut self) -> io::Result<usize> {
        if let Some(m) = self.mark {
            if self.pos - m > self.mark_limit {
                self.mark = None;
            }
        }
        match self.mark {
            None => {
                self.buf.clear();
                self.pos = 0;
                self.count = 0;
            }
            Some(m) => {
                self.buf.drain(..m);
                self.pos -= m;
                self.count -= m;
                self.mark = Some(0);
            }
        }
        let start = self.count;
        self.buf.resize(start + self.size, 0);
        let n = self.inner.inner.lock().read(&mut self.buf[start..])?;
        self.count = start + n;
        self.buf.truncate(self.count);
        Ok(n)
    }
}
impl InputSource for BufferedSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.pos >= self.count {
            if self.mark.is_none() && out.len() >= self.size {
                return self.inner.inner.lock().read(out);
            }
            if self.fill()? == 0 {
                return Ok(0);
            }
        }
        let n = out.len().min(self.count - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn available(&mut self) -> io::Result<usize> {
        Ok(self.count - self.pos + self.inner.inner.lock().available()?)
    }
    fn close(&mut self) -> io::Result<()> {
        self.buf = Vec::new();
        self.inner.inner.lock().close()
    }
    fn mark_supported(&self) -> bool {
        true
    }
    fn mark(&mut self, l: usize) {
        self.mark = Some(self.pos);
        self.mark_limit = l;
    }
    fn reset(&mut self) -> io::Result<()> {
        match self.mark {
            Some(m) => {
                self.pos = m;
                Ok(())
            }
            None => Err(io::Error::other("Resetting to invalid mark")),
        }
    }
}

/// `java.io.BufferedInputStream`.
#[derive(Clone)]
pub struct BufferedInputStream {
    pub base: InputStream,
}
stream_handle!(BufferedInputStream);
impl BufferedInputStream {
    pub fn new(in_: InputStream) -> Self {
        Self::new_2(in_, 8192)
    }
    pub fn new_2(in_: InputStream, size: i32) -> Self {
        if size <= 0 {
            panic!("java.lang.IllegalArgumentException: Buffer size <= 0");
        }
        BufferedInputStream {
            base: InputStream::new(BufferedSource {
                inner: in_,
                buf: Vec::new(),
                pos: 0,
                count: 0,
                mark: None,
                mark_limit: 0,
                size: size as usize,
            }),
        }
    }
}

/// `java.io.FilterInputStream`: passes everything to `in`.
#[derive(Clone)]
pub struct FilterInputStream {
    pub base: InputStream,
    /// The wrapped stream (Java's protected `in` field).
    pub in_: InputStream,
}
stream_handle!(FilterInputStream);
impl FilterInputStream {
    pub fn new(in_: InputStream) -> Self {
        FilterInputStream { base: InputStream::new(FilterSource(in_.clone())), in_ }
    }
}

// ---- PushbackInputStream
struct PushbackSource {
    inner: InputStream,
    back: Arc<Mutex<Vec<u8>>>,
}
impl InputSource for PushbackSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut back = self.back.lock();
        if !back.is_empty() {
            let mut n = 0;
            while n < out.len() {
                match back.pop() {
                    Some(b) => {
                        out[n] = b;
                        n += 1;
                    }
                    None => break,
                }
            }
            drop(back);
            if n < out.len() {
                // Java reads on from the stream when the pushback buffer runs out.
                n += self.inner.inner.lock().read(&mut out[n..])?;
            }
            return Ok(n);
        }
        drop(back);
        self.inner.inner.lock().read(out)
    }
    fn available(&mut self) -> io::Result<usize> {
        Ok(self.back.lock().len() + self.inner.inner.lock().available()?)
    }
    fn skip(&mut self, n: u64) -> io::Result<u64> {
        let mut back = self.back.lock();
        let k = (n as usize).min(back.len());
        let l = back.len();
        back.truncate(l - k);
        drop(back);
        let rest = if (k as u64) < n { self.inner.inner.lock().skip(n - k as u64)? } else { 0 };
        Ok(k as u64 + rest)
    }
    fn close(&mut self) -> io::Result<()> {
        self.back.lock().clear();
        self.inner.inner.lock().close()
    }
}

/// `java.io.PushbackInputStream`.
#[derive(Clone)]
pub struct PushbackInputStream {
    pub base: InputStream,
    /// Pushed-back bytes, last to be read first (top of the stack = end of the Vec).
    back: Arc<Mutex<Vec<u8>>>,
    size: usize,
}
stream_handle!(PushbackInputStream);
impl PushbackInputStream {
    pub fn new(in_: InputStream) -> Self {
        Self::new_2(in_, 1)
    }
    pub fn new_2(in_: InputStream, size: i32) -> Self {
        if size <= 0 {
            panic!("java.lang.IllegalArgumentException: size <= 0");
        }
        let back = Arc::new(Mutex::new(Vec::new()));
        PushbackInputStream {
            base: InputStream::new(PushbackSource { inner: in_, back: back.clone() }),
            back,
            size: size as usize,
        }
    }
    /// `unread(int)`.
    pub fn unread(&self, b: i32) -> Result<(), Throwable> {
        let mut back = self.back.lock();
        if back.len() >= self.size {
            return Err(throwable("java.io.IOException", "Push back buffer is full"));
        }
        back.push(b as u8);
        Ok(())
    }
    /// `unread(byte[])`.
    pub fn unread_1(&self, b: &[i8]) -> Result<(), Throwable> {
        self.unread_3(b, 0, b.len() as i32)
    }
    /// `unread(byte[], int, int)`: the bytes are read back in their order.
    pub fn unread_3(&self, b: &[i8], off: i32, len: i32) -> Result<(), Throwable> {
        let (s, e) = check_range(off, len, b.len());
        let mut back = self.back.lock();
        if back.len() + (e - s) > self.size {
            return Err(throwable("java.io.IOException", "Push back buffer is full"));
        }
        for &x in b[s..e].iter().rev() {
            back.push(x as u8);
        }
        Ok(())
    }
}
