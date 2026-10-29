//! The wither on the game side: building it (`WitherSkullBlock.checkSpawn`),
//! its boss bar, and its skulls hitting and exploding.
use super::Game;
use horizon_client::{
    
    projectiles::{Projectile, ProjectileKind}};

impl crate::game::Sim<'_> {
    fn is_wither_skull(&self, p: [i32; 3]) -> bool {
        let name = &*self
            .res
            .facts()
            .get(self.world.block(p[0], p[1], p[2]))
            .name;
        matches!(
            name,
            "minecraft:wither_skeleton_skull" | "minecraft:wither_skeleton_wall_skull"
        )
    }
    fn is_soul_base(&self, p: [i32; 3]) -> bool {
        let name = &*self
            .res
            .facts()
            .get(self.world.block(p[0], p[1], p[2]))
            .name;
        matches!(name, "minecraft:soul_sand" | "minecraft:soul_soil")
    }
    /// After a wither skeleton skull is placed: the T of soul sand (or soul
    /// soil) under three skulls, upright along X or Z, becomes a wither.
    pub(super) fn check_wither_spawn(&mut self, placed: [i32; 3]) -> horizon_client::Result<()> {
        // Nothing is built in Peaceful (`level.getDifficulty() != PEACEFUL`).
        if self.world_data.difficulty == 0 || !self.is_wither_skull(placed) {
            return Ok(());
        }
        // Pattern rows top to bottom: "^^^", "###", "~#~" ('~' air).
        for (ax, az) in [(1, 0), (0, 1)] {
            for col in 0..3 {
                let left = [placed[0] - ax * col, placed[1], placed[2] - az * col];
                let at = |c: i32, row: i32| [left[0] + ax * c, left[1] - row, left[2] + az * c];
                let ok = (0..3).all(|c| self.is_wither_skull(at(c, 0)))
                    && (0..3).all(|c| self.is_soul_base(at(c, 1)))
                    && self.is_soul_base(at(1, 2))
                    && [0, 2].iter().all(|&c| {
                        let p = at(c, 2);
                        self.res.facts().is_air(self.world.block(p[0], p[1], p[2]))
                    });
                if !ok {
                    continue;
                }
                // WitherSkullBlock.checkSpawn: clear the pattern, the wither
                // appears on the middle of the bottom row, then neighbours update.
                let cells: Vec<[i32; 3]> = (0..3)
                    .flat_map(|c| (0..3).map(move |row| (c, row)))
                    .map(|(c, row)| at(c, row))
                    .collect();
                self.clear_pattern_blocks(&cells)?;
                let base = at(1, 2);
                let at = [
                    f64::from(base[0]) + 0.5,
                    f64::from(base[1]) + 0.55,
                    f64::from(base[2]) + 0.5,
                ];
                let yaw = if ax == 1 { 0.0 } else { 90.0 };
                if let Some(id) = self.create_triggered("minecraft:wither", at, yaw)
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
                {
                    m.persistent = true;
                    horizon_client::mobs::wither::make_invulnerable(m);
                    let view =
                        self.mob_entity(&self.mobs.iter().find(|m| m.id == id).cloned().unwrap());
                    self.trigger_entity("minecraft:summoned_entity", "entity", &view);
                }
                self.update_pattern_blocks(&cells)?;
                return Ok(());
            }
        }
        Ok(())
    }
}
/// Wither skulls drawn as the skull model, blue when dangerous.
pub(super) fn skull_kind(p: &Projectile) -> Option<bool> {
    match p.kind {
        ProjectileKind::WitherSkull { dangerous } => Some(dangerous),
        _ => None,
    }
}
