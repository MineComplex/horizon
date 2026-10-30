//! Mobs on the client: the world the mob code reads, the player as mobs see
//! it, the particles of the server's mob events, projectiles and rendering.
use super::{Dim, Game, view_vector};
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    blocks::Fluid,
    collisions::SceneCollisions,
    mesher::Vertex,
    mobs::{
        MobWorld, PlayerView, Seen, Species, Who,
        spawning::{self},
    },
    projectiles::{Projectile, ProjectileKind, ProjectileWorld},
};
use horizon_core::{
    geometry::{Aabb, Vec3, traverse_blocks},
    position::BlockPos,
    random::RandomSource,
};

/// Mob-system state owned by the game.
#[derive(Default)]
pub(super) struct Creatures {
    pub projectiles: Vec<Projectile>,
    /// Ticks left of the elder guardian's apparition (MobAppearanceParticle).
    pub elder_apparition: i32,
}
pub(super) struct ProjWorld<'a, 'w> {
    pub w: &'a MobWorld<'w>,
    pub targets: Vec<(Who, Aabb)>,
}
impl ProjectileWorld for ProjWorld<'_, '_> {
    fn clip(&self, from: Vec3, to: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
        let mut scene = SceneCollisions {
            world: self.w.world,
            binding: self.w.collision,
            subject: None,
        };
        let r = traverse_blocks(from, to, 256, |p: BlockPos| {
            let state = self.w.world.block(p.x, p.y, p.z);
            if self.w.facts.is_air(state) {
                return None;
            }
            let shape = self
                .w
                .collision
                .collision_shape(&mut scene, state, [p.x, p.y, p.z])
                .ok()?;
            shape.clip(from, to, p)
        });
        let hit = r.ok().flatten()?;
        Some((
            hit.location,
            [hit.block.x, hit.block.y, hit.block.z],
            hit.direction.normal(),
        ))
    }
    fn in_water(&self, at: Vec3) -> bool {
        self.w.fluid(
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ) == Fluid::Water
    }
    fn targets(&self) -> &[(Who, Aabb)] {
        &self.targets
    }
    fn empty(&self, p: [i32; 3]) -> bool {
        self.w.facts.is_air(self.w.world.block(p[0], p[1], p[2]))
    }
    fn solid(&self, p: [i32; 3]) -> bool {
        self.w
            .facts
            .get(self.w.world.block(p[0], p[1], p[2]))
            .full_collision
    }
}
/// `FlameFeatureRenderer`: fire quads stacked up a burning entity's box,
/// turned to the camera around Y, at full block light. `at` is the entity's
/// feet relative to the camera.
fn flame_geometry(
    atlas: &horizon_client::hud::HudAtlas,
    at: glam::Vec3,
    width: f32,
    height: f32,
    camera_yaw: f32,
    light: u8,
    out: &mut Vec<Vertex>,
) {
    let (Some(fire0), Some(fire1)) = (
        atlas.sprites.get("block/fire_0").copied(),
        atlas.sprites.get("block/fire_1").copied(),
    ) else {
        return;
    };
    let s = width * 1.4;
    let mut h = height / s;
    // Mth.rotationAroundAxis(Y, camera.orientation): the camera's turn about Y.
    let turn = glam::Mat3::from_rotation_y(std::f32::consts::PI - camera_yaw.to_radians());
    let lift = 0.3 - (h as i32) as f32 * 0.02;
    let packed = (u32::from(light >> 4) * 16) | ((15 * 16) << 8);
    let (mut r, mut yo, mut zo, mut ss) = (0.5f32, 0.0f32, 0.0f32, 0);
    while h > 0.0 {
        let tex = if ss % 2 == 0 { fire0 } else { fire1 };
        let (mut u0, v0, mut u1, v1) = (tex.u0, tex.v0, tex.u1, tex.v1);
        if ss / 2 % 2 == 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        for (x, y, uv) in [
            (-r, -yo, [u1, v1]),
            (r, -yo, [u0, v1]),
            (r, 1.4 - yo, [u0, v0]),
            (-r, 1.4 - yo, [u1, v0]),
        ] {
            let p = at + turn * glam::Vec3::new(x, y, zo + lift) * s;
            out.push(Vertex {
                normal: [0; 4],
                pos: p.to_array(),
                uv,
                layer: 0,
                color: 0xFFFF_FFFF,
                light: packed,

                glint_uv: [0.0, 0.0],
            });
        }
        h -= 0.45;
        yo -= 0.45;
        r *= 0.9;
        zo -= 0.03;
        ss += 1;
    }
}
impl Game<'_> {
    /// Mob models and projectiles for the entity pass.
    pub(super) fn mob_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let atlas = &self.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        let scale = self.view_scale();
        for mob in &self.mobs {
            let entity_start = out.len();
            let outline = self.mob_outline(mob);
            // Entity.DATA_SHARED_FLAGS_ID invisible: only the name tag draws (and
            // the outline of a glowing mob, LivingEntityRenderer.getRenderType).
            if mob.invisible && outline.is_none() {
                continue;
            }
            let d = glam::DVec3::new(mob.position.x, mob.position.y, mob.position.z) - camera;
            // Entity.shouldRenderAtSqrDistance: 64 times the bounding box's
            // mean edge, scaled by the view scale. RemotePlayer's box counts
            // ten times.
            let (w, h, _) = mob.dims();
            let edge = (w + h + w) / 3.0
                * if &*mob.kind.id == "minecraft:player" {
                    10.0
                } else {
                    1.0
                };
            let range = edge * 64.0 * scale;
            if d.length_squared() >= range * range {
                continue;
            }
            // EntityRenderer.shouldRender: the culling box, grown by 0.5
            // (GuardianRenderer also draws while its beam is out).
            if let Some(view) = &self.view_frustum
                && horizon_client::mobs::guardian::beam(mob, alpha as f32).is_none()
            {
                let lo =
                    d.as_vec3() - glam::Vec3::new(w as f32 / 2.0 + 0.5, 0.5, w as f32 / 2.0 + 0.5);
                let hi = d.as_vec3()
                    + glam::Vec3::new(w as f32 / 2.0 + 0.5, h as f32 + 0.5, w as f32 / 2.0 + 0.5);
                if !view.intersects(lo, hi) {
                    continue;
                }
            }
            self.mark_entity(entity_start, &mob.kind.id);
            let light = self.world.light(
                mob.position.x.floor() as i32,
                (mob.position.y + mob.eye_height()).floor() as i32,
                mob.position.z.floor() as i32,
            );
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            if mob.fire_ticks > 0 || matches!(mob.species(), Species::Blaze | Species::MagmaCube) {
                packed = (15 * 16) | ((15 * 16) << 8);
            }
            if mob.species() == Species::SulfurCube
                && !horizon_client::mobs::trader::is_invisible(mob)
            {
                self.sulfur_cube_body_geometry(
                    mob,
                    alpha as f32,
                    horizon_client::mobs::render::base_transform(mob, alpha as f32, camera),
                    packed,
                    out,
                );
            }
            // Other players on a server: PlayerRenderer's HumanoidArmorLayer.
            let armor = if &*mob.kind.id == "minecraft:player" {
                self.worn_armor_layers(mob)
            } else {
                Vec::new()
            };
            horizon_client::mobs::render::render_with(
                mob,
                alpha as f32,
                camera,
                &lookup,
                packed,
                self.res.mob_data(),
                &armor,
                out,
            );
            if &*mob.kind.id == "minecraft:player" {
                // PlayerRenderer's WingsLayer and SpinAttackEffectLayer.
                self.player_elytra_geometry(
                    mob,
                    mob.extra.chest.as_deref(),
                    camera,
                    alpha as f32,
                    packed,
                    out,
                );
                self.spin_attack_geometry(mob, camera, alpha as f32, packed, out);
            }
            self.mob_block_geometry(mob, camera, alpha, packed, out);
            if let Some(rgb) = outline {
                self.mark_outline(&out[entity_start..], rgb);
                if mob.invisible {
                    out.truncate(entity_start);
                    continue;
                }
            }
            // Glowing outlines and a guardian's beam remain visible independently
            // of whether the body is hidden behind terrain.
            if outline.is_none()
                && horizon_client::mobs::guardian::beam(mob, alpha as f32).is_none()
            {
                self.cull_entity_vertices(out, entity_start);
            }
            // GuardianRenderer: the laser to the locked target.
            if let Some((time, scale)) = horizon_client::mobs::guardian::beam(mob, alpha as f32)
                && let Some(target) = self.beam_target(mob.ai.target, alpha)
                && let Some(uv) = lookup("entity/guardian/guardian_beam")
            {
                let a = alpha;
                let eye = glam::DVec3::new(
                    mob.previous.x + (mob.position.x - mob.previous.x) * a,
                    mob.previous.y + (mob.position.y - mob.previous.y) * a + mob.eye_height(),
                    mob.previous.z + (mob.position.z - mob.previous.z) * a,
                ) - camera;
                horizon_client::mobs::guardian::beam_geometry(
                    eye.as_vec3(),
                    (target - camera).as_vec3(),
                    time,
                    scale,
                    uv,
                    out,
                );
            }
        }
        self.elder_apparition_geometry(alpha, out);
        self.spawner_geometry(camera, alpha, out);
        // Projectiles: the baked vanilla arrow model, the rest as item billboards.
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        let forward = glam::Vec3::new(
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        let right = forward.cross(glam::Vec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let live = self.creatures.projectiles.iter().map(|a| {
            let at = [
                a.previous.x + (a.position.x - a.previous.x) * alpha,
                a.previous.y + (a.position.y - a.previous.y) * alpha,
                a.previous.z + (a.position.z - a.previous.z) * alpha,
            ];
            (a, at)
        });
        // ItemPickupParticle: a picked-up arrow flying to its collector.
        let taken = self.pickups.live.iter().filter_map(|p| match &p.taken {
            super::pickups::Taken::Arrow(a) => Some((&**a, super::pickups::position(p, alpha))),
            _ => None,
        });
        // A glowing projectile's outline: what it drew up to the next one.
        let mut glowing = None;
        for (a, at) in live.chain(taken) {
            if let Some((start, rgb)) = glowing.take() {
                self.mark_outline(&out[start..], rgb);
            }
            if a.id != 0 && self.entity_glowing(a.id) {
                glowing = Some((out.len(), super::glowing::team_color(self.mob_team(a.id))));
            }
            let p = glam::DVec3::from_array(at) - camera;
            let c = p.as_vec3();
            let light = self.world.light(
                a.position.x.floor() as i32,
                a.position.y.floor() as i32,
                a.position.z.floor() as i32,
            );
            let mut packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            if a.is_fireball() {
                packed = (15 * 16) | ((15 * 16) << 8);
            }
            if a.is_arrow() {
                let Some(model) = self.arrow_model.as_ref() else {
                    continue;
                };
                let Some(r) = atlas.sprites.get("entity/projectiles/arrow").copied() else {
                    continue;
                };
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((a.yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z(a.pitch.to_radians());
                horizon_client::mobs::render_model(
                    model,
                    m,
                    &|part| part.pose,
                    [r.u0, r.v0, r.u1, r.v1],
                    packed,
                    0,
                    out,
                );
                if a.arrow.as_ref().is_some_and(|s| s.on_fire()) {
                    flame_geometry(atlas, c, 0.5, 0.5, self.yaw, light, out);
                }
                continue;
            }
            // ThrownTridentRenderer.
            if matches!(a.kind, ProjectileKind::Trident) {
                let Some(model) = self.trident_model.as_ref() else {
                    continue;
                };
                let Some(r) = atlas.sprites.get("entity/trident/trident").copied() else {
                    continue;
                };
                let m = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_rotation_y((a.yaw - 90.0).to_radians())
                    * glam::Mat4::from_rotation_z((a.pitch + 90.0).to_radians());
                let poses = horizon_client::mobs::anim::Poses::new(model);
                horizon_client::mobs::render::draw(
                    model,
                    m,
                    &poses,
                    [r.u0, r.v0, r.u1, r.v1],
                    0xFFFF_FFFF,
                    packed,
                    0,
                    out,
                );
                continue;
            }
            // WindChargeRenderer.
            if matches!(
                a.kind,
                ProjectileKind::WindCharge | ProjectileKind::BreezeWindCharge
            ) {
                self.wind_charge_geometry(a, c, packed, out);
                continue;
            }
            // ShulkerBulletRenderer: the tumbling bullet model, then a larger
            // translucent copy of it (colour 0x26FFFFFF); always 15 block light.
            if matches!(a.kind, ProjectileKind::ShulkerBullet { .. }) {
                static BULLET: std::sync::OnceLock<Option<horizon_client::mobs::Part>> =
                    std::sync::OnceLock::new();
                let model = BULLET.get_or_init(|| {
                    horizon_client::mobs::load_model(
                        &self.options.resources,
                        "minecraft:shulker_bullet#main",
                    )
                });
                if let (Some(model), Some(r)) =
                    (model, atlas.sprites.get("entity/shulker/spark").copied())
                {
                    use horizon_core::geometry::trig;
                    let tc = a.age as f32 + alpha as f32;
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("main", a.yaw.to_radians());
                    poses.xrot("main", a.pitch.to_radians());
                    let light = (packed & 0xFF) | ((15 * 16) << 8);
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.15, 0.0))
                        * glam::Mat4::from_rotation_y(
                            (trig::sin(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_x(
                            (trig::cos(f64::from(tc * 0.1)) * 180.0).to_radians(),
                        )
                        * glam::Mat4::from_rotation_z(
                            (trig::sin(f64::from(tc * 0.15)) * 360.0).to_radians(),
                        )
                        * glam::Mat4::from_scale(glam::Vec3::new(-0.5, -0.5, 0.5));
                    let uv = [r.u0, r.v0, r.u1, r.v1];
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        uv,
                        0xFFFF_FFFF,
                        light,
                        0,
                        out,
                    );
                    // The translucent shell blends over the opaque core.
                    horizon_client::mobs::render::draw(
                        model,
                        base * glam::Mat4::from_scale(glam::Vec3::splat(1.5)),
                        &poses,
                        uv,
                        0x26FF_FFFF,
                        light,
                        0,
                        out,
                    );
                }
                continue;
            }
            // WitherSkullRenderer: the skull model facing its flight.
            if let Some(dangerous) = super::wither::skull_kind(a) {
                static SKULL: std::sync::OnceLock<Option<horizon_client::mobs::Part>> =
                    std::sync::OnceLock::new();
                let model = SKULL.get_or_init(|| {
                    horizon_client::mobs::load_model(
                        &self.options.resources,
                        "minecraft:wither_skull#main",
                    )
                });
                let tex = if dangerous {
                    "entity/wither/wither_invulnerable"
                } else {
                    "entity/wither/wither"
                };
                if let (Some(model), Some(r)) = (model, atlas.sprites.get(tex).copied()) {
                    let mut poses = horizon_client::mobs::anim::Poses::new(model);
                    poses.yrot("head", a.yaw.to_radians());
                    poses.xrot("head", a.pitch.to_radians());
                    let base = glam::Mat4::from_translation(c)
                        * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
                    horizon_client::mobs::render::draw(
                        model,
                        base,
                        &poses,
                        [r.u0, r.v0, r.u1, r.v1],
                        0xFFFF_FFFF,
                        packed,
                        0,
                        out,
                    );
                }
                continue;
            }
            if let Some(stack) = a.potion_item() {
                let owner = super::item_draw::owner_at(at, 0.0, "minecraft:splash_potion");
                let state = self.resolve_item(&stack, DisplayContext::Ground, Some(&owner), 0);
                let pose = glam::Mat4::from_translation(c)
                    * glam::Mat4::from_quat(self.camera_orientation());
                self.submit_item_world(&state, pose, packed, out);
                continue;
            }
            let Some(name) = a.item_sprite() else {
                continue;
            };
            let Some(r) = atlas.sprites.get(name).copied() else {
                continue;
            };
            let s = 0.25 * a.sprite_scale();
            for (k, uv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .zip([[r.u0, r.v1], [r.u1, r.v1], [r.u1, r.v0], [r.u0, r.v0]])
            {
                let v = c + right * k.0 * s + up * k.1 * s;
                out.push(Vertex {
                    normal: [0; 4],
                    pos: v.to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light: packed,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
        if let Some((start, rgb)) = glowing {
            self.mark_outline(&out[start..], rgb);
        }
    }
}
impl crate::game::Game<'_> {
    pub(super) fn player_view(&self) -> Option<PlayerView> {
        let p = self.player.as_ref()?;
        let w = f64::from(p.dimensions.width) / 2.0;
        let bounds = Aabb::new(
            Vec3::new(p.position.x - w, p.position.y, p.position.z - w),
            Vec3::new(
                p.position.x + w,
                p.position.y + f64::from(p.dimensions.height),
                p.position.z + w,
            ),
        );
        Some(PlayerView {
            position: p.position,
            bounds,
            eye: [
                p.position.x,
                p.position.y + f64::from(p.dimensions.eye_height),
                p.position.z,
            ],
            alive: !p.health.dead_or_dying(),
            // EntitySelector.NO_CREATIVE_OR_SPECTATOR.
            creative: self.creative || self.spectator(),
            sneaking: self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
            swimming: p.swimming,
            look: view_vector(self.yaw, self.pitch),
            main_hand: self.held().map(|s| s.item.clone()),
            off_hand: None,
            head: self.armor[0].as_ref().map(|s| s.item.clone()),
            armor_cover: self.armor.iter().filter(|a| a.is_some()).count() as f32 / 4.0,
            invisible: self.potions.effects.get("invisibility").is_some(),
            spectator: self.spectator(),
            gold_armor: self.wearing_gold(),
            hurt_by: self.player_hurt_by(),
            last_hurt: self.combat.last_hurt_mob.map(|(id, at)| (Who::Mob(id), at)),
            velocity: p.velocity,
            yaw: self.yaw,
            sprinting: p.sprinting,
            riding: self.riding.is_some(),
            hero: self.potions.effects.get("hero_of_the_village").is_some(),
            effects: self.potions.effects.list.iter().map(|e| e.id).collect(),
            uuid: self.level.player_uuid.unwrap_or_default(),
            allies: std::sync::Arc::new(self.player_allies()),
            remote: false,
            sleeping: self.sleeping.is_some(),
            sleep_timer: self.sleep_timer,
            passenger: self.riding.is_some(),
        })
    }
    /// The `mobs` command without the server: the mobs within `radius` as
    /// this client has them (what the server sent of them; no AI here).
    pub(super) fn control_mobs(&self, radius: f64) -> serde_json::Value {
        let center = self.player.as_ref().map_or(Vec3::ZERO, |p| p.position);
        let list: Vec<serde_json::Value> = self
            .mobs
            .iter()
            .filter(|m| m.distance_sqr(center) <= radius * radius)
            .map(|m| {
                let mut state = serde_json::json!({
                    "id": m.id,
                    "kind": m.kind.id,
                    "pos": [m.position.x, m.position.y, m.position.z],
                    "health": m.health,
                    "baby": m.is_baby(),
                    "target": m.ai.target.map(|t| format!("{t:?}")),
                    "held": m.extra.main_hand.as_deref(),
                    "mount": m.extra.mount,
                    "rider": m.extra.rider,
                    "saddled": m.extra.saddled,
                    "on_ground": m.on_ground,
                    "variant": m.extra.variant,
                    "color": m.extra.color,
                    "sheared": m.extra.sheared,
                    "burning": m.fire_ticks > 0,
                    "size": m.extra.size,
                    "leash": m.extra.leash.map(|h| format!("{h:?}")),
                    "tame": m.extra.tame,
                    "sitting": m.extra.sitting,
                    "chested": m.extra.chested,
                    "passenger": m.extra.passenger,
                });
                state["guardian_beam"] =
                    serde_json::json!(horizon_client::mobs::guardian::beam(m, 0.0));
                state
            })
            .collect();
        serde_json::json!({ "count": self.mobs.len(), "near": list })
    }
    pub(super) fn mob_world_base<'s>(&'s self, others: &'s [Seen]) -> MobWorld<'s> {
        let (rain, thunder) = self.world.weather.levels(1.0);
        // Level.canHaveWeather: only the overworld's rain and thunder levels
        // ever rise, so nothing rains or thunders in the Nether or the End.
        let weather = self.dim == Dim::Overworld;
        let player = self.player_view();
        MobWorld {
            world: &self.world,
            registry: self.pack.assets.registry,
            facts: self.res.facts(),
            fluid_tables: &self.motion.fluids,
            movement: &self.motion.blocks,
            collision: self.collision,
            data: self.res.mob_data(),
            nearest_player: player.as_ref().filter(|p| !p.spectator).map(|p| p.position),
            player,
            others,
            others_grid: None,
            others_index: None,
            daylight: self.dim != Dim::Nether && self.sky_darken() < 4,
            game_time: self.world.game_time,
            day_time: self.world.day_time,
            raining: weather && rain > 0.2,
            thundering: weather && thunder > 0.9,
            nether: self.dim == Dim::Nether,
            block_tags: &self.res.items().block_tags,
            biomes: self.res.biomes(),
            mob_griefing: self.world_data.rules.bool("mob_griefing"),
            mob_drops: self.world_data.rules.bool("mob_drops"),
            max_entity_cramming: self.world_data.rules.int("max_entity_cramming"),
            end: self.dim == Dim::End,
            world_seed: self.level.seed,
            sea_level: if self.dim == Dim::Nether { 32 } else { 63 },
            teams: std::sync::Arc::new(self.mob_teams()),
            items: Some(self.res.items()),
            // Plugins hear of the damage the surroundings deal: it waits for them.
            defer_env_damage: false,
            lid_openers: &self.lid_openers,
            // The first player in the level: a server has no player of its own.
            difficulty: self.local_difficulty(
                self.level_players()
                    .first()
                    .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]),
            ),
        }
    }
    /// `ServerLevel.getCurrentDifficultyAt`.
    pub(super) fn local_difficulty(&self, at: [f64; 3]) -> spawning::LocalDifficulty {
        use horizon_world::food::Difficulty;
        let base = match self.world.health_rules.difficulty {
            Difficulty::Peaceful => 0,
            Difficulty::Easy => 1,
            Difficulty::Normal => 2,
            Difficulty::Hard => 3,
        };
        let key = ((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
        let (inhabited, moon) = if self.world.chunks.contains_key(&key) {
            let phase = self.world.day_time.div_euclid(24000).rem_euclid(8) as usize;
            (
                self.world.inhabited.get(&key).copied().unwrap_or(0),
                spawning::MOON_BRIGHTNESS[phase],
            )
        } else {
            (0, 0.0)
        };
        spawning::LocalDifficulty::new(base, self.world.day_time, inhabited, moon)
    }
    /// `LivingEntity.makePoofParticles` (entity event 60) about a mob's box.
    pub(super) fn death_poof(&mut self, at: [f64; 3], width: f64, height: f64) {
        let options = horizon_client::particles::ParticleOptions::simple(
            horizon_client::particles::ParticleType::Poof,
        );
        for _ in 0..20 {
            let r = self.particles.level_random();
            let xa = r.next_gaussian() * 0.02;
            let ya = r.next_gaussian() * 0.02;
            let za = r.next_gaussian() * 0.02;
            let x = at[0] + width * (2.0 * r.next_double() - 1.0) - xa * 10.0;
            let y = at[1] + height * r.next_double() - ya * 10.0;
            let z = at[2] + width * (2.0 * r.next_double() - 1.0) - za * 10.0;
            self.particles.add(&options, [x, y, z], [xa, ya, za]);
        }
    }
    /// Adds a projectile, giving it an id from the entity counter
    /// (`Entity.id`, the same counter as the rest) when it has none.
    pub(super) fn add_projectile(&mut self, mut p: horizon_client::projectiles::Projectile) {
        if p.id == 0 {
            self.next_entity += 1;
            p.id = self.next_entity;
        }
        self.creatures.projectiles.push(p);
    }
    /// `Snowball`/`ThrownEgg.handleEntityEvent(3)`: eight crumbs of the
    /// thrown item where it broke, at rest for a snowball, scattered by the
    /// client entity's random (the particle engine's here) for an egg.
    pub(super) fn item_burst(&mut self, at: [f64; 3], item: &str) {
        let options = self.item_particle(&horizon_client::items::ItemStack::new(item, 1));
        let egg = item != "minecraft:snowball";
        for _ in 0..8 {
            let v = if egg {
                let r = self.particles.random();
                [
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                    (f64::from(r.next_float()) - 0.5) * 0.08,
                ]
            } else {
                [0.0; 3]
            };
            self.particles.add(&options, at, v);
        }
    }
    /// A beam target's centre (half its height), interpolated.
    fn beam_target(&self, who: Option<Who>, alpha: f64) -> Option<glam::DVec3> {
        match who? {
            Who::Player => {
                let p = self.player.as_ref()?;
                let (a, b) = (self.previous_position, p.position);
                Some(glam::DVec3::new(
                    a.x + (b.x - a.x) * alpha,
                    a.y + (b.y - a.y) * alpha + f64::from(p.dimensions.height) * 0.5,
                    a.z + (b.z - a.z) * alpha,
                ))
            }
            Who::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                Some(glam::DVec3::new(
                    m.previous.x + (m.position.x - m.previous.x) * alpha,
                    m.previous.y
                        + (m.position.y - m.previous.y) * alpha
                        + f64::from(m.height()) * 0.5,
                    m.previous.z + (m.position.z - m.previous.z) * alpha,
                ))
            }
        }
    }
    /// `Guardian.aiStep` on a client: two bubbles a tick trailing a guardian
    /// that swims along, and bubbles up the beam to its target. The server
    /// never draws them, so the mob's own random stays untouched.
    pub(super) fn guardian_bubbles(&mut self) {
        use horizon_client::mobs::guardian;
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let mut swimming = Vec::new();
        let mut beams = Vec::new();
        for m in &self.mobs {
            if !m.alive() || !matches!(m.species(), Species::Guardian | Species::ElderGuardian) {
                continue;
            }
            let at = [m.position.x, m.position.y, m.position.z];
            if m.extra.flag2 && m.fluids.in_water {
                let (width, height, _) = m.dims();
                swimming.push((at, width, height, m.view_vector()));
            }
            if let Some((_, scale)) = guardian::beam(m, 0.0)
                && let Some(t) = self.beam_target(m.ai.target, 1.0)
            {
                beams.push(([at[0], m.eye_y(), at[2]], [t.x, t.y, t.z], f64::from(scale)));
            }
        }
        let bubble = ParticleOptions::simple(ParticleType::Bubble);
        for (at, width, height, view) in swimming {
            for _ in 0..2 {
                let (x, y, z) = {
                    let r = self.particles.random();
                    (r.next_double(), r.next_double(), r.next_double())
                };
                let p = [
                    at[0] + width * (2.0 * x - 1.0) * 0.5 - view[0] * 1.5,
                    at[1] + height * y - view[1] * 1.5,
                    at[2] + width * (2.0 * z - 1.0) * 0.5 - view[2] * 1.5,
                ];
                self.particles.add(&bubble, p, [0.0; 3]);
            }
        }
        for (eye, target, scale) in beams {
            let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if length <= 0.0 {
                continue;
            }
            let mut dist = self.particles.random().next_double();
            while dist < length {
                let r = self.particles.random();
                dist += 1.8 - scale + r.next_double() * (1.7 - scale);
                let f = dist / length;
                let p = [eye[0] + d[0] * f, eye[1] + d[1] * f, eye[2] + d[2] * f];
                self.particles.add(&bubble, p, [0.0; 3]);
            }
        }
    }
}
impl crate::game::Game<'_> {
    /// A mob's particle burst (hearts, smoke, splashes) around a point.
    pub(super) fn mob_particles(&mut self, kind: &str, at: [f64; 3], count: u32) {
        let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) else {
            return;
        };
        if kind.starts_with("item_") {
            self.ensure_item_particle_sprites();
        }
        let options = horizon_client::particles::ParticleOptions::simple(ty);
        // Count 0: one particle exactly there, at rest.
        if count == 0 {
            self.particles.add(&options, at, [0.0; 3]);
        }
        for _ in 0..count {
            let r = &mut self.world.random;
            let p = [
                at[0] + (f64::from(r.next_float()) - 0.5),
                at[1] + (f64::from(r.next_float()) - 0.5) * 0.5,
                at[2] + (f64::from(r.next_float()) - 0.5),
            ];
            let v = [
                r.next_gaussian() * 0.02,
                r.next_gaussian() * 0.02,
                r.next_gaussian() * 0.02,
            ];
            self.particles.add(&options, p, v);
        }
    }
    /// One particle of a mob's, with its velocity.
    pub(super) fn mob_particle_at(&mut self, kind: &str, at: [f64; 3], velocity: [f64; 3]) {
        if let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) {
            let options = horizon_client::particles::ParticleOptions::simple(ty);
            self.particles.add(&options, at, velocity);
        }
    }
}
