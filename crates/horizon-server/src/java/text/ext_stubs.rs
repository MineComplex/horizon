//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DateFormat(pub std::marker::PhantomData<()>);
impl Clone for DateFormat { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DecimalFormat(pub std::marker::PhantomData<()>);
impl Clone for DecimalFormat { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
