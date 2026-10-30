//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CommentLine(pub std::marker::PhantomData<()>);
impl Clone for CommentLine { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CommentType(pub std::marker::PhantomData<()>);
impl Clone for CommentType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
