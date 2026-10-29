//! Brushing suspicious sand and gravel: `BrushItem.onUseTick` and
//! `BrushableBlockEntity`. The block's loot table stays in its saved block
//! entity (like a decorated pot's) until the first counted stroke rolls it.
use super::{Hit, view_vector};
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::random::RandomSource;
use std::collections::HashMap;

/// A brushable block's counters. Vanilla never saves them: the block's
/// `dusted` state is all that is left after a reload.
#[derive(Default)]
pub(super) struct BrushState {
    count: i32,
    resets_at: i64,
}

/// `BrushableBlockEntity.getCompletionState`: the `dusted` property.
fn completion_state(count: i32) -> i32 {
    match count {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

pub(super) type Brushing = HashMap<[i32; 3], BrushState>;

impl crate::game::Sim<'_> {
    /// `BrushItem.spawnDustParticles`.
    pub(super) fn brush_dust(&mut self, hit: &Hit, off_hand: bool) {
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let Some((sprite, tint)) = self.particle_sprite(state, hit.pos) else {
            return;
        };
        let options = ParticleOptions::block(ParticleType::Block, sprite, tint);
        // The arm that brushes: the main arm, or the other one for the off hand.
        let flip = if off_hand == self.settings.main_hand_left {
            1.0
        } else {
            -1.0
        };
        let view = view_vector(self.yaw, self.pitch);
        // DustParticlesDelta.fromDirection.
        let (dx, dz) = match hit.face {
            [0, _, 0] => (view[2], -view[0]),
            [0, 0, -1] => (1.0, -0.1),
            [0, 0, 1] => (-1.0, 0.1),
            [-1, 0, 0] => (-0.1, -1.0),
            _ => (0.1, 1.0),
        };
        let count = 7 + self.particles.level_random().next_int_bound(5).unwrap_or(0);
        let x = hit.point[0] - if hit.face == [-1, 0, 0] { 1.0e-6 } else { 0.0 };
        let z = hit.point[2] - if hit.face == [0, 0, -1] { 1.0e-6 } else { 0.0 };
        for _ in 0..count {
            let random = self.particles.level_random();
            let v = [
                dx * flip * 3.0 * random.next_double(),
                0.0,
                dz * flip * 3.0 * random.next_double(),
            ];
            self.particles.add(&options, [x, hit.point[1], z], v);
        }
    }
    fn set_dusted(&mut self, pos: [i32; 3], dusted: i32) {
        let registry = self.pack.assets.registry;
        let Some(mut state) = registry
            .state(self.world.block(pos[0], pos[1], pos[2]))
            .cloned()
        else {
            return;
        };
        state.properties.insert("dusted".into(), dusted.to_string());
        if let Ok(state) = registry.resolve(&state) {
            let _ = self.level_set(pos, state);
        }
    }
    fn schedule_brushable_tick(&mut self, pos: [i32; 3]) {
        let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.schedule_tick(level, ticks, at, 2));
    }
    /// `BrushableBlockEntity.checkReset`, from `BrushableBlock.tick`: a block
    /// left alone loses two counts, again every four ticks.
    pub(super) fn brushable_tick(&mut self, pos: [i32; 3]) {
        let time = self.world.game_time;
        let Some(mut brush) = self.brushing.remove(&pos) else {
            return;
        };
        let before = completion_state(brush.count);
        if brush.count != 0 && time >= brush.resets_at {
            brush.count = (brush.count - 2).max(0);
            brush.resets_at = time + 4;
        }
        let after = completion_state(brush.count);
        if brush.count != 0 {
            self.brushing.insert(pos, brush);
            self.schedule_brushable_tick(pos);
        }
        if before != after {
            self.set_dusted(pos, after);
        }
    }
    /// Level event 3008 (`LevelEventHandler`): the finished block's sound
    /// and break particles.
    pub(super) fn brush_complete_event(&mut self, pos: [i32; 3], data: i32) {
        let state = horizon_world::registry::BlockStateId(data as u32);
        let sound = match &*self.res.facts().get(state).name {
            "minecraft:suspicious_sand" => "minecraft:item.brush.brushing.sand.complete",
            "minecraft:suspicious_gravel" => "minecraft:item.brush.brushing.gravel.complete",
            _ => "",
        };
        if !sound.is_empty() {
            let at = pos.map(|v| f64::from(v) + 0.5);
            // Each client plays it from the event itself.
            self.sounds.play_unshared(sound, Some(at), 1.0, 1.0);
        }
        self.destroy_particles(pos, state);
    }
}
