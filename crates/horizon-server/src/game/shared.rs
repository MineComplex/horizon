//! Server methods that lived in the client's mixed files.
use super::*;
use crate::hc::command::args::GameMode;
use crate::hc::text::Text;
use serde_json::{Value, json};

impl crate::game::Server<'_> {
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
        if let Some(net) = self.network.as_mut() {
            for g in &mut net.hub.players {
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

impl crate::game::Server<'_> {
    /// A server's `flush_to_window`: it has no window, so what the game queued for one goes.
    pub(crate) fn drop_window_output(&mut self) {
        self.sounds.queue.clear();
        self.hands_used.clear();
        self.pointer_grab = None;
    }
}

impl crate::game::Server<'_> {
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

impl crate::game::Server<'_> {
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

impl crate::game::Server<'_> {}

impl crate::game::Server<'_> {
    /// `ServerboundChangeGameModePacket` (F3+F4, an operator's):
    /// `GameModeCommand.setGameMode` with its feedback.
    pub(crate) fn change_game_mode(&mut self, mode: GameMode) {
        if self.set_game_mode(mode) {
            let name = Text::translate(&format!("gameMode.{}", mode.name()), &[]);
            self.feedback(Text::translate("commands.gamemode.success.self", &[name]));
        }
    }
}

impl crate::game::Server<'_> {
    /// Their kind and place, for `status`.
    pub(crate) fn projectiles_status(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let at = |p: Vec3| [p.x, p.y, p.z];
        let mut out = Vec::new();
        for f in &self.fangs.list {
            out.push(
                json!({"id": f.id, "kind": "minecraft:evoker_fangs", "position": f.pos,
                "yaw": f.yaw, "started": f.started, "life": f.life}),
            );
        }
        for t in &self.tnts {
            out.push(json!({"id": t.id, "kind": "minecraft:tnt", "position": at(t.position), "fuse": t.fuse}));
        }
        for f in &self.falling {
            out.push(json!({"id": f.id, "kind": "minecraft:falling_block", "position": at(f.position), "state": f.state.0}));
        }
        for c in &self.crystals {
            out.push(json!({"id": c.id, "kind": "minecraft:end_crystal", "position": at(c.position), "show_bottom": c.show_bottom}));
        }
        for t in &self.tridents {
            out.push(json!({"id": t.id, "kind": "minecraft:trident", "position": at(t.position)}));
        }
        for p in &self.creatures.projectiles {
            out.push(json!({"id": p.id, "kind": format!("{:?}", p.kind).split([' ', '{']).next().unwrap_or(""), "position": at(p.position), "velocity": at(p.velocity),
                "player_owned": p.owner == Some(crate::hc::mobs::Who::Player)}));
        }
        for (kind, p, _) in self.potions.thrown_entities() {
            out.push(json!({"kind": kind, "position": at(p)}));
        }
        for (id, p) in self.end.eye_positions() {
            out.push(json!({"id": id, "kind": "minecraft:eye_of_ender", "position": p}));
        }
        out.extend(self.hooks_status());
        out
    }
}

impl crate::game::Server<'_> {
    /// `SpyglassItem.use`: start scoping.
    pub(crate) fn use_spyglass(&mut self) {
        if self.using.is_none() {
            self.using = Some((self.held_slot(), 0));
            let at = self.eye_position();
            self.sounds
                .play("minecraft:item.spyglass.use", at, 1.0, 1.0);
        }
    }
}

impl crate::game::Server<'_> {
    /// `horizon-ctl save` and `save-all`.
    pub(crate) fn control_save(&mut self, all: bool) -> Result<()> {
        if all {
            let keys: Vec<_> = self.world.chunks.keys().copied().collect();
            self.world.modified.extend(keys);
        }
        self.save_game()
    }
}
