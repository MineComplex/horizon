//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Cipher(pub std::marker::PhantomData<()>);
impl Clone for Cipher { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SecretKey(pub std::marker::PhantomData<()>);
impl Clone for SecretKey { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
