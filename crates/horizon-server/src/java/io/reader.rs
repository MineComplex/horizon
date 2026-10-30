//! `java.io.Reader` and its classes (`InputStreamReader`, `StringReader`, `BufferedReader`,
//! `FileReader`). Java's `char` is a UTF-16 unit; the port reads Rust `char`s (whole code points).
use super::bytes::{from_io, throwable};
use super::input_stream::InputStream;
use super::writer::Writer;
use crate::java::lang::Throwable;
use crate::java::nio::charset::{Charset, CharsetDecoder, StreamDecoder};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// What a `Reader` reads characters from (Java: a `Reader` subclass's `read(char[], int, int)`).
pub trait CharSource: Send {
    /// Appends at least one character to `out`, or returns `Ok(false)` at the end.
    fn fill(&mut self, out: &mut VecDeque<char>) -> Result<bool, Throwable>;
    /// Whether a read would not block.
    fn ready(&mut self) -> Result<bool, Throwable> {
        Ok(false)
    }
    fn close(&mut self) -> Result<(), Throwable> {
        Ok(())
    }
}

pub struct ReaderState {
    source: Box<dyn CharSource>,
    buf: VecDeque<char>,
    mark: Option<usize>,
    marked_read: Vec<char>,
    closed: bool,
}

impl ReaderState {
    fn next(&mut self) -> Result<Option<char>, Throwable> {
        if self.closed {
            return Err(throwable("java.io.IOException", "Stream closed"));
        }
        while self.buf.is_empty() {
            if !self.source.fill(&mut self.buf)? {
                return Ok(None);
            }
        }
        let c = self.buf.pop_front();
        if let (Some(c), Some(limit)) = (c, self.mark) {
            if self.marked_read.len() < limit {
                self.marked_read.push(c);
            } else {
                self.mark = None;
                self.marked_read.clear();
            }
        }
        Ok(c)
    }
    fn peek(&mut self) -> Result<Option<char>, Throwable> {
        while self.buf.is_empty() {
            if !self.source.fill(&mut self.buf)? {
                return Ok(None);
            }
        }
        Ok(self.buf.front().copied())
    }
}

/// `java.io.Reader` (a handle: clones share the reader).
#[derive(Clone)]
pub struct Reader {
    pub inner: Arc<Mutex<ReaderState>>,
}

