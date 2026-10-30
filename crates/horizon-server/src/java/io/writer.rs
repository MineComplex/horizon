//! `java.io.Writer` and its classes (`OutputStreamWriter`, `BufferedWriter`, `StringWriter`,
//! `PrintWriter`, `FileWriter`).
use super::bytes::{from_io, throwable};
use super::output_stream::{FileOutputStream, OutputStream};
use crate::java::lang::Throwable;
use crate::java::nio::charset::{Charset, CharsetEncoder, standard_charsets};
use parking_lot::Mutex;
use std::sync::Arc;

/// What a `Writer` writes characters to (Java: a `Writer` subclass).
pub trait CharSink: Send {
    fn write_str(&mut self, s: &str) -> Result<(), Throwable>;
    fn flush(&mut self) -> Result<(), Throwable> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), Throwable> {
        self.flush()
    }
}

/// `java.io.Writer` (a handle: clones share the writer).
#[derive(Clone)]
pub struct Writer {
    pub inner: Arc<Mutex<Box<dyn CharSink>>>,
}

/// What `Writer.write(x)`/`append(x)` take: a `String`/`CharSequence`, a `char`, a `char[]`.
pub trait WriterArg {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R;
}
impl WriterArg for &str {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self)
    }
}
impl WriterArg for String {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(&self)
    }
}
impl WriterArg for &String {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self)
    }
}
impl WriterArg for char {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self.encode_utf8(&mut [0u8; 4]))
    }
}
impl WriterArg for &[char] {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(&self.iter().collect::<String>())
    }
}
impl WriterArg for &Vec<char> {
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(&self.iter().collect::<String>())
    }
}
impl WriterArg for Option<String> {
    /// A `null` string writes `"null"`, as Java's `append(null)`.
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self.as_deref().unwrap_or("null"))
    }
}

impl Writer {
    pub fn new(sink: impl CharSink + 'static) -> Self {
        Writer { inner: Arc::new(Mutex::new(Box::new(sink))) }
    }
    /// `Writer.nullWriter()`.
    pub fn null_writer() -> Self {
        struct Null;
        impl CharSink for Null {
            fn write_str(&mut self, _s: &str) -> Result<(), Throwable> {
                Ok(())
            }
        }
        Writer::new(Null)
    }
    /// Writes a Rust string.
    pub fn write_str(&self, s: &str) -> Result<(), Throwable> {
        self.inner.lock().write_str(s)
    }
    /// `write(int)`: one character.
    pub fn write(&self, c: i32) -> Result<(), Throwable> {
        let ch = char::from_u32(c as u32 & 0xFFFF).unwrap_or('\u{FFFD}');
        self.write_str(ch.encode_utf8(&mut [0u8; 4]))
    }
    /// `write(String)` / `write(char[])`.
    pub fn write_1(&self, a: impl WriterArg) -> Result<(), Throwable> {
        a.with_str(|s| self.write_str(s))
    }
    /// `write(String, int off, int len)` / `write(char[], int off, int len)` (offsets in characters).
    pub fn write_3(&self, a: impl WriterArg, off: i32, len: i32) -> Result<(), Throwable> {
        a.with_str(|s| {
            let part: String = s.chars().skip(off.max(0) as usize).take(len.max(0) as usize).collect();
            self.write_str(&part)
        })
    }
    /// `append(CharSequence)` / `append(char)`: returns this writer.
    pub fn append(&self, a: impl WriterArg) -> Result<Writer, Throwable> {
        a.with_str(|s| self.write_str(s))?;
        Ok(self.clone())
    }
    /// `append(char)`.
    pub fn append_1(&self, a: impl WriterArg) -> Result<Writer, Throwable> {
        self.append(a)
    }
    /// `append(CharSequence, int start, int end)`.
    pub fn append_3(&self, a: impl WriterArg, start: i32, end: i32) -> Result<Writer, Throwable> {
        self.write_3(a, start, end - start)?;
        Ok(self.clone())
    }
    pub fn flush(&self) -> Result<(), Throwable> {
        self.inner.lock().flush()
    }
    pub fn close(&self) -> Result<(), Throwable> {
        self.inner.lock().close()
    }
    /// Identity (Java `==`).
    pub fn same(&self, other: &Writer) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        Writer::write_str(self, s).map_err(|_| std::fmt::Error)
    }
}

