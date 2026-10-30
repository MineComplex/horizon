//! Port of `io.netty.buffer.ByteBufInputStream`: an `InputStream`/`DataInput` reading a buffer's readable
//! bytes (advancing its reader index). `FriendlyByteBuf.readNbt` reads NBT through it; `NbtIo.read`
//! recognises it (`input instanceof ByteBufInputStream`).
//!
//! PORT: the java.io port's `DataInput`/`InputStream` handles need a constructor from this type
//! (`DataInput::from(ByteBufInputStream)`); it implements `std::io::Read` and every `DataInput` method.
use crate::io::netty::buffer::byte_buf::ByteBuf;
use crate::java::lang::Throwable;
use crate::prelude::*;

pub struct ByteBufInputStream {
    pub this: Weak<Self>,
    pub buffer: ByteBuf,
    pub start_index: i32,
    pub end_index: i32,
    pub closed: Mut<bool>,
    pub release_on_close: bool,
}
crate::jobject!(ByteBufInputStream);

/// `java.io.EOFException`.
pub struct EofException;
crate::jobject!(EofException);

fn eof() -> Throwable {
    crate::io::netty::util::internal::throwable_util::new_throwable(
        Arc::new(EofException),
        None,
        None,
    )
}

impl ByteBufInputStream {
    pub fn this(&self) -> JRef<Self> {
        self.this.upgrade().expect("this")
    }
    /// `new ByteBufInputStream(buffer)`: all readable bytes.
    pub fn new(buffer: ByteBuf) -> JRef<Self> {
        let n = buffer.readable_bytes();
        Self::new_3(buffer, n, false)
    }
    /// `new ByteBufInputStream(buffer, releaseOnClose)`.
    pub fn new_2(buffer: ByteBuf, release_on_close: bool) -> JRef<Self> {
        let n = buffer.readable_bytes();
        Self::new_3(buffer, n, release_on_close)
    }
    /// `new ByteBufInputStream(buffer, length, releaseOnClose)`.
    pub fn new_3(buffer: ByteBuf, length: i32, release_on_close: bool) -> JRef<Self> {
        if length < 0 || length > buffer.readable_bytes() {
            if release_on_close {
                buffer.release();
            }
            panic!(
                "IndexOutOfBoundsException: Too many bytes to be read - Needs {length}, maximum is {}",
                buffer.readable_bytes()
            );
        }
        let start_index = buffer.reader_index();
        Arc::new_cyclic(|this| ByteBufInputStream {
            this: this.clone(),
            buffer: buffer.clone(),
            start_index,
            end_index: start_index + length,
            closed: Mut::new(false),
            release_on_close,
        })
    }
    /// `readBytes()`: the bytes read so far.
    pub fn read_bytes(&self) -> i32 {
        self.buffer.reader_index() - self.start_index
    }
    pub fn close(&self) -> Result<(), Throwable> {
        if !self.closed.get() {
            self.closed.set(true);
            if self.release_on_close {
                self.buffer.release();
            }
        }
        Ok(())
    }
    pub fn available(&self) -> Result<i32, Throwable> {
        Ok(self.end_index - self.buffer.reader_index())
    }
    pub fn mark(&self, readlimit: i32) {
        let _ = readlimit;
        self.buffer.mark_reader_index();
    }
    pub fn mark_supported(&self) -> bool {
        true
    }
    pub fn reset(&self) -> Result<(), Throwable> {
        self.buffer.reset_reader_index();
        Ok(())
    }
    /// `read()`: the next byte (0-255), -1 at the end.
    pub fn read(&self) -> Result<i32, Throwable> {
        if self.available()? == 0 {
            return Ok(-1);
        }
        Ok(self.buffer.read_byte() as i32 & 0xff)
    }
    /// `read(b, off, len)`: the bytes read, -1 at the end.
    pub fn read_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<i32, Throwable> {
        let available = self.available()?;
        if available == 0 {
            return Ok(-1);
        }
        let len = available.min(len);
        self.buffer.read_bytes_3_2(b, off, len);
        Ok(len)
    }
    pub fn skip(&self, n: i64) -> Result<i64, Throwable> {
        let n = if n > i32::MAX as i64 {
            i32::MAX
        } else {
            n as i32
        };
        Ok(self.skip_bytes(n)? as i64)
    }
    fn check_available(&self, field_size: i32) -> Result<(), Throwable> {
        if field_size < 0 {
            panic!("IndexOutOfBoundsException: fieldSize cannot be a negative number");
        }
        if field_size > self.available()? {
            return Err(eof());
        }
        Ok(())
    }
    pub fn read_boolean(&self) -> Result<bool, Throwable> {
        self.check_available(1)?;
        Ok(self.read()? != 0)
    }
    pub fn read_byte(&self) -> Result<i8, Throwable> {
        let available = self.available()?;
        if available == 0 {
            return Err(eof());
        }
        Ok(self.buffer.read_byte())
    }
    pub fn read_char(&self) -> Result<char, Throwable> {
        Ok(char::from_u32(self.read_short()? as u16 as u32).unwrap_or('\u{FFFD}'))
    }
    pub fn read_double(&self) -> Result<f64, Throwable> {
        Ok(f64::from_bits(self.read_long()? as u64))
    }
    pub fn read_float(&self) -> Result<f32, Throwable> {
        Ok(f32::from_bits(self.read_int()? as u32))
    }
    /// `readFully(b)`.
    pub fn read_fully(&self, b: &mut [i8]) -> Result<(), Throwable> {
        let n = b.len() as i32;
        self.read_fully_3(b, 0, n)
    }
    pub fn read_fully_3(&self, b: &mut [i8], off: i32, len: i32) -> Result<(), Throwable> {
        self.check_available(len)?;
        self.buffer.read_bytes_3_2(b, off, len);
        Ok(())
    }
    pub fn read_int(&self) -> Result<i32, Throwable> {
        self.check_available(4)?;
        Ok(self.buffer.read_int())
    }
    /// `readLine()`: up to `\n` (dropping `\r\n`); `None` at the end.
    pub fn read_line(&self) -> Result<Option<String>, Throwable> {
        let mut available = self.available()?;
        if available == 0 {
            return Ok(None);
        }
        let mut line = String::new();
        loop {
            let c = self.buffer.read_unsigned_byte() as u8;
            available -= 1;
            if c == b'\n' {
                break;
            }
            if c == b'\r' {
                if available > 0
                    && self.buffer.get_unsigned_byte(self.buffer.reader_index()) as u8 == b'\n'
                {
                    self.buffer.skip_bytes(1);
                }
                break;
            }
            line.push(c as char);
            if available == 0 {
                break;
            }
        }
        Ok(Some(line))
    }
    pub fn read_long(&self) -> Result<i64, Throwable> {
        self.check_available(8)?;
        Ok(self.buffer.read_long())
    }
    pub fn read_short(&self) -> Result<i16, Throwable> {
        self.check_available(2)?;
        Ok(self.buffer.read_short())
    }
    /// `readUTF()`: Java's modified UTF-8 with an unsigned short length.
    pub fn read_utf(&self) -> Result<String, Throwable> {
        let len = self.read_unsigned_short()?;
        self.check_available(len)?;
        let bytes = self.buffer.read_byte_vec(len);
        Ok(decode_modified_utf8(&bytes))
    }
    pub fn read_unsigned_byte(&self) -> Result<i32, Throwable> {
        Ok(self.read_byte()? as i32 & 0xff)
    }
    pub fn read_unsigned_short(&self) -> Result<i32, Throwable> {
        Ok(self.read_short()? as i32 & 0xffff)
    }
    pub fn skip_bytes(&self, n: i32) -> Result<i32, Throwable> {
        let n_bytes = n.min(self.available()?);
        self.buffer.skip_bytes(n_bytes);
        Ok(n_bytes)
    }
}

