//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct AccessMode(pub std::marker::PhantomData<()>);
impl Clone for AccessMode { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct CopyOption(pub std::marker::PhantomData<()>);
impl Clone for CopyOption { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DirectoryStream<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DirectoryStream<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct DirectoryStreamFilter<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for DirectoryStreamFilter<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileStore(pub std::marker::PhantomData<()>);
impl Clone for FileStore { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileSystem(pub std::marker::PhantomData<()>);
impl Clone for FileSystem { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileVisitResult(pub std::marker::PhantomData<()>);
impl Clone for FileVisitResult { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct LinkOption(pub std::marker::PhantomData<()>);
impl Clone for LinkOption { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct OpenOption(pub std::marker::PhantomData<()>);
impl Clone for OpenOption { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait PathMatcher: Send + Sync {}
pub struct StandardOpenOption(pub std::marker::PhantomData<()>);
impl Clone for StandardOpenOption { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct WatchEventKind<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for WatchEventKind<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct WatchEventModifier(pub std::marker::PhantomData<()>);
impl Clone for WatchEventModifier { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct WatchKey(pub std::marker::PhantomData<()>);
impl Clone for WatchKey { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct WatchService(pub std::marker::PhantomData<()>);
impl Clone for WatchService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