/// A `Writer` as a byte sink: the bytes are UTF-8 (serializers that write bytes).
impl std::io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Writer::write_str(self, &String::from_utf8_lossy(buf)).map_err(super::bytes::to_io)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Writer::flush(self).map_err(super::bytes::to_io)
    }
}

macro_rules! writer_handle {
    ($t:ident) => {
        impl std::ops::Deref for $t {
            type Target = Writer;
            fn deref(&self) -> &Writer {
                &self.base
            }
        }
        impl From<$t> for Writer {
            fn from(w: $t) -> Writer {
                w.base
            }
        }
    };
}

/// An `OutputStream` is written to as UTF-8 where Java takes either (`new PrintWriter(OutputStream)`).
impl From<OutputStream> for Writer {
    fn from(out: OutputStream) -> Writer {
        OutputStreamWriter::new(out).base
    }
}

struct EncodingSink {
    out: OutputStream,
    encoder: CharsetEncoder,
}
impl CharSink for EncodingSink {
    fn write_str(&mut self, s: &str) -> Result<(), Throwable> {
        let b = self.encoder.encode_bytes(s)?;
        self.out.inner.lock().write(&b).map_err(from_io)
    }
    fn flush(&mut self) -> Result<(), Throwable> {
        self.out.flush()
    }
    fn close(&mut self) -> Result<(), Throwable> {
        let r = self.out.flush();
        self.out.close()?;
        r
    }
}

/// `java.io.OutputStreamWriter`.
#[derive(Clone)]
pub struct OutputStreamWriter {
    pub base: Writer,
    charset: Charset,
}
writer_handle!(OutputStreamWriter);
impl OutputStreamWriter {
    /// `new OutputStreamWriter(OutputStream)`: UTF-8.
    pub fn new(out: OutputStream) -> Self {
        Self::new_2(out, standard_charsets::UTF_8)
    }
    /// `new OutputStreamWriter(OutputStream, Charset | String | CharsetEncoder)`.
    pub fn new_2(out: OutputStream, cs: impl Into<CharsetEncoder>) -> Self {
        let encoder: CharsetEncoder = cs.into();
        OutputStreamWriter { charset: encoder.charset, base: Writer::new(EncodingSink { out, encoder }) }
    }
    pub fn get_encoding(&self) -> Option<String> {
        Some(self.charset.name())
    }
}

struct BufferedSink {
    out: Writer,
    buf: String,
    size: usize,
}
impl CharSink for BufferedSink {
    fn write_str(&mut self, s: &str) -> Result<(), Throwable> {
        self.buf.push_str(s);
        if self.buf.len() >= self.size {
            let b = std::mem::take(&mut self.buf);
            self.out.write_str(&b)?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Throwable> {
        if !self.buf.is_empty() {
            let b = std::mem::take(&mut self.buf);
            self.out.write_str(&b)?;
        }
        self.out.flush()
    }
    fn close(&mut self) -> Result<(), Throwable> {
        let r = self.flush();
        self.out.close()?;
        r
    }
}

/// `java.io.BufferedWriter`.
#[derive(Clone)]
pub struct BufferedWriter {
    pub base: Writer,
}
writer_handle!(BufferedWriter);
impl BufferedWriter {
    pub fn new(out: impl Into<Writer>) -> Self {
        Self::new_2(out, 8192)
    }
    pub fn new_2(out: impl Into<Writer>, size: i32) -> Self {
        if size <= 0 {
            panic!("java.lang.IllegalArgumentException: Buffer size <= 0");
        }
        BufferedWriter { base: Writer::new(BufferedSink { out: out.into(), buf: String::new(), size: size as usize }) }
    }
    /// `newLine()`: the platform line separator.
    pub fn new_line(&self) -> Result<(), Throwable> {
        self.base.write_str(LINE_SEPARATOR)
    }
}

/// `System.lineSeparator()`.
#[cfg(windows)]
pub const LINE_SEPARATOR: &str = "\r\n";
#[cfg(not(windows))]
pub const LINE_SEPARATOR: &str = "\n";

struct StringSink(Arc<Mutex<String>>);
impl CharSink for StringSink {
    fn write_str(&mut self, s: &str) -> Result<(), Throwable> {
        self.0.lock().push_str(s);
        Ok(())
    }
}

/// `java.io.StringWriter`.
#[derive(Clone)]
pub struct StringWriter {
    pub base: Writer,
    buf: Arc<Mutex<String>>,
}
writer_handle!(StringWriter);
impl StringWriter {
    pub fn new() -> Self {
        let buf = Arc::new(Mutex::new(String::new()));
        StringWriter { base: Writer::new(StringSink(buf.clone())), buf }
    }
    pub fn new_1(_initial_size: i32) -> Self {
        Self::new()
    }
    pub fn to_string(&self) -> String {
        self.buf.lock().clone()
    }
    /// `getBuffer()` (a copy: Java's `StringBuffer` is a `String` here).
    pub fn get_buffer(&self) -> String {
        self.to_string()
    }
}
impl Default for StringWriter {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for StringWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.buf.lock())
    }
}

