//! Fishing: `FishingRodItem.use` (cast / reel in), the `FishingHook` (flight,
//! bobbing, lure and bite timers, open water, hooking mobs, retrieving the
//! `gameplay/fishing` loot) and `FishingHookRenderer` (bobber and line).
use super::{Game, view_vector};
use horizon_client::{gizmos::Line, items::ItemStack, mesher::Vertex, pose::Pose};
use horizon_core::geometry::{Vec3, trig};
use horizon_protocol::entity_data::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Flying,
    Bobbing,
    Hooked(u32),
}
pub(super) struct Hook {
    /// Its entity id, which a server's players track it under.
    id: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
    state: State,
    nibble: i32,
    time_until_lured: i32,
    time_until_hooked: i32,
    open_water: bool,
}
impl Game<'_> {
    /// FishingHookRenderer: the bobbers (this player's and the others' a
    /// server tracks), camera-facing half-block quads centred on the hooks.
    pub(super) fn fishing_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let hooks: Vec<glam::DVec3> = self
            .hook_position(alpha)
            .into_iter()
            .chain(self.remote_hook_ends(alpha).into_iter().map(|(h, _)| h))
            .collect();
        if hooks.is_empty() {
            return;
        }
        let Some(r) = self
            .front
            .gui
            .atlas
            .sprites
            .get("entity/fishing/fishing_hook")
            .copied()
        else {
            return;
        };
        let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let s = 0.25;
        let uv = [[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]];
        for p in hooks {
            let light =
                self.world
                    .light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let corners = [
                p - right * s - up * s,
                p + right * s - up * s,
                p + right * s + up * s,
                p - right * s + up * s,
            ];
            for (v, uv) in corners.iter().zip(uv) {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: (*v - camera).as_vec3().to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
impl crate::game::Sim<'_> {
    /// `FishingRodCast`: the stack in the arm holding the rod
    /// (`FishingHookRenderer.getHoldingArm`) while the hook is out.
    pub(super) fn cast_rod(&self) -> Option<&ItemStack> {
        self.hook.as_ref()?;
        if self.main_hand_rod() {
            self.held()
        } else {
            self.offhand.as_ref()
        }
    }
    pub(super) fn main_hand_rod(&self) -> bool {
        self.held()
            .is_some_and(|s| &*s.item == "minecraft:fishing_rod")
    }
    /// The hook's position this frame.
    fn hook_position(&self, alpha: f64) -> Option<glam::DVec3> {
        let h = self.hook.as_ref()?;
        let lerp = |a: f64, b: f64| a + (b - a) * alpha;
        Some(glam::DVec3::new(
            lerp(h.previous.x, h.position.x),
            lerp(h.previous.y, h.position.y),
            lerp(h.previous.z, h.position.z),
        ))
    }
    /// FishingHookRenderer: each line, 16 black `lines` segments sagging
    /// from a quarter block above the hook to the rod in its owner's hand,
    /// `width` pixels wide (`Window.getAppropriateLineWidth`).
    pub(super) fn fishing_line(&self, alpha: f64, size: [u32; 2]) -> Vec<Line> {
        let own = self
            .hook_position(alpha)
            .zip(self.rod_hand_position(alpha, size));
        let width = horizon_client::gfx::outline_width(size[0]);
        let mut lines = Vec::new();
        for (hook, hand) in own.into_iter().chain(self.remote_hook_ends(alpha)) {
            let offset = (hand - (hook + glam::DVec3::new(0.0, 0.25, 0.0))).as_vec3();
            let point = |a: f32| {
                let y = offset.y * (a * a + a) * 0.5 + 0.25;
                hook + glam::Vec3::new(offset.x * a, y, offset.z * a).as_dvec3()
            };
            lines.extend((0..16).map(|i| Line {
                start: point(i as f32 / 16.0).to_array(),
                end: point((i + 1) as f32 / 16.0).to_array(),
                color: 0xFF00_0000,
                width,
            }));
        }
        lines
    }
    /// `FishingHookRenderer.getPlayerHandPos`: on the near plane in first
    /// person, beside the body otherwise.
    fn rod_hand_position(&self, alpha: f64, size: [u32; 2]) -> Option<glam::DVec3> {
        let p = self.player.as_ref()?;
        let right_arm = self.main_hand_rod() != self.settings.main_hand_left;
        let invert: f32 = if right_arm { 1.0 } else { -1.0 };
        let swing = trig::sin(f64::from(
            self.swing.animation(alpha as f32).sqrt() * std::f32::consts::PI,
        ));
        let feet = self.previous_position.lerp(p.position, alpha);
        let eye = glam::DVec3::new(feet.x, feet.y + f64::from(p.dimensions.eye_height), feet.z);
        if self.camera_type.first_person() {
            let fov = self.settings.fov as i32 as f32;
            // Camera.getNearPlane(fov).getPointOnPlane(invert * 0.525, -0.1).
            let aspect = f64::from(size[0]) / f64::from(size[1].max(1));
            let height =
                (f64::from(fov * (std::f64::consts::PI / 180.0) as f32) / 2.0).tan() * 0.05;
            let forward = glam::DVec3::from_array(view_vector(self.yaw, self.pitch));
            let left = -forward.cross(glam::DVec3::Y).normalize_or_zero();
            let up = forward.cross(left).normalize_or_zero();
            let point = forward * 0.05 + up * (height * -0.1)
                - left * (height * aspect * f64::from(invert * 0.525));
            let point = point * (960.0 / f64::from(fov));
            // Vec3.yRot(swing * 0.5), then xRot(-swing * 0.7).
            let (s, c) = (
                f64::from(trig::sin(f64::from(swing * 0.5))),
                f64::from(trig::cos(f64::from(swing * 0.5))),
            );
            let point = glam::DVec3::new(
                point.x * c + point.z * s,
                point.y,
                point.z * c - point.x * s,
            );
            let (s, c) = (
                f64::from(trig::sin(f64::from(-swing * 0.7))),
                f64::from(trig::cos(f64::from(-swing * 0.7))),
            );
            let point = glam::DVec3::new(
                point.x,
                point.y * c + point.z * s,
                point.z * c - point.y * s,
            );
            Some(eye + point)
        } else {
            let model = self.player_model.as_ref()?;
            let body =
                horizon_client::camera::lerp(alpha as f32, model.previous_body_yaw, model.body_yaw);
            let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
            let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
            let right = f64::from(invert) * 0.35;
            let crouch = if p.pose == Pose::Crouching {
                -0.1875
            } else {
                0.0
            };
            Some(
                eye + glam::DVec3::new(
                    -cos * right - sin * 0.8,
                    crouch - 0.45,
                    -sin * right + cos * 0.8,
                ),
            )
        }
    }
}
impl crate::game::Sim<'_> {
    /// The hooks drawn here, with their owners' ids (0 for this player's own).
    pub(super) fn hooks_status(&self) -> Vec<serde_json::Value> {
        let at = |p: Vec3| [p.x, p.y, p.z];
        let own = self.hook.iter().map(|h| (h.id, 0, h.position));
        let others = self
            .remote_hooks
            .iter()
            .map(|h| (h.id, h.owner, h.position));
        own.chain(others)
            .map(|(id, owner, p)| {
                serde_json::json!({"id": id, "kind": "minecraft:fishing_bobber", "owner": owner, "position": at(p)})
            })
            .collect()
    }
    /// The hook for `horizon-ctl status`.
    pub(super) fn hook_status(&self) -> Option<serde_json::Value> {
        let h = self.hook.as_ref()?;
        Some(serde_json::json!({
            "state": format!("{:?}", h.state),
            "nibble": h.nibble,
            "lured_in": h.time_until_lured,
            "hooked_in": h.time_until_hooked,
            "open_water": h.open_water,
        }))
    }
}

