//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ListType(pub std::marker::PhantomData<()>);
impl Clone for ListType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MapType(pub std::marker::PhantomData<()>);
impl Clone for MapType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ObjectType(pub std::marker::PhantomData<()>);
impl Clone for ObjectType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TypeUtil<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for TypeUtil<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
