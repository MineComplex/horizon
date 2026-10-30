//! Port of `com.mojang.authlib.properties.Property` (a record: by value).
use crate::prelude::*;
use crate::java::security::PublicKey;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}
impl Property {
    pub fn new(name: &str, value: &str) -> Self {
        Self::new_3(name, value, None)
    }

    pub fn new_3(name: &str, value: &str, signature: Option<String>) -> Self {
        Property { name: name.to_string(), value: value.to_string(), signature }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn value(&self) -> String {
        self.value.clone()
    }

    pub fn signature(&self) -> Option<String> {
        self.signature.clone()
    }

    pub fn has_signature(&self) -> bool {
        self.signature.is_some()
    }

    /// Deprecated in authlib. SHA1withRSA over the value's ASCII bytes against the base64 signature.
    pub fn is_signature_valid(&self, public_key: PublicKey) -> bool {
        // PORT: java.security.Signature ("SHA1withRSA") from the java.security port; this server has no
        // Mojang keys to check against (see the package doc), so an unverifiable signature is invalid.
        let _ = public_key;
        false
    }
}
impl std::fmt::Display for Property {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Property[name={}, value={}, signature={}]", self.name, self.value, self.signature.as_deref().unwrap_or("null"))
    }
}
crate::jobject!(Property);
