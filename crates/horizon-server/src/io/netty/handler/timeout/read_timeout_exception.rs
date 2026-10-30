//! Port of `io.netty.handler.timeout.ReadTimeoutException`: fired by the transport's read timeout
//! (`ReadTimeoutHandler(30)` in Java) when the client sends nothing for 30 seconds.
use crate::io::netty::handler::timeout::timeout_exception::TimeoutExceptionTrait;
use crate::prelude::*;

crate::netty_exception!(ReadTimeoutException: TimeoutExceptionTrait);

/// `ReadTimeoutException.INSTANCE`.
pub static INSTANCE: LazyLock<JRef<ReadTimeoutException>> =
    LazyLock::new(ReadTimeoutException::new);
