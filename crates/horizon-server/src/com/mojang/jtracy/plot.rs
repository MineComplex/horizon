//! Port of `com.mojang.jtracy.Plot`: a no-op without Tracy's natives.
#[derive(Clone, Debug)]
pub struct Plot {
    pub name: String,
}
impl Plot {
    pub fn new(name: &str) -> Self {
        Plot { name: name.to_string() }
    }
    pub fn set_value(&self, value: f64) {
        let _ = value;
    }
}
