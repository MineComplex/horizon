//! Port of `io.netty.handler.timeout.TimeoutException` (extends ChannelException). `Connection.exceptionCaught`
//! tests `cause instanceof TimeoutException`: `throwable_util::is::<dyn TimeoutExceptionTrait>(&cause)`.
use crate::prelude::*;

pub trait TimeoutExceptionTrait: JObject {}

crate::netty_exception!(TimeoutException: TimeoutExceptionTrait);
