pub mod inlined_registry_holder_impl;
pub mod paper_registry_holders;
pub mod reference_registry_holder_impl;
pub mod registry_holder;
pub use self::inlined_registry_holder_impl::{InlinedRegistryHolderImpl};
pub use self::paper_registry_holders::{PaperRegistryHolders};
pub use self::reference_registry_holder_impl::{ReferenceRegistryHolderImpl};
pub use self::registry_holder::{RegistryHolder, RegistryHolderReference, RegistryHolderInlined};
