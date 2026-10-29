//! Wind charges' rendering (`WindChargeRenderer`). Their bursts are the
//! server's.
use super::Game;
use horizon_client::projectiles::Projectile;

impl Game<'_> {
    /// `WindChargeRenderer`: the spinning charge and its wind.
    pub(super) fn wind_charge_geometry(
        &self,
        p: &Projectile,
        at: glam::Vec3,
        light: u32,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        static MODEL: std::sync::OnceLock<Option<horizon_client::mobs::Part>> =
            std::sync::OnceLock::new();
        let model = MODEL.get_or_init(|| {
            horizon_client::mobs::load_model(&self.options.resources, "minecraft:wind_charge#main")
        });
        let Some(model) = model else { return };
        let Some(r) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/projectiles/wind_charge")
            .copied()
        else {
            return;
        };
        let age = p.age as f32;
        let mut poses = horizon_client::mobs::anim::Poses::new(model);
        poses.yrot("wind_charge", -age * 16.0_f32.to_radians());
        poses.yrot("wind", age * 16.0_f32.to_radians());
        let base = glam::Mat4::from_translation(at)
            * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
        horizon_client::mobs::render::draw_scrolled(
            model,
            base,
            &poses,
            [r.u0, r.v0, r.u1, r.v1],
            horizon_client::mobs::render::scroll_offset(
                age,
                horizon_client::mobs::render::WIND_CHARGE_SCROLL,
            ),
            0xFFFF_FFFF,
            light,
            0,
            out,
        );
    }
}
