//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct CompoundListCompoundListType<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for CompoundListCompoundListType<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct HookHookFunction(pub std::marker::PhantomData<()>);
impl Clone for HookHookFunction { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ListListType<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for ListListType<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TaggedChoiceTaggedChoiceType<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for TaggedChoiceTaggedChoiceType<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TypeTemplate(pub std::marker::PhantomData<()>);
impl Clone for TypeTemplate { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
