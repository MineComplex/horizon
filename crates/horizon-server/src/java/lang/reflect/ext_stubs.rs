//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AnnotatedElement(pub std::marker::PhantomData<()>);
impl Clone for AnnotatedElement { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct AnnotatedParameterizedType(pub std::marker::PhantomData<()>);
impl Clone for AnnotatedParameterizedType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct AnnotatedType(pub std::marker::PhantomData<()>);
impl Clone for AnnotatedType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Constructor<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Constructor<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Field(pub std::marker::PhantomData<()>);
impl Clone for Field { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Method(pub std::marker::PhantomData<()>);
impl Clone for Method { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ParameterizedType(pub std::marker::PhantomData<()>);
impl Clone for ParameterizedType { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Type(pub std::marker::PhantomData<()>);
impl Clone for Type { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
