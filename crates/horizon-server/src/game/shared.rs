//! Server methods that lived in the client's mixed files.
use super::*;
use crate::hc::command::args::GameMode;
use serde_json::{Value, json};




impl crate::game::Sim<'_> {
    /// The hurts and melee attacks each player logged since the last call
    /// (`PlayerMotion::hurt_log`, `attack_log`), taken out.
    pub(crate) fn control_hurt_log(&mut self) -> Value {
        fn drain(
            name: &str,
            hurt_log: &mut Vec<crate::hc::health::HurtRecord>,
            attack_log: &mut Vec<crate::hc::health::AttackRecord>,
        ) -> Value {
            let hurts: Vec<Value> = hurt_log
                .drain(..)
                .map(|r| {
                    json!({
                        "t": r.game_time, "source": r.source, "amount": r.amount,
                        "blocked": r.blocked, "taken": r.taken, "hurt": r.hurt,
                        "armor": r.armor, "toughness": r.toughness, "health": r.health,
                        "fire_ticks": r.fire_ticks, "knockback": r.knockback,
                    })
                })
                .collect();
            let attacks: Vec<Value> = attack_log
                .drain(..)
                .map(|r| {
                    json!({
                        "t": r.game_time, "target": r.target, "id": r.id, "killed": r.killed, "amount": r.amount,
                        "scale": r.scale, "crit": r.crit, "landed": r.landed, "dealt": r.dealt,
                    })
                })
                .collect();
            json!({ "name": name, "hurts": hurts, "attacks": attacks })
        }
        let mut players = Vec::new();
        if let Some(p) = self.player.as_mut() {
            players.push(drain("host", &mut p.hurt_log, &mut p.attack_log));
        }
        if let Some(host) = self.lan.as_mut() {
            for g in &mut host.hub.guests {
                let name = g.name.clone();
                if let Some(m) = g.motion.as_mut() {
                    players.push(drain(&name, &mut m.hurt_log, &mut m.attack_log));
                } else if let Some((mut hurts, mut attacks)) = g.logs_kept.take() {
                    // Dead, not yet respawned: what its last body logged.
                    players.push(drain(&name, &mut hurts, &mut attacks));
                }
            }
        }
        json!({ "ok": true, "players": players })
    }
}

impl crate::game::Sim<'_> {
    /// A server's `flush_to_window`: it has no window, so what the game queued for one goes.
    pub(crate) fn drop_window_output(&mut self) {
        self.sounds.queue.clear();
        self.ui.chat.drop_unwrapped();
        self.rumbles.clear();
        self.hands_used.clear();
        self.pointer_grab = None;
    }
}


impl crate::game::Sim<'_> {
    /// `isEyeInFluid(WATER)`: the eye inside the water of its block column.
    pub(crate) fn eye_in_water(&self, x: f64, eye_y: f64, z: f64) -> bool {
        let (bx, by, bz) = (x.floor() as i32, eye_y.floor() as i32, z.floor() as i32);
        let f = self.res.facts().get(self.world.block(bx, by, bz));
        if f.fluid != Fluid::Water {
            return false;
        }
        let above = self.res.facts().get(self.world.block(bx, by + 1, bz));
        let height = if above.fluid == Fluid::Water {
            1.0
        } else {
            f64::from(f.fluid_level) / 9.0
        };
        eye_y <= f64::from(by) + height
    }
}








impl crate::game::Sim<'_> {
    pub(crate) fn state_string(&self, state: BlockStateId) -> String {
        let Some(s) = self.pack.assets.registry.state(state) else {
            return "?".into();
        };
        if s.properties.is_empty() {
            return s.name.clone();
        }
        let props: Vec<String> = s
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}[{}]", s.name, props.join(","))
    }
}


impl crate::game::Sim<'_> {
    /// `LevelEventHandler.levelEvent` 3011 to 3021.
    pub(crate) fn trial_level_event(&mut self, id: i32, pos: [i32; 3], data: i32) {
        // TrialSpawner.FlameParticle.decode.
        let spawn_flame = if data == 1 {
            T::SoulFireFlame
        } else {
            T::Flame
        };
        let sound = |s: &mut Self, name: &str, volume: f32| {
            let pitch = event_pitch(s.particles.level_random());
            s.local_sound(name, pos, volume, pitch);
        };
        match id {
            3011 => self.trial_spawn_particles(pos, spawn_flame),
            3012 => {
                sound(self, "block.trial_spawner.spawn_mob", 1.0);
                self.trial_spawn_particles(pos, spawn_flame);
            }
            3013 | 3019 => {
                sound(self, "block.trial_spawner.detect_player", 1.0);
                let ty = if id == 3013 {
                    T::TrialSpawnerDetection
                } else {
                    T::TrialSpawnerDetectionOminous
                };
                self.detect_player_particles(pos, data, ty);
            }
            3014 => {
                sound(self, "block.trial_spawner.eject_item", 1.0);
                self.eject_item_particles(pos);
            }
            3015 => {
                if matches!(
                    self.world.block_entities.get(&(pos[0], pos[1], pos[2])),
                    Some(BlockEntity::Vault(_))
                ) {
                    self.vault_activation_particles(pos, data != 0);
                    sound(self, "block.vault.activate", 1.0);
                }
            }
            3016 => {
                // emitDeactivationParticles.
                let ty = flame(data != 0);
                for _ in 0..20 {
                    let random = self.particles.level_random();
                    let p = [0.4, 0.4, 0.4].map(|lo| next_double(random, lo, 0.6));
                    let v = [0; 3].map(|_| random.next_gaussian() * 0.02);
                    let at = std::array::from_fn(|i| f64::from(pos[i]) + p[i]);
                    self.particle(ty, at, v);
                }
                sound(self, "block.vault.deactivate", 1.0);
            }
            3017 => self.eject_item_particles(pos),
            3018 => {
                for _ in 0..10 {
                    let random = self.particles.level_random();
                    let v = [0; 3].map(|_| random.next_gaussian() * 0.02);
                    let at = pos.map(|c| f64::from(c) + random.next_double());
                    self.particle(T::Poof, at, v);
                }
                sound(self, "block.cobweb.place", 1.0);
            }
            3020 => {
                sound(
                    self,
                    "block.trial_spawner.ominous_activate",
                    if data == 0 { 0.3 } else { 1.0 },
                );
                self.detect_player_particles(pos, 0, T::TrialSpawnerDetectionOminous);
                // addBecomeOminousParticles.
                for _ in 0..20 {
                    let random = self.particles.level_random();
                    let at = pos.map(|c| f64::from(c) + 0.5 + (random.next_double() - 0.5) * 2.0);
                    let v = [0; 3].map(|_| random.next_gaussian() * 0.02);
                    self.particle(T::TrialOmen, at, v);
                    self.particle(T::SoulFireFlame, at, v);
                }
            }
            3021 => {
                sound(self, "block.trial_spawner.spawn_item", 1.0);
                self.trial_spawn_particles(pos, spawn_flame);
            }
            _ => {}
        }
    }
}

impl crate::game::Sim<'_> {
    /// `ServerboundChangeGameModePacket` handled by the integrated server:
    /// `GameModeCommand.setGameMode` with its feedback.
    pub(crate) fn change_game_mode(&mut self, mode: GameMode) {
        
        if self.set_game_mode(mode) {
            let name = Text::translate(&format!("gameMode.{}", mode.name()), &[]);
            self.feedback(Text::translate("commands.gamemode.success.self", &[name]));
        }
    }
}
