//! `com.velocitypowered.natives`: Velocity's compression and cipher natives, as Paper uses them for the
//! connection's zlib compression and AES/CFB8 encryption. The port has only the Java variants
//! (`JavaVelocityCompressor` on flate2's zlib, `JavaVelocityCipher` on an in-tree AES-128/CFB8).
pub mod compression;
pub mod encryption;
pub mod util;

pub mod buffer_preference {
    //! `com.velocitypowered.natives.util.BufferPreference`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BufferPreference {
        /// Heap buffers are preferred.
        HEAP_PREFERRED,
        /// Direct buffers are preferred.
        DIRECT_PREFERRED,
        /// Direct buffers are required.
        DIRECT_REQUIRED,
    }
}