impl std::io::Read for &ByteBufInputStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let available = (self.end_index - self.buffer.reader_index()).max(0) as usize;
        let n = available.min(out.len());
        let bytes = self.buffer.read_byte_vec(n as i32);
        out[..n].copy_from_slice(&bytes);
        Ok(n)
    }
}

/// Java's modified UTF-8 (`DataInput.readUTF`): `0xC0 0x80` is NUL, surrogates are encoded separately.
pub fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as u16;
        if c < 0x80 {
            units.push(c);
            i += 1;
        } else if c >> 5 == 0b110 && i + 1 < bytes.len() {
            units.push(((c & 0x1f) << 6) | (bytes[i + 1] as u16 & 0x3f));
            i += 2;
        } else if c >> 4 == 0b1110 && i + 2 < bytes.len() {
            units.push(
                ((c & 0x0f) << 12)
                    | ((bytes[i + 1] as u16 & 0x3f) << 6)
                    | (bytes[i + 2] as u16 & 0x3f),
            );
            i += 3;
        } else {
            units.push(0xFFFD);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

/// Java's modified UTF-8 encoding (`DataOutput.writeUTF`), without the length prefix.
pub fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for u in s.encode_utf16() {
        if u != 0 && u < 0x80 {
            out.push(u as u8);
        } else if u < 0x800 {
            out.push(0xc0 | ((u >> 6) & 0x1f) as u8);
            out.push(0x80 | (u & 0x3f) as u8);
        } else {
            out.push(0xe0 | ((u >> 12) & 0x0f) as u8);
            out.push(0x80 | ((u >> 6) & 0x3f) as u8);
            out.push(0x80 | (u & 0x3f) as u8);
        }
    }
    out
}
