//! Port of `com.velocitypowered.natives.compression.VelocityCompressor` (an interface: `Disposable`,
//! `Native`). A cheap handle: clones are the same compressor (Java reference semantics).
use crate::com::velocitypowered::natives::buffer_preference::BufferPreference;
use crate::com::velocitypowered::natives::compression::java_velocity_compressor::JavaVelocityCompressor;
use crate::io::netty::buffer::ByteBuf;
use crate::java::lang::Throwable;
use crate::prelude::*;

#[derive(Clone)]
pub struct VelocityCompressor(pub JRef<JavaVelocityCompressor>);

impl VelocityCompressor {
    /// `inflate(source, destination, uncompressedSize)`: inflates `source`'s readable zlib stream into
    /// `destination`, advancing both indices. DataFormatException on a bad or oversized stream.
    pub fn inflate(&self, source: ByteBuf, destination: ByteBuf, uncompressed_size: i32) -> Result<(), Throwable> {
        self.0.inflate(source, destination, uncompressed_size)
    }
    /// `deflate(source, destination)`: deflates `source`'s readable bytes (zlib) onto `destination`.
    pub fn deflate(&self, source: ByteBuf, destination: ByteBuf) -> Result<(), Throwable> {
        self.0.deflate(source, destination)
    }
    /// `close()` (`Disposable`).
    pub fn close(&self) {
        self.0.close()
    }
    /// `preferredBufferType()` (`Native`).
    pub fn preferred_buffer_type(&self) -> BufferPreference {
        self.0.preferred_buffer_type()
    }
}
