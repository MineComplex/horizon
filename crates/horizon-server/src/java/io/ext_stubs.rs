//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BufferedReader(pub std::marker::PhantomData<()>);
impl Clone for BufferedReader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ByteArrayInputStream(pub std::marker::PhantomData<()>);
impl Clone for ByteArrayInputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Closeable: Send + Sync {}
pub struct DataInput(pub std::marker::PhantomData<()>);
impl Clone for DataInput { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DataInputStream(pub std::marker::PhantomData<()>);
impl Clone for DataInputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait DataOutput: Send + Sync {}
pub struct DataOutputStream(pub std::marker::PhantomData<()>);
impl Clone for DataOutputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FilterInputStream(pub std::marker::PhantomData<()>);
impl Clone for FilterInputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct InputStream(pub std::marker::PhantomData<()>);
impl Clone for InputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ObjectInputStream(pub std::marker::PhantomData<()>);
impl Clone for ObjectInputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ObjectOutputStream(pub std::marker::PhantomData<()>);
impl Clone for ObjectOutputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct OutputStream(pub std::marker::PhantomData<()>);
impl Clone for OutputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PrintStream(pub std::marker::PhantomData<()>);
impl Clone for PrintStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PrintWriter(pub std::marker::PhantomData<()>);
impl Clone for PrintWriter { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PushbackInputStream(pub std::marker::PhantomData<()>);
impl Clone for PushbackInputStream { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct Reader(pub std::marker::PhantomData<()>);
impl Clone for Reader { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait Serializable: Send + Sync {}
pub struct Writer(pub std::marker::PhantomData<()>);
impl Clone for Writer { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
