//! Port of `net.kyori.adventure.util.RGBLike`.
use crate::net::kyori::adventure::util::HSVLike;

// ---- interface net.kyori.adventure.util.RGBLike
pub trait RGBLike: Send + Sync {
    fn red(&self) -> i32;
    fn green(&self) -> i32;
    fn blue(&self) -> i32;
    fn as_hsv(&self) -> HSVLike {
        HSVLike::from_rgb(self.red(), self.green(), self.blue())
    }
}
