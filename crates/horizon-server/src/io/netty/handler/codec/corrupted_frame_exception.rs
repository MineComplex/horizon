//! Port of `io.netty.handler.codec.CorruptedFrameException` (extends DecoderException).
use crate::io::netty::handler::codec::codec_exception::CodecExceptionTrait;
use crate::io::netty::handler::codec::decoder_exception::DecoderExceptionTrait;
use crate::prelude::*;

crate::netty_exception!(CorruptedFrameException: CodecExceptionTrait, DecoderExceptionTrait);
