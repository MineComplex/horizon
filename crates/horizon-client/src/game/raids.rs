//! Pillager patrols (`PatrolSpawner`), the raid omen, and raids (`Raids`):
//! their waves, boss bar, victory and the raiders' bookkeeping. The rules are
//! `horizon_client::patrol` and `horizon_client::raid`; this is the game's
//! side of them.
use horizon_core::geometry::Vec3;

impl crate::game::Sim<'_> {
    /// A firework a villager launches to celebrate (`CelebrateVillagersSurvivedRaid`).
    pub(super) fn villager_firework(&mut self, at: [f64; 3], color: usize, flight: u8) {
        let mut stack = horizon_client::items::ItemStack::new("minecraft:firework_rocket", 1);
        stack.data_mut().fireworks = Some((
            flight,
            vec![horizon_client::particles::FireworkExplosion {
                shape: horizon_client::particles::FireworkShape::Burst,
                colors: vec![horizon_client::particles::DYE_FIREWORK_COLORS[color & 15].1],
                fade: Vec::new(),
                trail: false,
                twinkle: false,
            }],
        ));
        let rocket = self.new_rocket(Vec3::new(at[0], at[1], at[2]), &stack, false);
        self.rockets.push(rocket);
    }
    /// `BehaviorUtils.throwItem`: a hero's gift tossed toward the player.
    pub(super) fn throw_gift(&mut self, table: &str, from: [f64; 3], to: [f64; 3]) {
        let drops = self
            .res
            .items()
            .loot_table(table, false, None, &mut self.world.random);
        let d = Vec3::new(to[0] - from[0], to[1] - from[1], to[2] - from[2]).normalize();
        for stack in drops {
            self.spawn_item(stack, from, [d.x * 0.3, d.y * 0.3, d.z * 0.3], 10);
        }
    }
}