impl Reader {
    /// A reader over `source` (Java: a `Reader` subclass).
    pub fn new(source: impl CharSource + 'static) -> Self {
        Reader {
            inner: Arc::new(Mutex::new(ReaderState {
                source: Box::new(source),
                buf: VecDeque::new(),
                mark: None,
                marked_read: Vec::new(),
                closed: false,
            })),
        }
    }
    /// `Reader.nullReader()`.
    pub fn null_reader() -> Self {
        StringReader::new("").base
    }
    /// `read()`: the next character as an `int`, or -1 at the end.
    pub fn read(&self) -> Result<i32, Throwable> {
        Ok(self.inner.lock().next()?.map_or(-1, |c| c as i32))
    }
    /// `read(char[])`.
    pub fn read_1(&self, cbuf: &mut Vec<char>) -> Result<i32, Throwable> {
        let len = cbuf.len() as i32;
        self.read_3(cbuf, 0, len)
    }
    /// `read(char[], int, int)`: characters read, or -1 at the end.
    pub fn read_3(&self, cbuf: &mut [char], off: i32, len: i32) -> Result<i32, Throwable> {
        let (s, e) = super::bytes::check_range(off, len, cbuf.len());
        if s == e {
            return Ok(0);
        }
        let mut g = self.inner.lock();
        let mut n = 0;
        while s + n < e {
            // Java returns what is available once at least one character has been read.
            if n > 0 && g.buf.is_empty() {
                break;
            }
            match g.next()? {
                Some(c) => {
                    cbuf[s + n] = c;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(if n == 0 { -1 } else { n as i32 })
    }
    pub fn skip(&self, n: i64) -> Result<i64, Throwable> {
        if n < 0 {
            panic!("java.lang.IllegalArgumentException: skip value is negative");
        }
        let mut g = self.inner.lock();
        let mut k = 0;
        while k < n {
            if g.next()?.is_none() {
                break;
            }
            k += 1;
        }
        Ok(k)
    }
    pub fn ready(&self) -> Result<bool, Throwable> {
        let mut g = self.inner.lock();
        if g.closed {
            return Err(throwable("java.io.IOException", "Stream closed"));
        }
        if !g.buf.is_empty() {
            return Ok(true);
        }
        g.source.ready()
    }
    pub fn mark_supported(&self) -> bool {
        true
    }
    pub fn mark(&self, read_ahead_limit: i32) -> Result<(), Throwable> {
        let mut g = self.inner.lock();
        g.marked_read.clear();
        g.mark = Some(read_ahead_limit.max(0) as usize);
        Ok(())
    }
    pub fn reset(&self) -> Result<(), Throwable> {
        let mut g = self.inner.lock();
        if g.mark.is_none() {
            return Err(throwable("java.io.IOException", "Stream not marked"));
        }
        let back = std::mem::take(&mut g.marked_read);
        for c in back.into_iter().rev() {
            g.buf.push_front(c);
        }
        Ok(())
    }
    pub fn close(&self) -> Result<(), Throwable> {
        let mut g = self.inner.lock();
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        g.buf.clear();
        g.source.close()
    }
    /// `transferTo(Writer)`.
    pub fn transfer_to(&self, out: Writer) -> Result<i64, Throwable> {
        let mut n = 0i64;
        let mut s = String::new();
        loop {
            let c = self.inner.lock().next()?;
            match c {
                Some(c) => {
                    s.push(c);
                    n += 1;
                    if s.len() >= 8192 {
                        out.write_str(&s)?;
                        s.clear();
                    }
                }
                None => break,
            }
        }
        out.write_str(&s)?;
        Ok(n)
    }
    /// Reads everything left into a `String` (what `CharStreams.toString`/`IOUtils` do).
    pub fn read_to_string(&self) -> Result<String, Throwable> {
        let mut s = String::new();
        let mut g = self.inner.lock();
        while let Some(c) = g.next()? {
            s.push(c);
        }
        Ok(s)
    }
    /// `BufferedReader.readLine()`'s body: `None` at the end; `\n`, `\r` and `\r\n` end a line.
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        let mut g = self.inner.lock();
        let mut s = String::new();
        let mut any = false;
        loop {
            match g.next()? {
                None => return Ok(if any { Some(s) } else { None }),
                Some('\n') => return Ok(Some(s)),
                Some('\r') => {
                    if g.peek()? == Some('\n') {
                        g.next()?;
                    }
                    return Ok(Some(s));
                }
                Some(c) => {
                    any = true;
                    s.push(c);
                }
            }
        }
    }
    /// Identity (Java `==`).
    pub fn same(&self, other: &Reader) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::io::Read for Reader {
    /// The characters as UTF-8 bytes (for parsers that read bytes).
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0;
        let mut g = self.inner.lock();
        while n + 4 <= buf.len() || (n == 0 && !buf.is_empty()) {
            let c = match g.peek().map_err(super::bytes::to_io)? {
                Some(c) => c,
                None => break,
            };
            let l = c.len_utf8();
            if n + l > buf.len() {
                if n == 0 {
                    return Err(std::io::Error::other("buffer too small for one character"));
                }
                break;
            }
            g.next().map_err(super::bytes::to_io)?;
            c.encode_utf8(&mut buf[n..]);
            n += l;
            if g.buf.is_empty() {
                break;
            }
        }
        Ok(n)
    }
}

macro_rules! reader_handle {
    ($t:ident) => {
        impl std::ops::Deref for $t {
            type Target = Reader;
            fn deref(&self) -> &Reader {
                &self.base
            }
        }
        impl From<$t> for Reader {
            fn from(r: $t) -> Reader {
                r.base
            }
        }
    };
}

/// Decodes an `InputStream`.
struct StreamCharSource {
    in_: InputStream,
    decoder: StreamDecoder,
    eof: bool,
}
impl CharSource for StreamCharSource {
    fn fill(&mut self, out: &mut VecDeque<char>) -> Result<bool, Throwable> {
        let mut buf = vec![0u8; 8192];
        loop {
            if self.eof {
                return Ok(false);
            }
            let n = self.in_.inner.lock().read(&mut buf).map_err(from_io)?;
            let mut s = String::new();
            if n == 0 {
                self.eof = true;
                self.decoder.decode(&[], true, &mut s)?;
            } else {
                self.decoder.decode(&buf[..n], false, &mut s)?;
            }
            if !s.is_empty() {
                out.extend(s.chars());
                return Ok(true);
            }
        }
    }
    fn ready(&mut self) -> Result<bool, Throwable> {
        Ok(self.in_.inner.lock().available().map_err(from_io)? > 0)
    }
    fn close(&mut self) -> Result<(), Throwable> {
        self.in_.close()
    }
}

/// `java.io.InputStreamReader`.
#[derive(Clone)]
pub struct InputStreamReader {
    pub base: Reader,
    charset: Charset,
}
reader_handle!(InputStreamReader);
impl InputStreamReader {
    /// `new InputStreamReader(InputStream)`: UTF-8.
    pub fn new(in_: InputStream) -> Self {
        Self::new_2(in_, crate::java::nio::charset::standard_charsets::UTF_8)
    }
    /// `new InputStreamReader(InputStream, Charset | String | CharsetDecoder)`. A `CharsetDecoder`
    /// set to `REPORT` makes malformed input throw `MalformedInputException`.
    pub fn new_2(in_: InputStream, cs: impl Into<CharsetDecoder>) -> Self {
        let d: CharsetDecoder = cs.into();
        InputStreamReader {
            base: Reader::new(StreamCharSource { in_, decoder: StreamDecoder::new(d.charset, d.is_strict()), eof: false }),
            charset: d.charset,
        }
    }
    pub fn get_encoding(&self) -> Option<String> {
        Some(self.charset.name())
    }
}

struct StringSource(Option<String>);
impl CharSource for StringSource {
    fn fill(&mut self, out: &mut VecDeque<char>) -> Result<bool, Throwable> {
        match self.0.take() {
            Some(s) if !s.is_empty() => {
                out.extend(s.chars());
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    fn ready(&mut self) -> Result<bool, Throwable> {
        Ok(true)
    }
}

/// `java.io.StringReader`.
#[derive(Clone)]
pub struct StringReader {
    pub base: Reader,
}
reader_handle!(StringReader);
impl StringReader {
    pub fn new(s: &str) -> Self {
        StringReader { base: Reader::new(StringSource(Some(s.to_string()))) }
    }
}

/// Reads through another `Reader`.
struct ReaderSource(Reader);
impl CharSource for ReaderSource {
    fn fill(&mut self, out: &mut VecDeque<char>) -> Result<bool, Throwable> {
        let mut g = self.0.inner.lock();
        match g.next()? {
            Some(c) => {
                out.push_back(c);
                out.extend(g.buf.drain(..));
                Ok(true)
            }
            None => Ok(false),
        }
    }
    fn ready(&mut self) -> Result<bool, Throwable> {
        self.0.ready()
    }
    fn close(&mut self) -> Result<(), Throwable> {
        self.0.close()
    }
}

/// `java.io.BufferedReader`.
#[derive(Clone)]
pub struct BufferedReader {
    pub base: Reader,
}
reader_handle!(BufferedReader);
impl BufferedReader {
    pub fn new(in_: impl Into<Reader>) -> Self {
        BufferedReader { base: Reader::new(ReaderSource(in_.into())) }
    }
    pub fn new_2(in_: impl Into<Reader>, _size: i32) -> Self {
        Self::new(in_)
    }
    /// `readLine()`.
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        self.base.read_line()
    }
    /// `lines()`: the remaining lines (an `UncheckedIOException` panics, as Java throws it).
    pub fn lines(&self) -> BufferedReaderLines {
        BufferedReaderLines { reader: self.base.clone() }
    }
}

/// The iterator `BufferedReader.lines()` returns (Java: a `Stream<String>`).
pub struct BufferedReaderLines {
    reader: Reader,
}
impl Iterator for BufferedReaderLines {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        match self.reader.read_line() {
            Ok(l) => l,
            Err(e) => panic!("java.io.UncheckedIOException: {e}"),
        }
    }
}

/// `java.io.FileReader` (UTF-8 unless a charset is given).
#[derive(Clone)]
pub struct FileReader {
    pub base: Reader,
}
reader_handle!(FileReader);
impl FileReader {
    /// `new FileReader(File | String)`.
    pub fn new(file: impl AsRef<std::path::Path>) -> Result<Self, Throwable> {
        Ok(FileReader { base: InputStreamReader::new(super::input_stream::FileInputStream::new(file)?.base).base })
    }
    /// `new FileReader(File, Charset)`.
    pub fn new_2(file: impl AsRef<std::path::Path>, cs: Charset) -> Result<Self, Throwable> {
        Ok(FileReader { base: InputStreamReader::new_2(super::input_stream::FileInputStream::new(file)?.base, cs).base })
    }
}

/// `java.io.LineNumberReader`.
#[derive(Clone)]
pub struct LineNumberReader {
    pub base: Reader,
    line: Arc<std::sync::atomic::AtomicI32>,
}
reader_handle!(LineNumberReader);
impl LineNumberReader {
    pub fn new(in_: impl Into<Reader>) -> Self {
        LineNumberReader { base: Reader::new(ReaderSource(in_.into())), line: Default::default() }
    }
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        let l = self.base.read_line()?;
        if l.is_some() {
            self.line.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(l)
    }
    pub fn get_line_number(&self) -> i32 {
        self.line.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn set_line_number(&self, n: i32) {
        self.line.store(n, std::sync::atomic::Ordering::Relaxed)
    }
}
