//! The game side of piglins (`PiglinAi`): spotting and picking up gold,
//! bartering when the admiring is over, and anger at players who open
//! their chests or mine their gold.
use horizon_client::mobs::{Species, Who};

impl crate::game::Sim<'_> {
    /// `PiglinAi.isWearingSafeArmor`.
    pub(super) fn wearing_gold(&self) -> bool {
        let tag = self
            .res
            .items()
            .item_tags
            .get("minecraft:piglin_safe_armor");
        self.armor
            .iter()
            .flatten()
            .any(|s| tag.is_some_and(|t| t.contains(&*s.item)))
    }
    /// `PiglinAi.angerNearbyPiglins`.
    pub(super) fn anger_piglins(&mut self, only_if_seen: bool) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        if self.creative || self.spectator() {
            return;
        }
        let at = p.position;
        let eye = [at.x, at.y + 1.62, at.z];
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            if m.species() != Species::Piglin
                || m.is_baby()
                || !m.alive()
                || m.ai.target.is_some()
                || (m.position.x - at.x).abs() > 16.0
                || (m.position.y - at.y).abs() > 16.0
                || (m.position.z - at.z).abs() > 16.0
            {
                continue;
            }
            if only_if_seen && !self.can_see(m.center(), eye) {
                continue;
            }
            let m = &mut self.mobs[i];
            m.extra.anger = 600;
            m.ai.target = Some(Who::Player);
        }
    }
    /// A clear line of sight between two points (blocks with collision stop it).
    fn can_see(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        let steps = 32;
        (1..steps).all(|i| {
            let f = f64::from(i) / f64::from(steps);
            let p = [
                from[0] + (to[0] - from[0]) * f,
                from[1] + (to[1] - from[1]) * f,
                from[2] + (to[2] - from[2]) * f,
            ];
            !self
                .res
                .facts()
                .get(self.world.block(
                    p[0].floor() as i32,
                    p[1].floor() as i32,
                    p[2].floor() as i32,
                ))
                .full_collision
        })
    }
    /// Whether breaking or opening this block angers piglins.
    pub(super) fn guarded_by_piglins(&self, name: &str) -> bool {
        self.res
            .items()
            .block_tags
            .get("minecraft:guarded_by_piglins")
            .is_some_and(|t| t.contains(name))
    }
}
