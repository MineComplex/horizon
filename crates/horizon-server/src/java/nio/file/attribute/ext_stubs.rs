//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BasicFileAttributeView(pub std::marker::PhantomData<()>);
impl Clone for BasicFileAttributeView { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait BasicFileAttributes: Send + Sync {}
pub struct FileAttribute<A0 = JObjectRef>(pub std::marker::PhantomData<(fn() -> A0,)>);
impl<A0> Clone for FileAttribute<A0> { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileAttributeView(pub std::marker::PhantomData<()>);
impl Clone for FileAttributeView { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct FileTime(pub std::marker::PhantomData<()>);
impl Clone for FileTime { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct UserPrincipalLookupService(pub std::marker::PhantomData<()>);
impl Clone for UserPrincipalLookupService { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
