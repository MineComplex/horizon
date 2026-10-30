//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BigDecimal(pub std::marker::PhantomData<()>);
impl Clone for BigDecimal { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct BigInteger(pub std::marker::PhantomData<()>);
impl Clone for BigInteger { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
