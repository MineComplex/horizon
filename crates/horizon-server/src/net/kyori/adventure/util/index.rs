//! Port of `net.kyori.adventure.util.Index`: a bi-directional map whose keys and values are unique.
//! Both directions keep insertion order (Java's `EnumMap`s iterate in constant order).
use crate::prelude::*;
use std::hash::Hash;

// ---- class net.kyori.adventure.util.Index
#[derive(Clone)]
pub struct Index<K, V> {
    key_to_value: IndexMap<K, V>,
    value_to_key: Vec<(V, K)>,
}
impl<K: Clone + Eq + Hash, V: Clone + PartialEq> Index<K, V> {
    /// `Index.create(Class<V>, Function<V, K>)` / `create(Function, V...)` / `create(Function, List)`:
    /// the values (enum constants in declaration order) and the key function.
    pub fn create(values: Vec<V>, key_function: impl Fn(&V) -> K) -> Index<K, V> {
        let mut key_to_value = IndexMap::new();
        let mut value_to_key = Vec::with_capacity(values.len());
        for value in values {
            let key = key_function(&value);
            if key_to_value.contains_key(&key) {
                panic!("java.lang.IllegalStateException: Key already mapped");
            }
            if value_to_key.iter().any(|(v, _)| *v == value) {
                panic!("java.lang.IllegalStateException: Value already mapped");
            }
            key_to_value.insert(key.clone(), value.clone());
            value_to_key.push((value, key));
        }
        Index { key_to_value, value_to_key }
    }
    pub fn keys(&self) -> IndexSet<K> {
        self.key_to_value.keys().cloned().collect()
    }
    pub fn key(&self, value: &V) -> Option<K> {
        self.value_to_key.iter().find(|(v, _)| v == value).map(|(_, k)| k.clone())
    }
    pub fn key_or_throw(&self, value: &V) -> K {
        match self.key(value) {
            Some(k) => k,
            None => panic!("java.util.NoSuchElementException: There is no key for value"),
        }
    }
    pub fn key_or(&self, value: &V, default_key: K) -> K {
        self.key(value).unwrap_or(default_key)
    }
    pub fn values(&self) -> Vec<V> {
        self.value_to_key.iter().map(|(v, _)| v.clone()).collect()
    }
    pub fn value(&self, key: &K) -> Option<V> {
        self.key_to_value.get(key).cloned()
    }
    pub fn value_or_throw(&self, key: &K) -> V {
        match self.value(key) {
            Some(v) => v,
            None => panic!("java.util.NoSuchElementException: There is no value for key"),
        }
    }
    pub fn value_or(&self, key: &K, default_value: V) -> V {
        self.value(key).unwrap_or(default_value)
    }
    pub fn key_to_value(&self) -> IndexMap<K, V> {
        self.key_to_value.clone()
    }
    pub fn value_to_key(&self) -> Vec<(V, K)> {
        self.value_to_key.clone()
    }
}
impl<V: Clone + PartialEq> Index<String, V> {
    /// `value(String)` with a `&str` key.
    pub fn value_str(&self, key: &str) -> Option<V> {
        self.key_to_value.get(key).cloned()
    }
}
