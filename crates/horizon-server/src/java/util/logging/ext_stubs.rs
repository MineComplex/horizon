//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Level(pub std::marker::PhantomData<()>);
impl Clone for Level { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LogRecord(pub std::marker::PhantomData<()>);
impl Clone for LogRecord { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Logger(pub std::marker::PhantomData<()>);
impl Clone for Logger { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
