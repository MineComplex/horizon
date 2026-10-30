//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Color(pub std::marker::PhantomData<()>);
impl Clone for Color { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Image(pub std::marker::PhantomData<()>);
impl Clone for Image { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
