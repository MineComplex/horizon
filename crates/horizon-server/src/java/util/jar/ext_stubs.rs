//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct JarEntry(pub std::marker::PhantomData<()>);
impl Clone for JarEntry { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct JarFile(pub std::marker::PhantomData<()>);
impl Clone for JarFile { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Manifest(pub std::marker::PhantomData<()>);
impl Clone for Manifest { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
