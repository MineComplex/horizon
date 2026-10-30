//! Port of `net.kyori.adventure.text.event.DataComponentValue` (with `TagSerializable`, `Removed`).
//!
//! `DataComponentValue` is an interface the platform implements (`PaperAdventure.DataComponentValueImpl`,
//! `GsonDataComponentValue`...); here it is a value handle around the implementing object
//! (`value.0`), compared by identity except for `Removed`, which is a singleton.
use crate::prelude::*;
use crate::net::kyori::adventure::nbt::api::BinaryTagHolder;

// ---- interface net.kyori.adventure.text.event.DataComponentValue
#[derive(Clone)]
pub struct DataComponentValue(pub JObjectRef);
impl DataComponentValue {
    pub fn of<T: JObject>(value: JRef<T>) -> DataComponentValue {
        DataComponentValue(value)
    }
    /// `DataComponentValue.removed()`.
    pub fn removed() -> DataComponentValue {
        static REMOVED: LazyLock<JObjectRef> = LazyLock::new(|| Arc::new(DataComponentValueRemoved));
        DataComponentValue(REMOVED.clone())
    }
    /// `instanceof DataComponentValue.Removed`.
    pub fn is_removed(&self) -> bool {
        instance_of::<DataComponentValueRemoved, _>(&self.0)
    }
    /// `(T) value` / `instanceof T`.
    pub fn cast<T: ?Sized + 'static>(&self) -> Option<JRef<T>> {
        cast::<T, _>(&self.0)
    }
}
impl PartialEq for DataComponentValue {
    fn eq(&self, other: &Self) -> bool {
        same(&self.0, &other.0)
    }
}
impl Eq for DataComponentValue {}
impl std::fmt::Debug for DataComponentValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_removed() { f.write_str("DataComponentValue.Removed") } else { f.write_str("DataComponentValue") }
    }
}

// ---- interface net.kyori.adventure.text.event.DataComponentValue.TagSerializable
pub trait DataComponentValueTagSerializable: JObject {
    fn as_binary_tag(&self) -> BinaryTagHolder;
}

// ---- interface net.kyori.adventure.text.event.DataComponentValue.Removed (RemovedDataComponentValueImpl)
pub struct DataComponentValueRemoved;
crate::jobject!(DataComponentValueRemoved);
