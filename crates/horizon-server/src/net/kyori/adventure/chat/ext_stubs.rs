//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ChatType(pub std::marker::PhantomData<()>);
impl Clone for ChatType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ChatTypeBound(pub std::marker::PhantomData<()>);
impl Clone for ChatTypeBound { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait SignedMessage: Send + Sync {}
pub struct SignedMessageSignature(pub std::marker::PhantomData<()>);
impl Clone for SignedMessageSignature { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
