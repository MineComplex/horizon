//! Stand-ins for the external library types this package's users name (tools/leaf_port/gen.py).
//! Replace each with its port.
use crate::prelude::*;

pub trait Audience: Send + Sync {}
pub trait ForwardingAudience: Send + Sync {}
pub trait ForwardingAudienceSingle: Send + Sync {}
