//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct BossBar(pub std::marker::PhantomData<()>);
impl Clone for BossBar { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct BossBarColor(pub std::marker::PhantomData<()>);
impl Clone for BossBarColor { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct BossBarFlag(pub std::marker::PhantomData<()>);
impl Clone for BossBarFlag { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait BossBarImplementation: Send + Sync {}
pub struct BossBarOverlay(pub std::marker::PhantomData<()>);
impl Clone for BossBarOverlay { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub trait BossBarViewer: Send + Sync {}
