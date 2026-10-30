//! Port of `com.velocitypowered.natives.compression.VelocityCompressorFactory`.
use crate::com::velocitypowered::natives::compression::java_velocity_compressor::JavaVelocityCompressor;
use crate::com::velocitypowered::natives::compression::velocity_compressor::VelocityCompressor;
use crate::prelude::*;

#[derive(Clone, Copy)]
pub struct VelocityCompressorFactory;

impl VelocityCompressorFactory {
    /// `create(level)`: a compressor at zlib level `level` (-1 = the default, 6).
    pub fn create(&self, level: i32) -> VelocityCompressor {
        VelocityCompressor(JavaVelocityCompressor::new(level))
    }
}
