//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait BiConsumer<A0 = JObjectRef, A1 = JObjectRef>: Send + Sync {}
pub trait BiFunction<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>: Send + Sync {}
pub trait BiPredicate<A0 = JObjectRef, A1 = JObjectRef>: Send + Sync {}
pub trait BinaryOperator<A0 = JObjectRef>: Send + Sync {}
pub trait BooleanSupplier: Send + Sync {}
pub trait Consumer<A0 = JObjectRef>: Send + Sync {}
pub trait DoublePredicate: Send + Sync {}
pub trait DoubleSupplier: Send + Sync {}
pub trait DoubleUnaryOperator: Send + Sync {}
pub trait Function<A0 = JObjectRef, A1 = JObjectRef, A2 = JObjectRef>: Send + Sync {}
pub trait IntConsumer: Send + Sync {}
pub trait IntFunction<A0 = JObjectRef>: Send + Sync {}
pub trait IntPredicate: Send + Sync {}
pub trait IntSupplier: Send + Sync {}
pub trait IntUnaryOperator: Send + Sync {}
pub struct LongConsumer(pub std::marker::PhantomData<()>);
impl Clone for LongConsumer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait LongFunction<A0 = JObjectRef>: Send + Sync {}
pub trait LongPredicate: Send + Sync {}
pub trait LongSupplier: Send + Sync {}
pub trait Predicate<A0 = JObjectRef>: Send + Sync {}
pub trait Supplier<A0 = JObjectRef>: Send + Sync {}
pub trait ToDoubleFunction<A0 = JObjectRef>: Send + Sync {}
pub trait ToIntFunction<A0 = JObjectRef>: Send + Sync {}
pub trait UnaryOperator<A0 = JObjectRef>: Send + Sync {}
