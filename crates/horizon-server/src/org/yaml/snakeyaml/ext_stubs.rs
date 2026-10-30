//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct DumperOptions(pub std::marker::PhantomData<()>);
impl Clone for DumperOptions { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LoaderOptions(pub std::marker::PhantomData<()>);
impl Clone for LoaderOptions { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Yaml(pub std::marker::PhantomData<()>);
impl Clone for Yaml { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
