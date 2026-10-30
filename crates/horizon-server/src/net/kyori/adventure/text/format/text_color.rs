//! Port of `net.kyori.adventure.text.format.TextColor` (and `TextColorImpl`).
//!
//! `TextColor` is a value (RGB). `NamedTextColor` is a `TextColor` that remembers its name: in Rust both
//! are this one type (`NamedTextColor` is an alias), and `is_named()` replaces `instanceof NamedTextColor`.
//! As in Java, a named color and an unnamed one of the same value are not equal (`TextColor.color(int)`
//! returns the named color when the value is one).
use crate::net::kyori::adventure::text::format::{StyleBuilder, StyleBuilderApplicable};
use crate::net::kyori::adventure::util::{HSVLike, RGBLike};

pub const HEX_CHARACTER: char = '#';
pub const HEX_PREFIX: &str = "#";

// ---- interface net.kyori.adventure.text.format.TextColor
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextColor {
    pub(crate) value: i32,
    /// `Some(name)` for the 16 `NamedTextColor`s.
    pub(crate) name: Option<&'static str>,
}
impl TextColor {
    pub(crate) const fn named(name: &'static str, value: i32) -> TextColor {
        TextColor { value, name: Some(name) }
    }
    /// `TextColor.color(int)`.
    pub fn color(value: i32) -> TextColor {
        let truncated_value = value & 0xffffff;
        match crate::net::kyori::adventure::text::format::named_text_color::named_color(truncated_value) {
            Some(named) => named,
            None => TextColor { value: truncated_value, name: None },
        }
    }
    /// `TextColor.color(RGBLike)`.
    pub fn color_1(rgb: &dyn RGBLike) -> TextColor {
        TextColor::color_3(rgb.red(), rgb.green(), rgb.blue())
    }
    /// `TextColor.color(HSVLike)`.
    pub fn color_1_2(hsv: &HSVLike) -> TextColor {
        let s = hsv.s();
        let v = hsv.v();
        if s == 0.0 {
            return TextColor::color_3_2(v, v, v);
        }
        let h = hsv.h() * 6.0;
        let i = h.floor() as i32;
        let f = h - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        match i {
            0 => TextColor::color_3_2(v, t, p),
            1 => TextColor::color_3_2(q, v, p),
            2 => TextColor::color_3_2(p, v, t),
            3 => TextColor::color_3_2(p, q, v),
            4 => TextColor::color_3_2(t, p, v),
            _ => TextColor::color_3_2(v, p, q),
        }
    }
    /// `TextColor.color(int r, int g, int b)`.
    pub fn color_3(r: i32, g: i32, b: i32) -> TextColor {
        TextColor::color((r & 0xff) << 16 | (g & 0xff) << 8 | (b & 0xff))
    }
    /// `TextColor.color(float r, float g, float b)`.
    pub fn color_3_2(r: f32, g: f32, b: f32) -> TextColor {
        TextColor::color_3((r * 255.0) as i32, (g * 255.0) as i32, (b * 255.0) as i32)
    }
    pub fn from_hex_string(string: &str) -> Option<TextColor> {
        if let Some(rest) = string.strip_prefix(HEX_PREFIX) {
            return i32::from_str_radix(rest, 16).ok().map(TextColor::color);
        }
        None
    }
    pub fn from_css_hex_string(string: &str) -> Option<TextColor> {
        if let Some(hex_string) = string.strip_prefix(HEX_PREFIX) {
            if hex_string.len() != 3 && hex_string.len() != 6 {
                return None;
            }
            let hex = i32::from_str_radix(hex_string, 16).ok()?;
            if hex_string.len() == 6 {
                return Some(TextColor::color(hex));
            }
            let red = (hex & 0xf00) >> 8 | (hex & 0xf00) >> 4;
            let green = (hex & 0x0f0) >> 4 | (hex & 0x0f0);
            let blue = (hex & 0x00f) << 4 | (hex & 0x00f);
            return Some(TextColor::color_3(red, green, blue));
        }
        None
    }
    pub fn value(&self) -> i32 {
        self.value
    }
    pub fn as_hex_string(&self) -> String {
        format!("{}{:06x}", HEX_CHARACTER, self.value)
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
    /// `instanceof NamedTextColor`.
    pub fn is_named(&self) -> bool {
        self.name.is_some()
    }
    /// The `NamedTextColor`'s name (`NamedTextColor.toString()`), when this is one.
    pub fn named_name(&self) -> Option<&'static str> {
        self.name
    }
    pub fn as_hsv(&self) -> HSVLike {
        HSVLike::from_rgb(self.red(), self.green(), self.blue())
    }
    /// `TextColor.lerp(float, RGBLike, RGBLike)`.
    pub fn lerp(t: f32, a: &dyn RGBLike, b: &dyn RGBLike) -> TextColor {
        let clamped_t = t.clamp(0.0, 1.0);
        let ar = a.red();
        let br = b.red();
        let ag = a.green();
        let bg = b.green();
        let ab = a.blue();
        let bb = b.blue();
        TextColor::color_3(
            (ar as f32 + clamped_t * (br - ar) as f32).round() as i32,
            (ag as f32 + clamped_t * (bg - ag) as f32).round() as i32,
            (ab as f32 + clamped_t * (bb - ab) as f32).round() as i32,
        )
    }
    /// `TextColor.nearestColorTo(List, TextColor)`: the closest by HSV distance.
    pub fn nearest_color_to(values: &[TextColor], any: TextColor) -> TextColor {
        if values.is_empty() {
            panic!("java.lang.IllegalArgumentException: values is empty");
        }
        if values.contains(&any) {
            return any;
        }
        let mut match_distance = f32::MAX;
        let mut m = values[0];
        for potential in values {
            let distance = distance(any.as_hsv(), potential.as_hsv());
            if distance < match_distance {
                m = *potential;
                match_distance = distance;
            }
            if distance == 0.0 {
                break;
            }
        }
        m
    }
    pub fn compare_to(&self, that: &TextColor) -> std::cmp::Ordering {
        self.value.cmp(&that.value)
    }
}
/// `TextColor.distance(HSVLike, HSVLike)`.
fn distance(self_: HSVLike, other: HSVLike) -> f32 {
    let h_distance = 3.0 * (self_.h() - other.h()).abs().min(1.0 - (self_.h() - other.h()).abs());
    let s_diff = self_.s() - other.s();
    let v_diff = self_.v() - other.v();
    h_distance * h_distance + s_diff * s_diff + v_diff * v_diff
}
impl RGBLike for TextColor {
    fn red(&self) -> i32 {
        TextColor::red(self)
    }
    fn green(&self) -> i32 {
        TextColor::green(self)
    }
    fn blue(&self) -> i32 {
        TextColor::blue(self)
    }
}
impl StyleBuilderApplicable for TextColor {
    fn style_apply(&self, style: &StyleBuilder) {
        style.color(Some(*self));
    }
}
impl std::fmt::Display for TextColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.name {
            Some(n) => f.write_str(n),
            None => f.write_str(&self.as_hex_string()),
        }
    }
}
impl std::fmt::Debug for TextColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl PartialOrd for TextColor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.compare_to(other))
    }
}