/// A hook a server tracks to this client (`FishingHook.recreateFromPacket`)
/// that another player cast: drawn where the server puts it, its line
/// running to that player's hand.
pub(super) struct RemoteHook {
    id: u32,
    owner: u32,
    pub(super) position: Vec3,
    pub(super) previous: Vec3,
}

impl Hook {
    /// This client's own hook as its server tracks it: drawn, never ticked here.
    fn remote(id: u32, at: Vec3) -> Self {
        Self {
            id,
            position: at,
            previous: at,
            state: State::Flying,
            nibble: 0,
            time_until_lured: 0,
            time_until_hooked: 0,
            open_water: true,
        }
    }
}

impl crate::game::Sim<'_> {
    /// `handleAddEntity` for a fishing bobber: this player's own becomes its
    /// hook (`Player.fishing`), another player's is drawn on its line to them.
    pub(super) fn spawn_remote_hook(&mut self, id: u32, at: Vec3, owner: i32) {
        let own = self.remote.as_ref().map(|r| r.entity_id);
        if own == Some(owner) {
            self.hook = Some(Hook::remote(id, at));
        } else {
            self.remote_hooks.push(RemoteHook {
                id,
                owner: owner as u32,
                position: at,
                previous: at,
            });
        }
    }
    /// One tick of interpolation for the hooks a server tracks.
    pub(super) fn step_remote_hooks(&mut self, mut step: impl FnMut(u32, Vec3) -> Vec3) {
        if let Some(h) = self.hook.as_mut() {
            h.previous = h.position;
            h.position = step(h.id, h.position);
        }
        for h in &mut self.remote_hooks {
            h.previous = h.position;
            h.position = step(h.id, h.position);
        }
    }
    /// `FishingHook.onSyncedDataUpdated`: this player's hook is biting
    /// (DATA_BITING), which `status` shows as a nibble.
    pub(super) fn remote_hook_data(&mut self, id: u32, values: &[(u8, i32, Value)]) {
        let Some(h) = self.hook.as_mut().filter(|h| h.id == id) else {
            return;
        };
        for (index, _, value) in values {
            if let (9, Value::Bool(biting)) = (index, value) {
                h.nibble = i32::from(*biting);
            }
        }
    }
    pub(super) fn remove_remote_hook(&mut self, id: u32) {
        if self.hook.as_ref().is_some_and(|h| h.id == id) {
            self.hook = None;
        }
        self.remote_hooks.retain(|h| h.id != id);
    }
    pub(super) fn clear_remote_hooks(&mut self) {
        {
            self.hook = None;
        }
        self.remote_hooks.clear();
    }
    /// Other players' hooks: the bobber and its line to the owner's hand
    /// (`FishingHookRenderer.getPlayerHandPos` in third person), as
    /// (hook, hand) this frame.
    fn remote_hook_ends(&self, alpha: f64) -> Vec<(glam::DVec3, glam::DVec3)> {
        let lerp = |a: Vec3, b: Vec3| {
            glam::DVec3::new(
                a.x + (b.x - a.x) * alpha,
                a.y + (b.y - a.y) * alpha,
                a.z + (b.z - a.z) * alpha,
            )
        };
        self.remote_hooks
            .iter()
            .filter_map(|h| {
                let owner = self.mobs.iter().find(|m| m.id == h.owner)?;
                let feet = lerp(owner.previous, owner.position);
                let body = horizon_client::camera::lerp(
                    alpha as f32,
                    owner.previous_body_yaw,
                    owner.body_yaw,
                );
                let angle = f64::from(body * (std::f64::consts::PI / 180.0) as f32);
                let (sin, cos) = (f64::from(trig::sin(angle)), f64::from(trig::cos(angle)));
                // The right arm; a crouching owner's eyes are lower.
                let eye = owner.eye_y() - owner.position.y;
                let hand = feet
                    + glam::DVec3::new(
                        -cos * 0.35 - sin * 0.8,
                        eye - 0.45,
                        -sin * 0.35 + cos * 0.8,
                    );
                Some((lerp(h.previous, h.position), hand))
            })
            .collect()
    }
}
