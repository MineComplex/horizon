//! Port of `com.mojang.jtracy.DiscontinuousFrame`: a no-op without Tracy's natives.
#[derive(Clone, Debug)]
pub struct DiscontinuousFrame {
    pub name: String,
}
impl DiscontinuousFrame {
    pub fn new(name: &str) -> Self {
        DiscontinuousFrame { name: name.to_string() }
    }
    pub fn start(&self) {}
    pub fn end(&self) {}
}
