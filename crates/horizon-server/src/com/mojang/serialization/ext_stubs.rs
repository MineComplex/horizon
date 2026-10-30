//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Codec<A0 = JObjectRef>: Send + Sync {}
pub struct CodecResultFunction<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for CodecResultFunction<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataResult<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DataResult<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataResultError<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DataResultError<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Decoder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Decoder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DynamicLike<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DynamicLike<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait DynamicOps<A0 = JObjectRef>: Send + Sync {}
pub struct Encoder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for Encoder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Keyable: Send + Sync {}
pub struct Lifecycle(pub std::marker::PhantomData<()>);
impl Clone for Lifecycle { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait ListBuilder<A0 = JObjectRef>: Send + Sync {}
pub struct MapCodec<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for MapCodec<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MapEncoder<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for MapEncoder<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct MapLike<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for MapLike<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct OptionalDynamic<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for OptionalDynamic<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait RecordBuilder<A0 = JObjectRef>: Send + Sync {}