/// `java.io.FileWriter`.
#[derive(Clone)]
pub struct FileWriter {
    pub base: Writer,
}
writer_handle!(FileWriter);
impl FileWriter {
    /// `new FileWriter(File | String)`.
    pub fn new(file: impl AsRef<std::path::Path>) -> Result<Self, Throwable> {
        Self::new_2(file, false)
    }
    /// `new FileWriter(File, boolean append)`.
    pub fn new_2(file: impl AsRef<std::path::Path>, append: bool) -> Result<Self, Throwable> {
        Ok(FileWriter { base: OutputStreamWriter::new(FileOutputStream::new_2(file, append)?.base).base })
    }
}

/// `java.io.PrintWriter`: never throws; `check_error()` reports a failed write.
#[derive(Clone)]
pub struct PrintWriter {
    pub base: Writer,
    auto_flush: bool,
    error: Arc<std::sync::atomic::AtomicBool>,
}
writer_handle!(PrintWriter);
impl PrintWriter {
    /// `new PrintWriter(Writer | OutputStream)`.
    pub fn new(out: impl Into<Writer>) -> Self {
        Self::new_2(out, false)
    }
    /// `new PrintWriter(Writer | OutputStream, boolean autoFlush)`.
    pub fn new_2(out: impl Into<Writer>, auto_flush: bool) -> Self {
        PrintWriter { base: out.into(), auto_flush, error: Default::default() }
    }
    /// `new PrintWriter(File | String)`.
    pub fn new_file(file: impl AsRef<std::path::Path>) -> Result<Self, Throwable> {
        Ok(Self::new(FileWriter::new(file)?))
    }
    fn emit(&self, s: &str) {
        if self.base.write_str(s).is_err() {
            self.error.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    /// `print(x)`: any value's `toString()`.
    pub fn print(&self, v: impl std::fmt::Display) {
        self.emit(&v.to_string())
    }
    /// `println()`.
    pub fn println(&self) {
        self.emit(LINE_SEPARATOR);
        if self.auto_flush {
            self.flush();
        }
    }
    /// `println(x)`.
    pub fn println_1(&self, v: impl std::fmt::Display) {
        self.emit(&format!("{v}{LINE_SEPARATOR}"));
        if self.auto_flush {
            self.flush();
        }
    }
    /// `printf`/`format` with an already formatted string (Java format strings are not parsed).
    pub fn print_formatted(&self, s: &str) -> PrintWriter {
        self.emit(s);
        if self.auto_flush {
            self.flush();
        }
        self.clone()
    }
    /// `write(String)` / `write(char[])` (never throws).
    pub fn write_1(&self, a: impl WriterArg) {
        a.with_str(|s| self.emit(s))
    }
    /// `write(int)`.
    pub fn write(&self, c: i32) {
        if self.base.write(c).is_err() {
            self.error.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    pub fn flush(&self) {
        if self.base.flush().is_err() {
            self.error.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    pub fn close(&self) {
        if self.base.close().is_err() {
            self.error.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    pub fn check_error(&self) -> bool {
        self.flush();
        self.error.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[allow(dead_code)]
fn _unused() -> Throwable {
    throwable("java.io.IOException", "")
}
