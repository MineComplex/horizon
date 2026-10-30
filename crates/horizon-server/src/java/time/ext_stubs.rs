//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Instant(pub std::marker::PhantomData<()>);
impl Clone for Instant { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MonthDay(pub std::marker::PhantomData<()>);
impl Clone for MonthDay { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ZonedDateTime(pub std::marker::PhantomData<()>);
impl Clone for ZonedDateTime { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
