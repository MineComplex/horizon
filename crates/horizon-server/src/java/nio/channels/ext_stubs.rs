//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct FileChannel(pub std::marker::PhantomData<()>);
impl Clone for FileChannel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileLock(pub std::marker::PhantomData<()>);
impl Clone for FileLock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct GatheringByteChannel(pub std::marker::PhantomData<()>);
impl Clone for GatheringByteChannel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ScatteringByteChannel(pub std::marker::PhantomData<()>);
impl Clone for ScatteringByteChannel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct SeekableByteChannel(pub std::marker::PhantomData<()>);
impl Clone for SeekableByteChannel { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
