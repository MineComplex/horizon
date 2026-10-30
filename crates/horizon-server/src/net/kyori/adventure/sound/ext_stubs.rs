//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Sound: Send + Sync {}
pub trait SoundEmitter: Send + Sync {}
pub struct SoundSource(pub std::marker::PhantomData<()>);
impl Clone for SoundSource { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait SoundType: Send + Sync {}
