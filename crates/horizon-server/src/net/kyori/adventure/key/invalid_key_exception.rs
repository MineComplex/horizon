//! Port of `net.kyori.adventure.key.InvalidKeyException` (adventure-key).
//! An unchecked exception: thrown as a panic with this message, as Java throws it.

// ---- class net.kyori.adventure.key.InvalidKeyException
#[derive(Clone, Debug)]
pub struct InvalidKeyException {
    pub key_namespace: String,
    pub key_value: String,
    pub message: Option<String>,
}
impl InvalidKeyException {
    pub fn new(key_namespace: &str, key_value: &str, message: Option<String>) -> Self {
        InvalidKeyException { key_namespace: key_namespace.to_string(), key_value: key_value.to_string(), message }
    }
    pub fn namespace(&self) -> String {
        self.key_namespace.clone()
    }
    pub fn value(&self) -> String {
        self.key_value.clone()
    }
    pub fn get_message(&self) -> Option<String> {
        self.message.clone()
    }
}
impl std::fmt::Display for InvalidKeyException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message.as_deref().unwrap_or(""))
    }
}
