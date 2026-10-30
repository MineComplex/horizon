//! Port of `com.mojang.jtracy.ContinuousFrame`: a no-op without Tracy's natives.
#[derive(Clone, Debug)]
pub struct ContinuousFrame {
    pub name: String,
}
impl ContinuousFrame {
    pub fn new(name: &str) -> Self {
        ContinuousFrame { name: name.to_string() }
    }
    pub fn mark(&self) {}
}
