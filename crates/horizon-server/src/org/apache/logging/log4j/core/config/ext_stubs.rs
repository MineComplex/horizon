//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AppenderControl(pub std::marker::PhantomData<()>);
impl Clone for AppenderControl { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct AppenderRef(pub std::marker::PhantomData<()>);
impl Clone for AppenderRef { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Configuration(pub std::marker::PhantomData<()>);
impl Clone for Configuration { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Property(pub std::marker::PhantomData<()>);
impl Clone for Property { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
