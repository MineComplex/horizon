//! Blocks mobs carry: block items in a hand (`ItemInHandLayer`) and the
//! block an enderman holds (`CarriedBlockLayer`), drawn as shaded cubes like
//! dropped block items. Also the block models mobs wear (`SnowGolemHeadLayer`,
//! `MushroomCowMushroomLayer`).

/// The six faces of the unit cube with their shade: top, bottom, the two
/// z faces, the two x faces.
const FACES: [([[f32; 3]; 4], f32, u8); 6] = [
    (
        [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
        1.0,
        0,
    ),
    (
        [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        0.5,
        0,
    ),
    (
        [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
        0.8,
        1,
    ),
    (
        [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        0.8,
        1,
    ),
    (
        [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        0.6,
        2,
    ),
    (
        [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
        0.6,
        2,
    ),
];
/// The sprites of a cube's top, z sides and x sides, and the top's tint.
struct Cube<'a> {
    sprites: [&'a str; 3],
    tint: u32,
}

/// `CustomHeadLayer.translateToHead` and the banner's `head` display.
fn banner_matrix(head: glam::Mat4) -> glam::Mat4 {
    head * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.25, 0.0))
        * glam::Mat4::from_rotation_y(std::f32::consts::PI)
        * glam::Mat4::from_scale(glam::Vec3::new(0.625, -0.625, -0.625))
        * glam::Mat4::from_translation(glam::Vec3::new(0.0, 1.0, 7.0 / 16.0))
        * glam::Mat4::from_rotation_y(std::f32::consts::PI)
        * glam::Mat4::from_scale(glam::Vec3::splat(1.5))
        * glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(2.0 / 3.0, -2.0 / 3.0, -2.0 / 3.0))
}

/// `ElderGuardianParticle.lifetime`.
pub(super) const APPARITION_TICKS: i32 = 30;

impl crate::game::Sim<'_> {
    /// Count the elder guardian's apparition down.
    pub(super) fn tick_apparition(&mut self) {
        self.creatures.elder_apparition = (self.creatures.elder_apparition - 1).max(0);
    }
}
