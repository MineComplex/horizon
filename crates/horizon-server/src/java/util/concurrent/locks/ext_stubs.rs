//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Lock(pub std::marker::PhantomData<()>);
impl Clone for Lock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ReadWriteLock(pub std::marker::PhantomData<()>);
impl Clone for ReadWriteLock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ReentrantLock(pub std::marker::PhantomData<()>);
impl Clone for ReentrantLock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct ReentrantReadWriteLock(pub std::marker::PhantomData<()>);
impl Clone for ReentrantReadWriteLock { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
