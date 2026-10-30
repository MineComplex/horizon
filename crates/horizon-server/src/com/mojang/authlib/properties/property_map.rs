//! Port of `com.mojang.authlib.properties.PropertyMap` (a Guava `ForwardingMultimap<String, Property>`).
//! A cheap-to-clone handle whose clones share the multimap, as Java's references do. authlib's own maps
//! are immutable (`ImmutableMultimap`: mutators panic with UnsupportedOperationException); Paper's
//! `MutablePropertyMap` (a `HashMultimap`) is `PropertyMap::new_mutable()`.
use crate::prelude::*;
use crate::com::google::gson::JsonArray;
use crate::com::google::gson::JsonElement;
use crate::com::google::gson::JsonObject;
use crate::com::mojang::authlib::properties::Property;

#[derive(Clone)]
pub struct PropertyMap {
    pub properties: Arc<Mut<IndexMap<String, Vec<Property>>>>,
    pub mutable: bool,
}

pub static EMPTY: LazyLock<PropertyMap> = LazyLock::new(|| PropertyMap::new(IndexMap::new()));

impl PropertyMap {
    /// `new PropertyMap(multimap)`: an immutable copy.
    pub fn new(properties: IndexMap<String, Vec<Property>>) -> Self {
        let properties = properties.into_iter().filter(|(_, v)| !v.is_empty()).collect();
        PropertyMap { properties: Arc::new(Mut::new(properties)), mutable: false }
    }

    /// Paper's `MutablePropertyMap`: an empty `HashMultimap` (set semantics per key).
    pub fn new_mutable() -> Self {
        PropertyMap { properties: Arc::new(Mut::new(IndexMap::new())), mutable: true }
    }

    fn check_mutable(&self) {
        if !self.mutable {
            panic!("java.lang.UnsupportedOperationException");
        }
    }

    pub fn delegate(&self) -> IndexMap<String, Vec<Property>> {
        self.properties.get()
    }

    pub fn size(&self) -> i32 {
        self.properties.borrow().values().map(|v| v.len() as i32).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.properties.borrow().is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.properties.borrow().contains_key(key)
    }

    pub fn contains_value(&self, value: &Property) -> bool {
        self.properties.borrow().values().any(|v| v.contains(value))
    }

    pub fn contains_entry(&self, key: &str, value: &Property) -> bool {
        self.properties.borrow().get(key).is_some_and(|v| v.contains(value))
    }

    pub fn get(&self, key: &str) -> Vec<Property> {
        self.properties.borrow().get(key).cloned().unwrap_or_default()
    }

