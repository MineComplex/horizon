//! What every ported module imports (`use crate::prelude::*;`).
pub use crate::java::{
    api, as_object, cast, instance_of, same, ById, JGeneric, JObject, JObjectRef, JRef, Monitor, Mut,
};
pub use crate::prelude_traits::*;
pub use indexmap::{IndexMap, IndexSet};
pub use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
pub use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering as AtomicOrdering};
pub use std::sync::{Arc, LazyLock, Weak};
pub use uuid::Uuid;
