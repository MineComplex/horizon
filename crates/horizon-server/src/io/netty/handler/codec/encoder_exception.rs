//! Port of `io.netty.handler.codec.EncoderException` (a RuntimeException).
use crate::io::netty::handler::codec::codec_exception::CodecExceptionTrait;
use crate::prelude::*;

pub trait EncoderExceptionTrait: CodecExceptionTrait {}

crate::netty_exception!(EncoderException: CodecExceptionTrait, EncoderExceptionTrait);
