//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Environment(pub std::marker::PhantomData<()>);
impl Clone for Environment { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct GameProfile(pub std::marker::PhantomData<()>);
impl Clone for GameProfile { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait GameProfileRepository: Send + Sync {}
pub trait ProfileLookupCallback: Send + Sync {}
