//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DateTimeFormatter(pub std::marker::PhantomData<()>);
impl Clone for DateTimeFormatter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FormatStyle(pub std::marker::PhantomData<()>);
impl Clone for FormatStyle { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
