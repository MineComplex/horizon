//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct ConfigFile(pub std::marker::PhantomData<()>);
impl Clone for ConfigFile { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ConfigSection(pub std::marker::PhantomData<()>);
impl Clone for ConfigSection { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
