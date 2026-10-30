//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ObjectContents(pub std::marker::PhantomData<()>);
impl Clone for ObjectContents { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ObjectContentsLike: Send + Sync {}
pub struct PlayerHeadObjectContentsBuilder(pub std::marker::PhantomData<()>);
impl Clone for PlayerHeadObjectContentsBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait PlayerHeadObjectContentsSkinSource: Send + Sync {}
