//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct HashCode(pub std::marker::PhantomData<()>);
impl Clone for HashCode { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct HashFunction(pub std::marker::PhantomData<()>);
impl Clone for HashFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Hasher(pub std::marker::PhantomData<()>);
impl Clone for Hasher { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
