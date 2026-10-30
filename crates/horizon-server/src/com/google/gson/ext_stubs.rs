//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Gson(pub std::marker::PhantomData<()>);
impl Clone for Gson { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JsonArray(pub std::marker::PhantomData<()>);
impl Clone for JsonArray { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JsonDeserializationContext(pub std::marker::PhantomData<()>);
impl Clone for JsonDeserializationContext { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JsonElement(pub std::marker::PhantomData<()>);
impl Clone for JsonElement { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JsonObject(pub std::marker::PhantomData<()>);
impl Clone for JsonObject { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JsonPrimitive(pub std::marker::PhantomData<()>);
impl Clone for JsonPrimitive { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
