//! A player's blow at a ghast's fireball or a wind charge
//! (`#minecraft:redirectable_projectile`): `Player.attack` turns it with
//! `ProjectileDeflection.AIM_DEFLECT` instead of hurting anything. It flies on
//! along the player's view and is the player's now (`Projectile.deflect`
//! sets the owner), so a ghast it meets dies of it (`Ghast.isReflectedFireball`).
use super::{ray_box, view_vector};
use horizon_client::projectiles::{Projectile, ProjectileKind};
use horizon_core::geometry::{Aabb, Vec3};

/// `AbstractHurtingProjectile.getPickRadius`: the crosshair finds it a block
/// round its box.
const PICK_RADIUS: f64 = 1.0;

/// `#minecraft:redirectable_projectile`: a ghast's fireball and both wind charges.
pub(super) fn redirectable(p: &Projectile) -> bool {
    !p.stuck
        && matches!(
            p.kind,
            ProjectileKind::LargeFireball { .. }
                | ProjectileKind::WindCharge
                | ProjectileKind::BreezeWindCharge
        )
}

/// Its box for the crosshair: the entity's (a fireball 1 x 1, a wind charge
/// 0.3125, from its feet up) grown by the pick radius.
fn pick_box(p: &Projectile) -> Aabb {
    let size = match p.kind {
        ProjectileKind::LargeFireball { .. } => 1.0,
        _ => 0.3125,
    };
    let half = size / 2.0 + PICK_RADIUS;
    Aabb::new(
        Vec3::new(
            p.position.x - half,
            p.position.y - PICK_RADIUS,
            p.position.z - half,
        ),
        Vec3::new(
            p.position.x + half,
            p.position.y + size + PICK_RADIUS,
            p.position.z + half,
        ),
    )
}

impl crate::game::Game<'_> {
    /// The redirectable projectile under the crosshair within the attack
    /// reach, and how far along the view it is.
    pub(super) fn redirectable_hit(&self) -> Option<(usize, f64)> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        self.creatures
            .projectiles
            .iter()
            .enumerate()
            .filter(|(_, p)| redirectable(p))
            .filter_map(|(i, p)| ray_box(eye, dir, reach, pick_box(p)).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ghast_fireballs_and_wind_charges_are_redirectable() {
        let at = Vec3::new(0.0, 64.0, 0.0);
        let large = Projectile::fireball(
            ProjectileKind::LargeFireball { power: 1 },
            at,
            None,
            [1.0, 0.0, 0.0],
        );
        let small = Projectile::fireball(ProjectileKind::SmallFireball, at, None, [1.0, 0.0, 0.0]);
        assert!(redirectable(&large));
        assert!(!redirectable(&small));
        // The pick box: a block round the fireball's 1 x 1 box.
        let b = pick_box(&large);
        assert_eq!(b.min_position().x, -1.5);
        assert_eq!(b.max_position().y, 66.0);
    }
}
