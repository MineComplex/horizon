//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Key: Send + Sync {}
pub trait Keyed: Send + Sync {}
pub trait Namespaced: Send + Sync {}
