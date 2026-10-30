//! Port of `net.kyori.adventure.key.Key` (adventure-key).
//!
//! `Key` is an interface (Paper's `NamespacedKey` implements it too), so keys are `JRef<dyn Key>`.
//! Equality and hashing go by namespace and value, as `KeyImpl.equals` does, so `JRef<dyn Key>` works
//! as a map key whatever class made it. Static methods are free functions of this module
//! (`key::key("minecraft:stone")`, `key::key_2("paper", "x")`).
use crate::prelude::*;
use crate::net::kyori::adventure::key::key_impl::{self, KeyImpl};
use crate::net::kyori::adventure::key::{Keyed, Namespaced};

pub const MINECRAFT_NAMESPACE: &str = "minecraft";
pub const DEFAULT_SEPARATOR: char = ':';

// ---- interface net.kyori.adventure.key.Key
pub trait Key: Namespaced + Keyed + JObject {
    fn value(&self) -> String;
    fn as_string(&self) -> String {
        format!("{}:{}", Namespaced::namespace(self), self.value())
    }
    fn as_minimal_string(&self) -> String {
        if Namespaced::namespace(self) == MINECRAFT_NAMESPACE {
            return self.value();
        }
        self.as_string()
    }
    fn compare_to(&self, that: &dyn Key) -> std::cmp::Ordering {
        compare(self.value(), Namespaced::namespace(self), that)
    }
}

fn compare(value: String, namespace: String, that: &dyn Key) -> std::cmp::Ordering {
    // KeyImpl.COMPARATOR: Comparator.comparing(Key::value).thenComparing(Key::namespace)
    value
        .encode_utf16()
        .cmp(that.value().encode_utf16())
        .then_with(|| namespace.encode_utf16().cmp(Namespaced::namespace(that).encode_utf16()))
}

/// `Key.key(String)`: parses `namespace:value` (the namespace defaults to `minecraft`).
pub fn key(string: &str) -> JRef<dyn Key> {
    key_2_separator(string, DEFAULT_SEPARATOR)
}
/// `Key.key(String, char)`.
pub fn key_2_separator(string: &str, character: char) -> JRef<dyn Key> {
    let index = string.find(character);
    let namespace = match index {
        Some(i) if i >= 1 => &string[..i],
        _ => MINECRAFT_NAMESPACE,
    };
    let value = match index {
        Some(i) => &string[i + character.len_utf8()..],
        None => string,
    };
    key_2(namespace, value)
}
/// `Key.key(Namespaced, String)`.
pub fn key_2_namespaced(namespaced: &dyn Namespaced, value: &str) -> JRef<dyn Key> {
    key_2(&namespaced.namespace(), value)
}
/// `Key.key(String namespace, String value)`.
pub fn key_2(namespace: &str, value: &str) -> JRef<dyn Key> {
    KeyImpl::new(namespace, value)
}
/// `Key.comparator()`.
pub fn comparator() -> impl Fn(&JRef<dyn Key>, &JRef<dyn Key>) -> std::cmp::Ordering {
    |a, b| a.compare_to(&**b)
}
pub fn parseable(string: Option<&str>) -> bool {
    let Some(string) = string else { return false };
    let index = string.find(DEFAULT_SEPARATOR);
    let namespace = match index {
        Some(i) if i >= 1 => &string[..i],
        _ => MINECRAFT_NAMESPACE,
    };
    let value = match index {
        Some(i) => &string[i + 1..],
        None => string,
    };
    parseable_namespace(namespace) && parseable_value(value)
}
pub fn parseable_namespace(namespace: &str) -> bool {
    check_namespace(namespace).is_none()
}
/// `Key.checkNamespace`: the index of the first character not allowed, if any.
pub fn check_namespace(namespace: &str) -> Option<i32> {
    for (i, c) in namespace.chars().enumerate() {
        if !allowed_in_namespace(c) {
            return Some(i as i32);
        }
    }
    None
}
pub fn parseable_value(value: &str) -> bool {
    check_value(value).is_none()
}
pub fn check_value(value: &str) -> Option<i32> {
    for (i, c) in value.chars().enumerate() {
        if !allowed_in_value(c) {
            return Some(i as i32);
        }
    }
    None
}
pub fn allowed_in_namespace(character: char) -> bool {
    key_impl::allowed_in_namespace(character)
}
pub fn allowed_in_value(character: char) -> bool {
    key_impl::allowed_in_value(character)
}

/// `Key.equals` across implementations (namespace and value).
pub fn equals(a: &dyn Key, b: &dyn Key) -> bool {
    Namespaced::namespace(a) == Namespaced::namespace(b) && a.value() == b.value()
}
/// `KeyImpl.hashCode()` (Java's: `31 * namespace.hashCode() + value.hashCode()`).
pub fn hash_code(k: &dyn Key) -> i32 {
    key_impl::java_string_hash(&Namespaced::namespace(k)).wrapping_mul(31).wrapping_add(key_impl::java_string_hash(&k.value()))
}

impl PartialEq for dyn Key {
    fn eq(&self, other: &Self) -> bool {
        equals(self, other)
    }
}
impl Eq for dyn Key {}
impl std::hash::Hash for dyn Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Namespaced::namespace(self).hash(state);
        self.value().hash(state);
    }
}
impl PartialOrd for dyn Key {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.compare_to(other))
    }
}
impl Ord for dyn Key {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.compare_to(other)
    }
}
impl std::fmt::Display for dyn Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_string())
    }
}
impl std::fmt::Debug for dyn Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_string())
    }
}
