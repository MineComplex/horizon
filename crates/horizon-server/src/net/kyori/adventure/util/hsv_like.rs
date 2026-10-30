//! Port of `net.kyori.adventure.util.HSVLike` (and `HSVLikeImpl`): a value.

// ---- interface net.kyori.adventure.util.HSVLike
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HSVLike {
    h: f32,
    s: f32,
    v: f32,
}
impl HSVLike {
    pub fn hsv_like(h: f32, s: f32, v: f32) -> HSVLike {
        if !(0.0..=1.0).contains(&h) || !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&v) {
            panic!("java.lang.IllegalArgumentException: HSV values must be in [0, 1]");
        }
        HSVLike { h, s, v }
    }
    pub fn of(h: f32, s: f32, v: f32) -> HSVLike {
        HSVLike::hsv_like(h, s, v)
    }
    pub fn from_rgb(red: i32, green: i32, blue: i32) -> HSVLike {
        let r = red as f32 / 255.0;
        let g = green as f32 / 255.0;
        let b = blue as f32 / 255.0;
        let min = r.min(g.min(b));
        let max = r.max(g.max(b));
        let delta = max - min;
        let s = if max != 0.0 { delta / max } else { 0.0 };
        if s == 0.0 {
            return HSVLike { h: 0.0, s, v: max };
        }
        let mut h = if r == max {
            (g - b) / delta
        } else if g == max {
            2.0 + (b - r) / delta
        } else {
            4.0 + (r - g) / delta
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
        HSVLike { h: h / 360.0, s, v: max }
    }
    pub fn h(&self) -> f32 {
        self.h
    }
    pub fn s(&self) -> f32 {
        self.s
    }
    pub fn v(&self) -> f32 {
        self.v
    }
}
