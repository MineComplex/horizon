//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait SuggestionProvider<A0 = JObjectRef>: Send + Sync {}
pub struct Suggestions(pub std::marker::PhantomData<()>);
impl Clone for Suggestions { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SuggestionsBuilder(pub std::marker::PhantomData<()>);
impl Clone for SuggestionsBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
