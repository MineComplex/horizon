//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ChatColor(pub std::marker::PhantomData<()>);
impl Clone for ChatColor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChatMessageType(pub std::marker::PhantomData<()>);
impl Clone for ChatMessageType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
