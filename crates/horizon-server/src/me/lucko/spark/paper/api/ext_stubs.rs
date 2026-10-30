//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub struct Compatibility(pub std::marker::PhantomData<()>);
impl Clone for Compatibility { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PaperClassLookup(pub std::marker::PhantomData<()>);
impl Clone for PaperClassLookup { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PaperScheduler(pub std::marker::PhantomData<()>);
impl Clone for PaperScheduler { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
pub struct PaperSparkModule(pub std::marker::PhantomData<()>);
impl Clone for PaperSparkModule { fn clone(&self) -> Self { Self(std::marker::PhantomData) } }