    pub fn key_set(&self) -> IndexSet<String> {
        self.properties.borrow().keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<Property> {
        self.properties.borrow().values().flatten().cloned().collect()
    }

    pub fn entries(&self) -> Vec<(String, Property)> {
        let map = self.properties.borrow();
        map.iter().flat_map(|(k, v)| v.iter().map(move |p| (k.clone(), p.clone()))).collect()
    }

    pub fn as_map(&self) -> IndexMap<String, Vec<Property>> {
        self.properties.get()
    }

    pub fn for_each(&self, mut action: impl FnMut(String, Property)) {
        for (k, v) in self.entries() {
            action(k, v);
        }
    }

    pub fn put(&self, key: &str, value: Property) -> bool {
        self.check_mutable();
        let map = self.properties.borrow_mut();
        let values = map.entry(key.to_string()).or_default();
        if values.contains(&value) {
            return false;
        }
        values.push(value);
        true
    }

    /// `putAll(Multimap)`.
    pub fn put_all(&self, other: &PropertyMap) -> bool {
        let mut changed = false;
        for (k, v) in other.entries() {
            changed |= self.put(&k, v);
        }
        changed
    }

    /// `putAll(key, values)`.
    pub fn put_all_2(&self, key: &str, values: &Vec<Property>) -> bool {
        let mut changed = false;
        for v in values {
            changed |= self.put(key, v.clone());
        }
        changed
    }

    pub fn remove(&self, key: &str, value: &Property) -> bool {
        self.check_mutable();
        let map = self.properties.borrow_mut();
        let Some(values) = map.get_mut(key) else { return false };
        let Some(i) = values.iter().position(|p| p == value) else { return false };
        values.remove(i);
        if values.is_empty() {
            map.shift_remove(key);
        }
        true
    }

    pub fn remove_all(&self, key: &str) -> Vec<Property> {
        self.check_mutable();
        self.properties.borrow_mut().shift_remove(key).unwrap_or_default()
    }

    pub fn replace_values(&self, key: &str, values: &Vec<Property>) -> Vec<Property> {
        let old = self.remove_all(key);
        self.put_all_2(key, values);
        old
    }

    pub fn clear(&self) {
        self.check_mutable();
        self.properties.borrow_mut().clear();
    }
}
impl PartialEq for PropertyMap {
    /// `Multimap.equals`: the same keys mapping to the same values.
    fn eq(&self, other: &Self) -> bool {
        let a = self.properties.get();
        let b = other.properties.get();
        a.len() == b.len()
            && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.len() == w.len() && v.iter().all(|p| w.contains(p))))
    }
}
impl Eq for PropertyMap {}
impl std::hash::Hash for PropertyMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::{BuildHasher, Hasher};
        // order-independent, as `asMap().hashCode()`
        let build = std::hash::BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default();
        let mut sum: u64 = 0;
        for (k, v) in self.entries() {
            let mut h = build.build_hasher();
            k.hash(&mut h);
            v.hash(&mut h);
            sum = sum.wrapping_add(h.finish());
        }
        state.write_u64(sum);
    }
}
impl std::fmt::Debug for PropertyMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}
impl std::fmt::Display for PropertyMap {
    /// `Multimap.toString()`: `{key=[values], ...}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{")?;
        for (i, (k, v)) in self.properties.get().iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{k}=[")?;
            for (j, p) in v.iter().enumerate() {
                if j > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{p}")?;
            }
            write!(f, "]")?;
        }
        write!(f, "}}")
    }
}
crate::jobject!(PropertyMap);

/// `PropertyMap.Serializer` (Gson's adapter): `[{name, value, signature?}...]`, or the legacy
/// `{name: [value...]}` form when reading.
pub struct PropertyMapSerializer;
impl PropertyMapSerializer {
    pub fn deserialize(json: &JsonElement) -> PropertyMap {
        let mut builder: IndexMap<String, Vec<Property>> = IndexMap::new();
        if json.is_json_object() {
            let object = json.get_as_json_object();
            for (key, value) in object.entry_set() {
                if value.is_json_array() {
                    for element in value.get_as_json_array().iter() {
                        builder.entry(key.clone()).or_default().push(Property::new(&key, &element.get_as_string()));
                    }
                }
            }
        } else if json.is_json_array() {
            for element in json.get_as_json_array().iter() {
                if element.is_json_object() {
                    let object = element.get_as_json_object();
                    let name = object.get_as_json_primitive("name").get_as_string();
                    let value = object.get_as_json_primitive("value").get_as_string();
                    if object.has("signature") {
                        let signature = object.get_as_json_primitive("signature").get_as_string();
                        builder.entry(name.clone()).or_default().push(Property::new_3(&name, &value, Some(signature)));
                    } else {
                        builder.entry(name.clone()).or_default().push(Property::new(&name, &value));
                    }
                }
            }
        }
        PropertyMap::new(builder)
    }

    pub fn serialize(src: &PropertyMap) -> JsonElement {
        let result = JsonArray::new();
        for property in src.values() {
            let object = JsonObject::new();
            object.add_property("name", &property.name);
            object.add_property("value", &property.value);
            if let Some(signature) = &property.signature {
                object.add_property("signature", signature);
            }
            result.add(object.into());
        }
        result.into()
    }
}
