//! Sulfur cubes on the game side (`SulfurCube.mobInteract`, `pickUpItem`,
//! `remove`): feeding babies, swallowing a held block, shearing it back
//! out, lighting an explosive one, swallowing blocks from the ground and
//! splitting into two babies.
use super::Game;
use horizon_client::{
    mesher::Vertex,
    mobs::{Mob, sulfur_cube}};

fn block_transform(base: glam::Mat4, baby: bool) -> glam::Mat4 {
    base * glam::Mat4::from_scale(glam::Vec3::new(1.0, -1.0, -1.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(if baby { 0.5 } else { 1.0 }))
        * glam::Mat4::from_translation(glam::Vec3::new(-0.5, -0.518, -0.5))
}

fn block_overlay(fuse_ticks: i32, alpha: f32, hurt: bool) -> u32 {
    let fuse = if fuse_ticks >= 0 {
        fuse_ticks as f32 - alpha + 1.0
    } else {
        0.0
    };
    if fuse > 0.0 && (fuse / 5.0) as i32 % 2 == 0 {
        horizon_client::overlay::living(1.0, false)
    } else {
        horizon_client::overlay::living(0.0, hurt)
    }
}

fn block_light(light: u32, emission: u8) -> u32 {
    let block = (light >> 8 & 0xFF).max(u32::from(emission) * 16);
    (light & 0xFF) | block << 8
}

impl Game<'_> {
    /// The swallowed block replaces the opaque core and precedes the shell.
    pub(super) fn sulfur_cube_body_geometry(
        &self,
        mob: &Mob,
        alpha: f32,
        base: glam::Mat4,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        let Some(item) = mob.extra.head.as_deref() else {
            return;
        };
        if super::armor_stands::skull_kind(item).is_some() {
            return;
        }
        // shortcut: the body slot retains only an item id; preserve the stack
        // before supporting its block-state components here.
        let Some(state) = self.res.items().get(item).and_then(|item| item.block) else {
            return;
        };
        let start = out.len();
        self.block_model_vertices(
            state,
            42,
            block_transform(base, mob.is_baby()),
            block_light(light, self.res.facts().get(state).emission),
            true,
            out,
        );
        let overlay = block_overlay(
            sulfur_cube::fuse(mob),
            alpha,
            mob.hurt_time > 0 || mob.death_time > 0,
        );
        if overlay != 0 {
            for vertex in &mut out[start..] {
                vertex.layer = overlay;
            }
        }
    }
}
impl crate::game::Sim<'_> {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swallowed_block_uses_vanilla_lift_and_baby_scale() {
        let base = glam::Mat4::from_translation(glam::Vec3::new(3.0, 4.0, -2.0))
            * glam::Mat4::from_rotation_y(0.7)
            * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
        for baby in [false, true] {
            let size = if baby { 0.5 } else { 1.0 };
            let block = block_transform(base, baby);
            for (point, expected) in [
                (glam::Vec3::ZERO, glam::Vec3::new(-0.5, 0.518, 0.5)),
                (glam::Vec3::ONE, glam::Vec3::new(0.5, -0.482, -0.5)),
            ] {
                let actual = block.transform_point3(point);
                let expected = base.transform_point3(expected * size);
                assert!(actual.abs_diff_eq(expected, 1e-5), "baby={baby}");
            }
            assert!(block.transform_vector3(glam::Vec3::Y).y > 0.0);
        }
    }

    #[test]
    fn swallowed_tnt_flashes_white_over_hurt_using_the_interpolated_fuse() {
        let white = horizon_client::overlay::WHITE + 15;
        assert_eq!(block_overlay(-1, 0.0, false), 0);
        assert_eq!(block_overlay(-1, 0.0, true), 1);
        assert_eq!(block_overlay(9, 0.0, true), white);
        assert_eq!(block_overlay(9, 0.5, false), 0);
        assert_eq!(block_overlay(9, 0.5, true), 1);
        assert_eq!(block_overlay(0, 0.5, false), white);
    }

    #[test]
    fn contained_block_emission_preserves_brighter_light_and_the_sky_channel() {
        assert_eq!(block_light(0x2050, 1), 0x2050);
        assert_eq!(block_light(0x2050, 7), 0x7050);
        assert_eq!(block_light(0x0070, 0), 0x0070);
    }
}
