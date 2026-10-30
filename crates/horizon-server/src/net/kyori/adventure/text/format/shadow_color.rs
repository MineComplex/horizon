//! Port of `net.kyori.adventure.text.format.ShadowColor` (and `ShadowColorImpl`): an ARGB value.
use crate::net::kyori::adventure::text::format::{StyleBuilder, StyleBuilderApplicable};
use crate::net::kyori::adventure::util::RGBLike;

pub const NONE_VALUE: i32 = 0;

// ---- interface net.kyori.adventure.text.format.ShadowColor
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ShadowColor {
    pub(crate) value: i32,
}
impl ShadowColor {
    pub fn lerp(t: f32, a: ShadowColor, b: ShadowColor) -> ShadowColor {
        let clamped_t = t.clamp(0.0, 1.0);
        let l = |x: i32, y: i32| (x as f32 + clamped_t * (y - x) as f32).round() as i32;
        ShadowColor::shadow_color_4(l(a.red(), b.red()), l(a.green(), b.green()), l(a.blue(), b.blue()), l(a.alpha(), b.alpha()))
    }
    pub fn none() -> ShadowColor {
        ShadowColor { value: NONE_VALUE }
    }
    /// `ShadowColor.shadowColor(int argb)`.
    pub fn shadow_color(argb: i32) -> ShadowColor {
        ShadowColor { value: argb }
    }
    pub fn shadow_color_4(red: i32, green: i32, blue: i32, alpha: i32) -> ShadowColor {
        ShadowColor { value: (alpha & 0xff) << 24 | (red & 0xff) << 16 | (green & 0xff) << 8 | (blue & 0xff) }
    }
    /// `ShadowColor.shadowColor(RGBLike, int alpha)`.
    pub fn shadow_color_2(rgb: &dyn RGBLike, alpha: i32) -> ShadowColor {
        ShadowColor::shadow_color_4(rgb.red(), rgb.green(), rgb.blue(), alpha)
    }
    pub fn from_hex_string(hex: &str) -> Option<ShadowColor> {
        if hex.len() != 9 || !hex.starts_with('#') {
            return None;
        }
        let p = |r: std::ops::Range<usize>| i32::from_str_radix(&hex[r], 16).ok();
        let (r, g, b, a) = (p(1..3)?, p(3..5)?, p(5..7)?, p(7..9)?);
        Some(ShadowColor { value: (a << 24) | (r << 16) | (g << 8) | b })
    }
    pub fn as_hex_string(&self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.red(), self.green(), self.blue(), self.alpha())
    }
    pub fn red(&self) -> i32 {
        (self.value >> 16) & 0xff
    }
    pub fn green(&self) -> i32 {
        (self.value >> 8) & 0xff
    }
    pub fn blue(&self) -> i32 {
        self.value & 0xff
    }
    pub fn alpha(&self) -> i32 {
        (self.value >> 24) & 0xff
    }
    pub fn value(&self) -> i32 {
        self.value
    }
}
impl RGBLike for ShadowColor {
    fn red(&self) -> i32 {
        ShadowColor::red(self)
    }
    fn green(&self) -> i32 {
        ShadowColor::green(self)
    }
    fn blue(&self) -> i32 {
        ShadowColor::blue(self)
    }
}
impl StyleBuilderApplicable for ShadowColor {
    fn style_apply(&self, style: &StyleBuilder) {
        style.shadow_color(Some(*self));
    }
}
