//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Deflater(pub std::marker::PhantomData<()>);
impl Clone for Deflater { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Inflater(pub std::marker::PhantomData<()>);
impl Clone for Inflater { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ZipEntry(pub std::marker::PhantomData<()>);
impl Clone for ZipEntry { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ZipFile(pub std::marker::PhantomData<()>);
impl Clone for ZipFile { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
