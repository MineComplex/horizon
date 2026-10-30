//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct YamlConfigurationLoader(pub std::marker::PhantomData<()>);
impl Clone for YamlConfigurationLoader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct YamlConfigurationLoaderBuilder(pub std::marker::PhantomData<()>);
impl Clone for YamlConfigurationLoaderBuilder { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
