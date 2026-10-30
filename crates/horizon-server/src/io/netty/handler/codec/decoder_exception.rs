//! Port of `io.netty.handler.codec.DecoderException` (a RuntimeException). Subclasses (NMS's
//! `SkipPacketDecoderException`, `CorruptedFrameException`) implement `DecoderExceptionTrait` too.
use crate::io::netty::handler::codec::codec_exception::CodecExceptionTrait;
use crate::prelude::*;

pub trait DecoderExceptionTrait: CodecExceptionTrait {}

crate::netty_exception!(DecoderException: CodecExceptionTrait, DecoderExceptionTrait);
