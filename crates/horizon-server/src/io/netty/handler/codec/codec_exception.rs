//! `io.netty.handler.codec.CodecException`: the superclass of decoder and encoder exceptions, for
//! `instanceof` (`throwable_util::is::<dyn CodecExceptionTrait>(&t)`).
use crate::prelude::*;

pub trait CodecExceptionTrait: JObject {}
