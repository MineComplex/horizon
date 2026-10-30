//! Port of `net.kyori.adventure.util.ARGBLike`.
use crate::net::kyori::adventure::util::RGBLike;

// ---- interface net.kyori.adventure.util.ARGBLike
pub trait ARGBLike: RGBLike {
    fn alpha(&self) -> i32;
}
impl ARGBLike for crate::net::kyori::adventure::text::format::ShadowColor {
    fn alpha(&self) -> i32 {
        crate::net::kyori::adventure::text::format::ShadowColor::alpha(self)
    }
}
