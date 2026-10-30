//! Port of `io.netty.buffer.ByteBufOutputStream`: an `OutputStream`/`DataOutput` appending to a buffer.
//! `FriendlyByteBuf.writeNbt` writes NBT through it.
//!
//! PORT: the java.io port's `DataOutput`/`OutputStream` need a constructor from this type; it implements
//! `std::io::Write` and every `DataOutput` method.
use crate::io::netty::buffer::byte_buf::ByteBuf;
use crate::io::netty::buffer::byte_buf_input_stream::encode_modified_utf8;
use crate::java::lang::Throwable;
use crate::prelude::*;

pub struct ByteBufOutputStream {
    pub this: Weak<Self>,
    pub buffer: ByteBuf,
    pub start_index: i32,
    pub release_on_close: bool,
    pub closed: Mut<bool>,
}
crate::jobject!(ByteBufOutputStream);

impl ByteBufOutputStream {
    pub fn this(&self) -> JRef<Self> {
        self.this.upgrade().expect("this")
    }
    pub fn new(buffer: ByteBuf) -> JRef<Self> {
        Self::new_2(buffer, false)
    }
    pub fn new_2(buffer: ByteBuf, release_on_close: bool) -> JRef<Self> {
        let start_index = buffer.writer_index();
        Arc::new_cyclic(|this| ByteBufOutputStream {
            this: this.clone(),
            buffer,
            start_index,
            release_on_close,
            closed: Mut::new(false),
        })
    }
    /// `writtenBytes()`.
    pub fn written_bytes(&self) -> i32 {
        self.buffer.writer_index() - self.start_index
    }
    pub fn buffer(&self) -> ByteBuf {
        self.buffer.clone()
    }
    /// `write(b, off, len)`.
    pub fn write_3(&self, b: &[i8], off: i32, len: i32) -> Result<(), Throwable> {
        if len == 0 {
            return Ok(());
        }
        self.buffer.write_bytes_3_2(b, off, len);
        Ok(())
    }
    /// `write(byte[])`.
    pub fn write_1(&self, b: &[i8]) -> Result<(), Throwable> {
        self.buffer.write_bytes_1(b);
        Ok(())
    }
    /// `write(int)`: one byte.
    pub fn write(&self, b: i32) -> Result<(), Throwable> {
        self.buffer.write_byte(b);
        Ok(())
    }
    pub fn write_boolean(&self, v: bool) -> Result<(), Throwable> {
        self.buffer.write_boolean(v);
        Ok(())
    }
    pub fn write_byte(&self, v: i32) -> Result<(), Throwable> {
        self.buffer.write_byte(v);
        Ok(())
    }
    /// `writeBytes(String)`: the low byte of each char.
    pub fn write_bytes(&self, s: &str) -> Result<(), Throwable> {
        let bytes: Vec<u8> = s.encode_utf16().map(|u| u as u8).collect();
        self.buffer.write_byte_slice(&bytes);
        Ok(())
    }
    pub fn write_char(&self, v: i32) -> Result<(), Throwable> {
        self.buffer.write_char(v);
        Ok(())
    }
    /// `writeChars(String)`: each char as two bytes.
    pub fn write_chars(&self, s: &str) -> Result<(), Throwable> {
        for u in s.encode_utf16() {
            self.buffer.write_char(u as i32);
        }
        Ok(())
    }
    pub fn write_double(&self, v: f64) -> Result<(), Throwable> {
        self.buffer.write_double(v);
        Ok(())
    }
    pub fn write_float(&self, v: f32) -> Result<(), Throwable> {
        self.buffer.write_float(v);
        Ok(())
    }
    pub fn write_int(&self, v: i32) -> Result<(), Throwable> {
        self.buffer.write_int(v);
        Ok(())
    }
    pub fn write_long(&self, v: i64) -> Result<(), Throwable> {
        self.buffer.write_long(v);
        Ok(())
    }
    pub fn write_short(&self, v: i32) -> Result<(), Throwable> {
        self.buffer.write_short(v);
        Ok(())
    }
    /// `writeUTF(String)`: modified UTF-8 with an unsigned short length (UTFDataFormatException over 65535).
    pub fn write_utf(&self, s: &str) -> Result<(), Throwable> {
        let bytes = encode_modified_utf8(s);
        if bytes.len() > 65535 {
            panic!(
                "UTFDataFormatException: encoded string too long: {} bytes",
                bytes.len()
            );
        }
        self.buffer.write_short(bytes.len() as i32);
        self.buffer.write_byte_slice(&bytes);
        Ok(())
    }
    pub fn flush(&self) -> Result<(), Throwable> {
        Ok(())
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
}

impl std::io::Write for &ByteBufOutputStream {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buffer.write_byte_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
