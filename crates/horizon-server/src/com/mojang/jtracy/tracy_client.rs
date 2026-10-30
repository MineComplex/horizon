//! Port of `com.mojang.jtracy.TracyClient` without its natives (see the package doc).
use crate::com::mojang::jtracy::{ContinuousFrame, DiscontinuousFrame, Plot, Zone};

pub struct TracyClient;
impl TracyClient {
    pub fn is_available() -> bool {
        false
    }

    /// `load()`: the native library does not exist in this port.
    pub fn load() {}

    pub fn mark_frame() {}

    pub fn begin_zone(name: &str, capture_source: bool) -> Zone {
        let _ = (name, capture_source);
        Zone::new()
    }

    pub fn begin_zone_4(name: &str, function: &str, file: &str, line: i32) -> Zone {
        let _ = (name, function, file, line);
        Zone::new()
    }

    pub fn set_thread_name(name: &str, group: i32) {
        let _ = (name, group);
    }

    pub fn create_plot(name: &str) -> Plot {
        Plot::new(name)
    }

    pub fn create_discontinuous_frame(name: &str) -> DiscontinuousFrame {
        DiscontinuousFrame::new(name)
    }

    pub fn create_continuous_frame(name: &str) -> ContinuousFrame {
        ContinuousFrame::new(name)
    }

    pub fn report_app_info(text: &str) {
        let _ = text;
    }

    pub fn message(text: &str) {
        let _ = text;
    }

    pub fn message_2(text: &str, color: i32) {
        let _ = (text, color);
    }
}
