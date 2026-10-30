//! Port of `net.kyori.adventure.key.KeyImpl` (adventure-key).
use crate::prelude::*;
use crate::net::kyori::adventure::key::invalid_key_exception::InvalidKeyException;
use crate::net::kyori::adventure::key::key::{self, Key};
use crate::net::kyori::adventure::key::{Keyed, Namespaced};

pub const NAMESPACE_PATTERN: &str = "[a-z0-9_\\-.]+";
pub const VALUE_PATTERN: &str = "[a-z0-9_\\-./]+";

// ---- class net.kyori.adventure.key.KeyImpl
pub struct KeyImpl {
    pub namespace: String,
    pub value: String,
}
impl KeyImpl {
    pub fn new(namespace: &str, value: &str) -> JRef<KeyImpl> {
        check_error("namespace", namespace, namespace, value, key::check_namespace(namespace), NAMESPACE_PATTERN);
        check_error("value", value, namespace, value, key::check_value(value), VALUE_PATTERN);
        Arc::new(KeyImpl { namespace: namespace.to_string(), value: value.to_string() })
    }
}
fn check_error(name: &str, check_part: &str, namespace: &str, value: &str, index: Option<i32>, pattern: &str) {
    if let Some(index_value) = index {
        let character = check_part.chars().nth(index_value as usize).unwrap_or('\0');
        let mut buf = [0u8; 4];
        let bytes: Vec<String> = character.encode_utf8(&mut buf).bytes().map(|b| (b as i8).to_string()).collect();
        let e = InvalidKeyException::new(
            namespace,
            value,
            Some(format!(
                "Non {} character in {} of Key[{}:{}] at index {} ('{}', bytes: [{}])",
                pattern,
                name,
                namespace,
                value,
                index_value,
                character,
                bytes.join(", ")
            )),
        );
        panic!("net.kyori.adventure.key.InvalidKeyException: {}", e);
    }
}
pub fn allowed_in_namespace(character: char) -> bool {
    character == '_' || character == '-' || character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.'
}
pub fn allowed_in_value(character: char) -> bool {
    character == '_'
        || character == '-'
        || character.is_ascii_lowercase()
        || character.is_ascii_digit()
        || character == '.'
        || character == '/'
}
/// Java's `String.hashCode()` (over UTF-16 code units).
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for c in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    h
}
impl Namespaced for KeyImpl {
    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}
impl Keyed for KeyImpl {
    fn key(&self) -> JRef<dyn Key> {
        Arc::new(KeyImpl { namespace: self.namespace.clone(), value: self.value.clone() })
    }
}
impl Key for KeyImpl {
    fn value(&self) -> String {
        self.value.clone()
    }
}
impl std::fmt::Display for KeyImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.value)
    }
}
crate::jobject!(KeyImpl: dyn Key, dyn Keyed, dyn Namespaced);
