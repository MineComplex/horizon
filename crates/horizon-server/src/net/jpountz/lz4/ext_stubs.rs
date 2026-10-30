//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct LZ4Compressor(pub std::marker::PhantomData<()>);
impl Clone for LZ4Compressor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LZ4FastDecompressor(pub std::marker::PhantomData<()>);
impl Clone for LZ4FastDecompressor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
