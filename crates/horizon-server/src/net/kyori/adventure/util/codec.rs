//! Port of `net.kyori.adventure.util.Codec` (with `Codec.Decoder` and `Codec.Encoder`).
//! `decode`/`encode` "throw" their exception types: they return `Result`.
use crate::prelude::*;

// ---- interface net.kyori.adventure.util.Codec.Decoder
pub struct CodecDecoder<D = JObjectRef, E = JObjectRef, X = JObjectRef>(pub Arc<dyn Fn(E) -> Result<D, X> + Send + Sync>);
impl<D, E, X> Clone for CodecDecoder<D, E, X> {
    fn clone(&self) -> Self {
        CodecDecoder(self.0.clone())
    }
}
impl<D, E, X> CodecDecoder<D, E, X> {
    pub fn new(f: impl Fn(E) -> Result<D, X> + Send + Sync + 'static) -> Self {
        CodecDecoder(Arc::new(f))
    }
    pub fn decode(&self, encoded: E) -> Result<D, X> {
        (self.0)(encoded)
    }
}

// ---- interface net.kyori.adventure.util.Codec.Encoder
pub struct CodecEncoder<D = JObjectRef, E = JObjectRef, X = JObjectRef>(pub Arc<dyn Fn(D) -> Result<E, X> + Send + Sync>);
impl<D, E, X> Clone for CodecEncoder<D, E, X> {
    fn clone(&self) -> Self {
        CodecEncoder(self.0.clone())
    }
}
impl<D, E, X> CodecEncoder<D, E, X> {
    pub fn new(f: impl Fn(D) -> Result<E, X> + Send + Sync + 'static) -> Self {
        CodecEncoder(Arc::new(f))
    }
    pub fn encode(&self, decoded: D) -> Result<E, X> {
        (self.0)(decoded)
    }
}

// ---- interface net.kyori.adventure.util.Codec
pub struct Codec<D = JObjectRef, E = JObjectRef, DX = JObjectRef, EX = JObjectRef> {
    pub decoder: CodecDecoder<D, E, DX>,
    pub encoder: CodecEncoder<D, E, EX>,
}
impl<D, E, DX, EX> Clone for Codec<D, E, DX, EX> {
    fn clone(&self) -> Self {
        Codec { decoder: self.decoder.clone(), encoder: self.encoder.clone() }
    }
}
impl<D, E, DX, EX> Codec<D, E, DX, EX> {
    /// `Codec.codec(Decoder, Encoder)`.
    pub fn codec(
        decoder: impl Fn(E) -> Result<D, DX> + Send + Sync + 'static,
        encoder: impl Fn(D) -> Result<E, EX> + Send + Sync + 'static,
    ) -> Self {
        Codec { decoder: CodecDecoder::new(decoder), encoder: CodecEncoder::new(encoder) }
    }
    /// `Codec.of(Decoder, Encoder)`.
    pub fn of(decoder: CodecDecoder<D, E, DX>, encoder: CodecEncoder<D, E, EX>) -> Self {
        Codec { decoder, encoder }
    }
    pub fn decode(&self, encoded: E) -> Result<D, DX> {
        self.decoder.decode(encoded)
    }
    pub fn encode(&self, decoded: D) -> Result<E, EX> {
        self.encoder.encode(decoded)
    }
    pub fn decoder(&self) -> CodecDecoder<D, E, DX> {
        self.decoder.clone()
    }
    pub fn encoder(&self) -> CodecEncoder<D, E, EX> {
        self.encoder.clone()
    }
}
