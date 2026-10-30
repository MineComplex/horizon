//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait ConfigurationTransformation: Send + Sync {}
pub struct ConfigurationTransformationBuilder(pub std::marker::PhantomData<()>);
impl Clone for ConfigurationTransformationBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConfigurationTransformationVersioned(pub std::marker::PhantomData<()>);
impl Clone for ConfigurationTransformationVersioned { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConfigurationTransformationVersionedBuilder(pub std::marker::PhantomData<()>);
impl Clone for ConfigurationTransformationVersionedBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
