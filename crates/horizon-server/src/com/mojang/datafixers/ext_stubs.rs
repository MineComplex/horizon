//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DSLTypeReference(pub std::marker::PhantomData<()>);
impl Clone for DSLTypeReference { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataFix(pub std::marker::PhantomData<()>);
impl Clone for DataFix { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataFixer(pub std::marker::PhantomData<()>);
impl Clone for DataFixer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct OpticFinder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for OpticFinder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP1<A0 = JObjectRef, A1 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1)>);
impl<A0, A1> Clone for ProductsP1<A0, A1> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP2<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2)>);
impl<A0, A1, A2> Clone for ProductsP2<A0, A1, A2> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP3<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3)>);
impl<A0, A1, A2, A3> Clone for ProductsP3<A0, A1, A2, A3> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP4<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef, A4 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3, fn() -> A4)>);
impl<A0, A1, A2, A3, A4> Clone for ProductsP4<A0, A1, A2, A3, A4> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP5<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef, A4 = JObjectRef, A5 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3, fn() -> A4, fn() -> A5)>);
impl<A0, A1, A2, A3, A4, A5> Clone for ProductsP5<A0, A1, A2, A3, A4, A5> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ProductsP9<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef, A3 = JObjectRef, A4 = JObjectRef, A5 = JObjectRef, A6 = JObjectRef, A7 = JObjectRef, A8 = JObjectRef, A9 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0, fn() -> A1, fn() -> A2, fn() -> A3, fn() -> A4, fn() -> A5, fn() -> A6, fn() -> A7, fn() -> A8, fn() -> A9)>);
impl<A0, A1, A2, A3, A4, A5, A6, A7, A8, A9> Clone for ProductsP9<A0, A1, A2, A3, A4, A5, A6, A7, A8, A9> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct TypeRewriteRule(pub std::marker::PhantomData<()>);
impl Clone for TypeRewriteRule { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Typed<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Typed<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
