//! Riding horses, donkeys and mules: feeding (`handleEating`), saddles,
//! armour and chests, mounting, the rider's control and jump bar
//! (`LocalPlayer.aiStep`'s riding jump), positioning, dismounting, and the
//! mount's health and jump bar on the HUD.
use super::Game;
use horizon_client::{
    hud::HudBatch,
    mobs::{MobEvent, Species, equine},
    sprint::Keys,
};
use horizon_core::geometry::Vec3;

/// `passengerAttachments` minus the player's 0.6 vehicle attachment.
fn seat(s: Species) -> f64 {
    let attach = match s {
        Species::Pig => 0.86875,
        Species::Strider => 1.7,
        Species::Horse => 1.44375,
        Species::Llama | Species::TraderLlama => 1.37,
        Species::Donkey => 1.1125,
        Species::Mule => 1.2125,
        Species::Nautilus | Species::ZombieNautilus => 1.1375,
        _ => 1.31875,
    };
    attach - 0.6
}
#[derive(Default)]
pub(super) struct MountState {
    /// `jumpRidingTicks`: -10 cooling down after a jump, >= 0 charging.
    pub(super) jump_ticks: i32,
    /// `jumpRidingScale` (0..1).
    pub(super) jump_scale: f32,
    was_jumping: bool,
}
impl Game<'_> {
    /// `PlayerRideableJumping`: horses, camels and nautili (the dash).
    fn jumps(species: Species) -> bool {
        equine::is_equine(species) || horizon_client::mobs::nautilus::is_nautilus(species)
    }
    /// `Hud.extractVehicleHealth`, and the jump bar when it is the
    /// contextual bar (`jump_bar`). Returns the mount's heart count
    /// (`getVehicleMaxHearts`).
    pub(super) fn draw_mount_hud(
        &self,
        batch: &mut HudBatch,
        w: i32,
        h: i32,
        jump_bar: bool,
    ) -> i32 {
        let Some(i) = self.mount_index() else {
            return 0;
        };
        let m = &self.mobs[i];
        let atlas = &self.front.gui.atlas;
        let hearts = horizon_client::hud_layout::vehicle_max_hearts(m.attrs.max_health);
        let draws = horizon_client::hud_layout::vehicle_health(w, h, hearts, m.health);
        horizon_client::hud_layout::paint(batch, atlas, &draws);
        if jump_bar {
            // JumpableVehicleBar: `getJumpCooldown`, which only the camel's and
            // the nautilus' dash have; a horse always shows the progress.
            let cooldown = i32::from(
                self.mount.jump_ticks < 0
                    && (matches!(m.species(), Species::Camel | Species::CamelHusk)
                        || horizon_client::mobs::nautilus::is_nautilus(m.species())),
            );
            let draws =
                horizon_client::hud_overlays::jump_bar(w, h, cooldown, self.mount.jump_scale);
            horizon_client::hud_layout::paint(batch, atlas, &draws);
        }
        hearts
    }
}
impl crate::game::Sim<'_> {
    fn mount_index(&self) -> Option<usize> {
        let id = self.riding?;
        self.mobs.iter().position(|m| m.id == id)
    }
    /// `LocalPlayer.aiStep`'s riding jump: the bar charges while jump is
    /// held; on release, the jump's strength (0 when there is none).
    pub(super) fn charge_mount_jump(&mut self, i: usize, keys: Keys) -> f32 {
        let saddled =
            self.mobs[i].extra.saddled && crate::game::Game::jumps(self.mobs[i].species());
        let mut jump = 0.0;
        if saddled {
            let st = &mut self.mount;
            if st.jump_ticks < 0 {
                st.jump_ticks += 1;
                if st.jump_ticks == 0 {
                    st.jump_scale = 0.0;
                }
            }
            if st.was_jumping && !keys.jump {
                st.jump_ticks = -10;
                let amount = (st.jump_scale * 100.0).floor() as i32;
                // PlayerRideableJumping.getPlayerJumpPendingScale
                jump = if amount >= 90 {
                    1.0
                } else {
                    0.4 + 0.4 * amount as f32 / 90.0
                };
            } else if !st.was_jumping && keys.jump {
                st.jump_ticks = 0;
                st.jump_scale = 0.0;
            } else if st.was_jumping {
                st.jump_ticks += 1;
                st.jump_scale = if st.jump_ticks < 10 {
                    st.jump_ticks as f32 * 0.1
                } else {
                    0.8 + 2.0 / (st.jump_ticks - 9) as f32 * 0.1
                };
            }
            st.was_jumping = keys.jump;
        }
        jump
    }
    /// `positionRider`: the player sits on the mount after it moves.
    pub(super) fn position_rider(&mut self) {
        let Some(i) = self.mount_index() else {
            return;
        };
        let m = &self.mobs[i];
        let mut at = Vec3::new(m.position.x, m.position.y + seat(m.species()), m.position.z);
        if horizon_client::mobs::camel::is_camel(m.species()) {
            // Camel.getPassengerAttachmentPoint: the driver half a block
            // forward, at the body anchor (lower while sitting).
            let scale = if m.is_baby() { 0.6 } else { 1.0 };
            let anchor = if horizon_client::mobs::camel::sitting(m) {
                (2.375 - 1.43) * scale - if m.is_baby() { 0.09375 } else { 0.375 } + 0.2 * scale
            } else {
                2.375 * scale - if m.is_baby() { 0.09375 } else { 0.375 }
            };
            let yaw = m.yaw.to_radians();
            at = Vec3::new(
                m.position.x - f64::from(0.5 * scale * yaw.sin()),
                m.position.y + f64::from(anchor) - 0.6,
                m.position.z + f64::from(0.5 * scale * yaw.cos()),
            );
        }
        if m.species() == Species::HappyGhast {
            // The first passenger attachment (0, 4, 1.7) of the harnessed back.
            let yaw = f64::from(m.yaw.to_radians());
            at = Vec3::new(
                m.position.x - 1.7 * yaw.sin(),
                m.position.y + 4.0 - 0.6,
                m.position.z + 1.7 * yaw.cos(),
            );
        }
        if horizon_client::mobs::llama::is_llama(m.species()) {
            // The passenger attachment (0, 1.37, -0.3), half of it for a baby.
            let (y, z) = if m.is_baby() {
                (0.81, -0.15)
            } else {
                (1.37, -0.3)
            };
            let yaw = f64::from(m.yaw.to_radians());
            at = Vec3::new(
                m.position.x - z * yaw.sin(),
                m.position.y + y - 0.6,
                m.position.z + z * yaw.cos(),
            );
        }
        if let Some(p) = self.player.as_mut() {
            self.previous_position = p.position;
            p.position = at;
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
    }
    /// `removePassenger` + `getDismountLocationForPassenger`: beside the mount.
    pub(super) fn dismount_mount(&mut self) {
        let Some(id) = self.riding.take() else {
            return;
        };
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        m.rider = None;
        if m.species() == Species::HappyGhast {
            let id = m.id;
            self.dismount_happy_ghast(id);
            return;
        }
        let yaw = m.yaw.to_radians();
        let side = Vec3::new(f64::from(yaw.cos()), 0.0, f64::from(yaw.sin()));
        let w = f64::from(m.width()) / 2.0 + 0.4;
        let base = m.position;
        let candidates = [
            Vec3::new(base.x - side.x * w, base.y, base.z - side.z * w),
            Vec3::new(base.x + side.x * w, base.y, base.z + side.z * w),
            Vec3::new(base.x, base.y + f64::from(m.height()), base.z),
        ];
        let spot = candidates
            .into_iter()
            .find(|c| {
                let f = |dy: i32| {
                    self.res
                        .facts()
                        .get(self.world.block(
                            c.x.floor() as i32,
                            c.y.floor() as i32 + dy,
                            c.z.floor() as i32,
                        ))
                        .full_collision
                };
                !f(0) && !f(1)
            })
            .unwrap_or(candidates[2]);
        if let Some(p) = self.player.as_mut() {
            p.position = spot;
            p.velocity = Vec3::ZERO;
        }
    }
    pub(super) fn mount_event(&mut self, e: &MobEvent) {
        match *e {
            MobEvent::Tamed { mob } => {
                let owner = self.level.player_uuid.or(Some([0, 0, 0, 1]));
                if let Some(view) = self
                    .mobs
                    .iter()
                    .find(|m| m.id == mob)
                    .map(|m| self.mob_entity(m))
                {
                    self.trigger_entity("minecraft:tame_animal", "entity", &view);
                }
                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
                    m.extra.owner = owner;
                    m.persistent = true;
                    // AbstractHorse.tameWithName: entity event 7, spawnTamingParticles(true).
                    self.mob_event_particles(mob, 7);
                }
            }
            MobEvent::Buck { mob } => {
                if self.riding == Some(mob) {
                    self.dismount_mount();
                }
                // RunAroundLikeCrazyGoal.tick: entity event 6, smoke.
                self.mob_event_particles(mob, 6);
            }
            _ => {}
        }
    }
    /// `Player.jumpableVehicle`: a saddled mount that jumps (not a pig or a
    /// strider, which a saddle only steers).
    pub(super) fn jumpable_mount(&self) -> bool {
        self.mount_index().is_some_and(|i| {
            self.mobs[i].extra.saddled && crate::game::Game::jumps(self.mobs[i].species())
        })
    }
    /// `willPrioritizeJumpInfo`: charging a jump or recharging.
    pub(super) fn mount_jumping(&self) -> bool {
        self.mount.jump_scale > 0.0 || self.mount.jump_ticks < 0
    }
}
