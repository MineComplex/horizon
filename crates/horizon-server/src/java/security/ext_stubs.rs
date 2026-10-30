//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Key(pub std::marker::PhantomData<()>);
impl Clone for Key { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct KeyPair(pub std::marker::PhantomData<()>);
impl Clone for KeyPair { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PrivateKey(pub std::marker::PhantomData<()>);
impl Clone for PrivateKey { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PublicKey(pub std::marker::PhantomData<()>);
impl Clone for PublicKey { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SecureRandom(pub std::marker::PhantomData<()>);
impl Clone for SecureRandom { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Signature(pub std::marker::PhantomData<()>);
impl Clone for Signature { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
