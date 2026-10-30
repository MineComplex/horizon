//! Port of `com.mojang.jtracy.Zone` (AutoCloseable): a no-op without Tracy's natives.
#[derive(Clone, Debug, Default)]
pub struct Zone;
impl Zone {
    pub fn new() -> Self {
        Zone
    }
    pub fn add_text(&self, text: &str) -> Zone {
        let _ = text;
        self.clone()
    }
    pub fn set_color(&self, color: i32) -> Zone {
        let _ = color;
        self.clone()
    }
    pub fn add_value(&self, value: i64) -> Zone {
        let _ = value;
        self.clone()
    }
    pub fn close(&self) {}
}
