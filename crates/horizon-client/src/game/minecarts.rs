//! Minecarts (`AbstractMinecart` with `OldMinecartBehavior`): following
//! rails and slopes, powered and activator rails, coming off the track,
//! riding with the rider's push, pushing by players and other carts,
//! `MinecartItem` placement, `VehicleEntity` breaking, the chest, hopper,
//! furnace and TNT variants, saving, and `AbstractMinecartRenderer`'s old
//! rail-following render with the display block.
use super::{Game, view_vector};
use horizon_client::{
    blocks::Fluid,
    items::{ItemStack, Items},
    mesher::Vertex,
    mobs::{Part, anim::Poses},
    sound_engine::{CartSound, SoundEngine, SoundInstance, Sounds},
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::registry::BlockStateId;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CartKind {
    Rideable,
    Chest,
    Hopper,
    Furnace,
    Tnt,
    /// `MinecartCommandBlock`; its command block holds no command, so an
    /// activator rail runs nothing.
    CommandBlock,
}
impl CartKind {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id.strip_prefix("minecraft:")? {
            "minecart" => Self::Rideable,
            "chest_minecart" => Self::Chest,
            "hopper_minecart" => Self::Hopper,
            "furnace_minecart" => Self::Furnace,
            "tnt_minecart" => Self::Tnt,
            "command_block_minecart" => Self::CommandBlock,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Rideable => "minecraft:minecart",
            Self::Chest => "minecraft:chest_minecart",
            Self::Hopper => "minecraft:hopper_minecart",
            Self::Furnace => "minecraft:furnace_minecart",
            Self::Tnt => "minecraft:tnt_minecart",
            Self::CommandBlock => "minecraft:command_block_minecart",
        }
    }
    fn slots(self) -> usize {
        match self {
            Self::Chest => 27,
            Self::Hopper => 5,
            _ => 0,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Minecart {
    pub id: u32,
    pub kind: CartKind,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub previous_yaw: f32,
    pub(super) on_ground: bool,
    pub hurt_time: i32,
    pub hurt_dir: i32,
    pub damage: f32,
    pub items: Vec<Option<ItemStack>>,
    /// Furnace minecarts: fuel ticks and the push direction.
    pub fuel: i32,
    /// TNT minecarts: ticks until the explosion, -1 when not primed.
    pub fuse: i32,
}
pub(super) const WIDTH: f64 = 0.98;
pub(super) const HEIGHT: f64 = 0.7;
/// AbstractMinecart.getDefaultDisplayOffset, in sixteenths of a block.
fn display_offset(kind: CartKind) -> f64 {
    if kind == CartKind::Hopper { 1.0 } else { 6.0 }
}
/// The minecart's passenger attachment minus Player.DEFAULT_VEHICLE_ATTACHMENT.
pub(super) const SEAT: f64 = 0.1875 - 0.6;
/// The placeholder container key under which an open cart's items live.
/// The y the server's menus give a vehicle's container key (no block is there).
pub(super) const CART_KEY_Y: i32 = -100_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Shape {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}
impl Shape {
    fn parse(s: &str) -> Self {
        match s {
            "east_west" => Self::EastWest,
            "ascending_east" => Self::AscendingEast,
            "ascending_west" => Self::AscendingWest,
            "ascending_north" => Self::AscendingNorth,
            "ascending_south" => Self::AscendingSouth,
            "south_east" => Self::SouthEast,
            "south_west" => Self::SouthWest,
            "north_west" => Self::NorthWest,
            "north_east" => Self::NorthEast,
            _ => Self::NorthSouth,
        }
    }
    pub(super) fn is_slope(self) -> bool {
        matches!(
            self,
            Self::AscendingEast | Self::AscendingWest | Self::AscendingNorth | Self::AscendingSouth
        )
    }
    /// `AbstractMinecart.EXITS`.
    fn exits(self) -> ([i32; 3], [i32; 3]) {
        let (w, e, n, s) = ([-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]);
        let below = |v: [i32; 3]| [v[0], -1, v[2]];
        match self {
            Self::NorthSouth => (n, s),
            Self::EastWest => (w, e),
            Self::AscendingEast => (below(w), e),
            Self::AscendingWest => (w, below(e)),
            Self::AscendingNorth => (n, below(s)),
            Self::AscendingSouth => (below(n), s),
            Self::SouthEast => (s, e),
            Self::SouthWest => (s, w),
            Self::NorthWest => (n, w),
            Self::NorthEast => (n, e),
        }
    }
}
pub(super) struct Rail {
    pub(super) shape: Shape,
}
impl Minecart {
    pub fn new(id: u32, kind: CartKind, position: Vec3) -> Self {
        Self {
            id,
            kind,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            previous_yaw: 0.0,
            on_ground: false,
            hurt_time: 0,
            hurt_dir: 1,
            damage: 0.0,
            items: vec![None; kind.slots()],
            fuel: 0,
            fuse: -1,
        }
    }
    pub fn bounds(&self) -> Aabb {
        let (w, p) = (WIDTH / 2.0, self.position);
        Aabb::new(
            Vec3::new(p.x - w, p.y, p.z - w),
            Vec3::new(p.x + w, p.y + HEIGHT, p.z + w),
        )
    }
    /// `AbstractContainerMenu.getRedstoneSignalFromContainer` of `slots`.
    pub(super) fn container_signal(slots: &[Option<ItemStack>], items: &Items) -> i32 {
        let mut total = 0.0f32;
        for s in slots.iter().flatten() {
            let max = items.get(&s.item).map_or(64, |i| i.max_stack).min(64);
            total += s.count as f32 / max as f32;
        }
        total /= slots.len().max(1) as f32;
        // Mth.lerpDiscrete(total, 0, 15)
        (total * 14.0).floor() as i32 + i32::from(total > 0.0)
    }
}
pub(super) struct CartModels {
    pub models: HashMap<CartKind, Part>,
}
impl CartModels {
    pub fn load(document: Option<&horizon_client::mobs::EntityModelDocument>) -> Self {
        let kinds = [
            CartKind::Rideable,
            CartKind::Chest,
            CartKind::Hopper,
            CartKind::Furnace,
            CartKind::Tnt,
            CartKind::CommandBlock,
        ];
        let models = kinds
            .into_iter()
            .filter_map(|k| document?.model(&format!("{}#main", k.id())).map(|m| (k, m)))
            .collect();
        Self { models }
    }
}
impl Game<'_> {
    /// `ClientPacketListener.postAddEntitySoundInstance` for the carts that
    /// joined the level since the last tick, and `LocalPlayer.startRiding`'s
    /// two inside sounds when the player got in one; then what the sound
    /// instances read this tick (the ridden cart and every cart).
    pub(super) fn start_cart_sounds(&mut self) -> (Option<u32>, Vec<CartSound>) {
        let carts: Vec<CartSound> = self
            .carts
            .iter()
            .map(|c| CartSound {
                id: c.id,
                position: [c.position.x, c.position.y, c.position.z],
                // Vec3.horizontalDistance.
                speed: (c.velocity.x * c.velocity.x + c.velocity.z * c.velocity.z).sqrt(),
            })
            .collect();
        self.cart_sounds
            .retain(|id| carts.iter().any(|c| c.id == *id));
        for cart in &carts {
            if self.cart_sounds.insert(cart.id) {
                let instance = SoundInstance::minecart(cart, SoundEngine::unseeded());
                self.audio.play_instance(instance, false);
            }
        }
        let vehicle = self.riding.filter(|id| carts.iter().any(|c| c.id == *id));
        if vehicle != self.riding_cart_sounds
            && let Some(cart) = carts.iter().find(|c| Some(c.id) == vehicle)
        {
            for underwater in [true, false] {
                let instance =
                    SoundInstance::riding_minecart(cart, underwater, SoundEngine::unseeded());
                self.audio.play_instance(instance, false);
            }
        }
        self.riding_cart_sounds = vehicle;
        (vehicle, carts)
    }
    /// Baked block faces with terrain shade or block-item diffuse lighting.
    pub(super) fn block_model_vertices(
        &self,
        state: BlockStateId,
        seed: i64,
        m: glam::Mat4,
        light: u32,
        diffuse: bool,
        out: &mut Vec<Vertex>,
    ) {
        let Ok(models) = self.pack.render.model_catalog.collect_seeded(state, seed) else {
            return;
        };
        for (_, model) in &models {
            for face in &model.faces {
                let Some(s) = self
                    .pack
                    .layer_names
                    .get(&face.layer)
                    .and_then(|n| self.gui.atlas.sprites.get(n))
                else {
                    continue;
                };
                use horizon_assets::model::Direction as D;
                let shade: u32 = match face.quad.facing {
                    D::Up => 255,
                    D::Down => 127,
                    D::North | D::South => 204,
                    D::West | D::East => 153,
                };
                let color = if diffuse {
                    u32::MAX
                } else {
                    0xFF00_0000 | shade << 16 | shade << 8 | shade
                };
                let normal = if diffuse {
                    use horizon_client::item_render::{direction_vector, transform_normal};
                    let n = transform_normal(m, direction_vector(face.quad.facing));
                    let n = n.to_array().map(|v| (v.clamp(-1.0, 1.0) * 127.0) as i8);
                    [n[0], n[1], n[2], 127]
                } else {
                    [0; 4]
                };
                let light = if diffuse {
                    let emission = u32::from(face.quad.light_emission) * 16;
                    (light & 0xFF) | (light >> 8 & 0xFF).max(emission) << 8
                } else {
                    light
                };
                for k in 0..4 {
                    let p = face.quad.positions[k];
                    let uv = face.quad.uv[k];
                    out.push(Vertex {
                        normal,
                        pos: m.transform_point3(glam::Vec3::from_array(p)).to_array(),
                        uv: [s.u0 + uv[0] * (s.u1 - s.u0), s.v0 + uv[1] * (s.v1 - s.v0)],
                        layer: 0,
                        color,
                        light,
                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
    /// `AbstractMinecartRenderer` (old render): on the rail between the
    /// front and back points, pitched on slopes, with the display block.
    pub(super) fn cart_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let a = alpha as f32;
        let Some(uv) = self
            .gui
            .atlas
            .sprites
            .get("entity/minecart/minecart")
            .map(|r| [r.u0, r.v0, r.u1, r.v1])
        else {
            return;
        };
        // Entity.shouldRenderAtSqrDistance: 64 times the bounding box's mean
        // edge, scaled by the view scale.
        let range = (WIDTH + HEIGHT + WIDTH) / 3.0 * 64.0 * self.view_scale();
        for c in &self.carts {
            let entity_start = out.len();
            let Some(model) = self.cart_models.models.get(&c.kind) else {
                continue;
            };
            let (x, y, z) = (
                c.previous.x + (c.position.x - c.previous.x) * alpha,
                c.previous.y + (c.position.y - c.previous.y) * alpha,
                c.previous.z + (c.position.z - c.previous.z) * alpha,
            );
            // LevelExtractor.isEntityVisible: the cart the player rides always draws.
            if self.riding != Some(c.id) {
                let d = glam::DVec3::new(c.position.x, c.position.y, c.position.z) - camera;
                if d.length_squared() >= range * range {
                    continue;
                }
                // EntityRenderer.shouldRender: the interpolated box grown by 0.5;
                // AbstractMinecartRenderer.getBoundingBoxForCulling stretches it
                // up by the display block's offset.
                if let Some(view) = &self.view_frustum {
                    let top = HEIGHT
                        + if c.kind == CartKind::Rideable {
                            0.0
                        } else {
                            display_offset(c.kind) * 0.75 / 16.0
                        };
                    let p = glam::DVec3::new(x, y, z) - camera;
                    let lo = p - glam::DVec3::new(WIDTH / 2.0 + 0.5, 0.5, WIDTH / 2.0 + 0.5);
                    let hi = p + glam::DVec3::new(WIDTH / 2.0 + 0.5, top + 0.5, WIDTH / 2.0 + 0.5);
                    if !view.intersects(lo.as_vec3(), hi.as_vec3()) {
                        continue;
                    }
                }
            }
            let mut yaw =
                c.previous_yaw + ((c.yaw - c.previous_yaw + 180.0).rem_euclid(360.0) - 180.0) * a;
            let mut pitch = 0.0f32;
            let mut offset = glam::DVec3::ZERO;
            if let Some(on) = self.rail_pos(x, y, z) {
                let front = self.rail_pos_offs(x, y, z, 0.3).unwrap_or(on);
                let back = self.rail_pos_offs(x, y, z, -0.3).unwrap_or(on);
                offset = glam::DVec3::new(on[0] - x, (front[1] + back[1]) / 2.0 - y, on[2] - z);
                let d =
                    glam::DVec3::new(back[0] - front[0], back[1] - front[1], back[2] - front[2]);
                if d.length() != 0.0 {
                    let d = d.normalize();
                    yaw = d.z.atan2(d.x).to_degrees() as f32;
                    pitch = (d.y.atan() * 73.0) as f32;
                }
            }
            // The per-entity jitter (offsetSeed) that keeps stacked carts from z-fighting.
            let seed = i64::from(c.id).wrapping_mul(493_286_711);
            let seed = seed
                .wrapping_mul(seed)
                .wrapping_mul(4_392_167_121)
                .wrapping_add(seed.wrapping_mul(98_761));
            let jitter = |shift: i64| (((seed >> shift & 7) as f32 + 0.5) / 8.0 - 0.5) * 0.004;
            let p = glam::DVec3::new(x, y, z) + offset - camera;
            let mut base = glam::Mat4::from_translation(
                p.as_vec3() + glam::Vec3::new(jitter(16), jitter(20) + 0.375, jitter(24)),
            ) * glam::Mat4::from_rotation_y((180.0 - yaw).to_radians())
                * glam::Mat4::from_rotation_z((-pitch).to_radians());
            let hurt = c.hurt_time as f32 - a;
            if hurt > 0.0 {
                let damage = (c.damage - a).max(0.0);
                base *= glam::Mat4::from_rotation_x(
                    (hurt.sin() * hurt * damage / 10.0 * c.hurt_dir as f32).to_radians(),
                );
            }
            let l = self.world.light(
                c.position.x.floor() as i32,
                (c.position.y + 0.35).floor() as i32,
                c.position.z.floor() as i32,
            );
            let packed = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
            if let Some(state) = self.display_state(c) {
                let display = base
                    * glam::Mat4::from_scale(glam::Vec3::splat(0.75))
                    * glam::Mat4::from_translation(glam::Vec3::new(
                        -0.5,
                        (display_offset(c.kind) as f32 - 8.0) / 16.0,
                        0.5,
                    ))
                    * glam::Mat4::from_rotation_y(90f32.to_radians());
                if c.kind == CartKind::Chest {
                    self.cart_chest(state, display, l, out);
                } else {
                    // TNT minecarts flash white as the fuse runs out (TntMinecartRenderer).
                    let swell = if c.kind == CartKind::Tnt && c.fuse > -1 {
                        let fuse = c.fuse as f32 - a + 1.0;
                        if fuse < 10.0 {
                            let g = (1.0 - fuse / 10.0).clamp(0.0, 1.0);
                            1.0 + g * g * g * g * 0.3
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    };
                    let m = display
                        * glam::Mat4::from_translation(glam::Vec3::splat(0.5))
                        * glam::Mat4::from_scale(glam::Vec3::splat(swell))
                        * glam::Mat4::from_translation(glam::Vec3::splat(-0.5));
                    let flash = c.kind == CartKind::Tnt && c.fuse > -1 && (c.fuse / 5) % 2 == 0;
                    let start = out.len();
                    self.block_model_vertices(state, 0, m, packed, false, out);
                    if flash {
                        // submitWhiteSolidBlock: OverlayTexture.u(1.0) on the cart's own light.
                        for v in &mut out[start..] {
                            v.layer = 5;
                        }
                    }
                }
            }
            let m = base * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
            let poses = Poses::new(model);
            horizon_client::mobs::render::draw(model, m, &poses, uv, 0xFFFF_FFFF, packed, 0, out);
            self.cull_entity_vertices(out, entity_start);
        }
    }
    /// The chest inside a chest minecart, drawn by the chest block-entity renderer.
    fn cart_chest(&self, state: BlockStateId, m: glam::Mat4, light: u8, out: &mut Vec<Vertex>) {
        let lid = |_: [i32; 3]| 0.0;
        let scene = horizon_client::block_entity_render::Scene {
            camera: glam::DVec3::ZERO,
            orientation: glam::Quat::IDENTITY,
            anim: &|_| horizon_client::block_entity_render::BlockAnim::default(),
            texts: None,
            partial: 0.0,
            game_time: self.world.game_time,
            max_y: self.dim.max_y(),
            world: &self.world,
            sprites: &self.gui.atlas.sprites,
            nether: self.dim == super::Dim::Nether,
            lid: &lid,
            sign: &|_| None,
            banner: &|_| Vec::new(),
            skull: &|_| None,
            glyph: &|c| self.gui.atlas.glyph_cell(c),
            glyphs: self.gui.atlas.font.epoch(),
        };
        self.res
            .block_entity_render()
            .render_state(&scene, state.0, m, light, out);
    }
}
impl crate::game::Game<'_> {
    pub(super) fn rail(&self, x: i32, y: i32, z: i32) -> Option<Rail> {
        let state = self.world.block(x, y, z);
        let name = self.res.facts().get(state).name.clone();
        if !matches!(
            &*name,
            "minecraft:rail"
                | "minecraft:powered_rail"
                | "minecraft:detector_rail"
                | "minecraft:activator_rail"
        ) {
            return None;
        }
        let s = self.pack.assets.registry.state(state)?;
        let shape = Shape::parse(s.properties.get("shape").map_or("", String::as_str));
        let _powered = s.properties.get("powered").is_some_and(|v| v == "true");
        Some(Rail { shape })
    }
    /// `getCurrentBlockPosOrRailBelow`.
    fn rail_block(&self, p: Vec3) -> [i32; 3] {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        if self.rail(x, y - 1, z).is_some() {
            [x, y - 1, z]
        } else {
            [x, y, z]
        }
    }
    /// `OldMinecartBehavior.getPos`: the point on the rail under (x, y, z).
    fn rail_pos(&self, x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
        let [xt, yt, zt] = self.rail_block(Vec3::new(x, y, z));
        let rail = self.rail(xt, yt, zt)?;
        let (e0, e1) = rail.shape.exits();
        let (fx, fy, fz) = (f64::from(xt), f64::from(yt), f64::from(zt));
        let x0 = fx + 0.5 + f64::from(e0[0]) * 0.5;
        let y0 = fy + 0.0625 + f64::from(e0[1]) * 0.5;
        let z0 = fz + 0.5 + f64::from(e0[2]) * 0.5;
        let x1 = fx + 0.5 + f64::from(e1[0]) * 0.5;
        let y1 = fy + 0.0625 + f64::from(e1[1]) * 0.5;
        let z1 = fz + 0.5 + f64::from(e1[2]) * 0.5;
        let (xd, yd, zd) = (x1 - x0, (y1 - y0) * 2.0, z1 - z0);
        let progress = if xd == 0.0 {
            z - fz
        } else if zd == 0.0 {
            x - fx
        } else {
            ((x - x0) * xd + (z - z0) * zd) * 2.0
        };
        let mut out = [x0 + xd * progress, y0 + yd * progress, z0 + zd * progress];
        if yd < 0.0 {
            out[1] += 1.0;
        } else if yd > 0.0 {
            out[1] += 0.5;
        }
        Some(out)
    }
    /// `OldMinecartBehavior.getPosOffs`.
    fn rail_pos_offs(&self, mut x: f64, y: f64, mut z: f64, offs: f64) -> Option<[f64; 3]> {
        let [xt, yt, zt] = self.rail_block(Vec3::new(x, y, z));
        let rail = self.rail(xt, yt, zt)?;
        let mut y = f64::from(yt) + if rail.shape.is_slope() { 1.0 } else { 0.0 };
        let (e0, e1) = rail.shape.exits();
        let (mut xd, mut zd) = (f64::from(e1[0] - e0[0]), f64::from(e1[2] - e0[2]));
        let dd = xd.hypot(zd);
        xd /= dd;
        zd /= dd;
        x += xd * offs;
        z += zd * offs;
        let at = |e: [i32; 3]| x.floor() as i32 - xt == e[0] && z.floor() as i32 - zt == e[2];
        if e0[1] != 0 && at(e0) {
            y += f64::from(e0[1]);
        } else if e1[1] != 0 && at(e1) {
            y += f64::from(e1[1]);
        }
        self.rail_pos(x, y, z)
    }
    /// `getDismountLocationForPassenger`: beside the cart (across its facing), else on top.
    pub(super) fn dismount_cart(&mut self, i: usize) {
        self.riding = None;
        let c = &self.carts[i];
        let (bx, by, bz) = (
            c.position.x.floor() as i32,
            c.position.y.floor() as i32,
            c.position.z.floor() as i32,
        );
        let mut target = None;
        // DismountHelper.offsetsForDirection of the cart's facing
        // (Direction.fromYRot of its yaw): right, left, back right, back left,
        // forward right, forward left, back, forward.
        let step = |q: i32| [(0, 1), (-1, 0), (0, -1), (1, 0)][(q & 3) as usize];
        let facing = (f64::from(c.yaw) / 90.0 + 0.5).floor() as i32;
        let (fx, fz) = step(facing);
        let (rx, rz) = (-fz, fx);
        let offsets = [
            (rx, rz),
            (-rx, -rz),
            (rx - fx, rz - fz),
            (-rx - fx, -rz - fz),
            (fx + rx, fz + rz),
            (fx - rx, fz - rz),
            (-fx, -fz),
            (fx, fz),
        ];
        'search: for dy in [0, 1, -1] {
            for (dx, dz) in offsets {
                let (x, y, z) = (bx + dx, by + dy, bz + dz);
                let below = self.res.facts().get(self.world.block(x, y - 1, z));
                let here = self.res.facts().get(self.world.block(x, y, z));
                let head = self.res.facts().get(self.world.block(x, y + 1, z));
                if (below.full_collision || below.legacy_solid)
                    && !here.full_collision
                    && !head.full_collision
                    && here.fluid == Fluid::None
                {
                    target = Some(Vec3::new(
                        f64::from(x) + 0.5,
                        f64::from(y),
                        f64::from(z) + 0.5,
                    ));
                    break 'search;
                }
            }
        }
        let top = c.bounds().max_position().y;
        let target = target.unwrap_or(Vec3::new(c.position.x, top, c.position.z));
        if let Some(p) = self.player.as_mut() {
            p.position = target;
            p.velocity = Vec3::ZERO;
        }
    }
    pub(super) fn cart_hit(&self) -> Option<(usize, f64)> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64)> = None;
        for (i, c) in self.carts.iter().enumerate() {
            if Some(c.id) == self.riding {
                continue;
            }
            let bb = c.bounds();
            let (lo, hi) = (bb.min_position(), bb.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for a in 0..3 {
                if dir[a].abs() < 1e-9 {
                    hit &= eye[a] >= lo[a] && eye[a] <= hi[a];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((i, t0));
            }
        }
        best
    }
    /// The open cart's kind when a cart container is shown.
    pub(super) fn open_cart_kind(&self) -> Option<CartKind> {
        let key = self.container.filter(|k| k.1 == CART_KEY_Y)?;
        // A chest boat's chest shows like a chest minecart's.
        self.carts
            .iter()
            .find(|c| c.id as i32 == key.2)
            .map(|c| c.kind)
            .or_else(|| {
                self.boats
                    .iter()
                    .any(|b| b.chest && b.id as i32 == key.2)
                    .then_some(CartKind::Chest)
            })
    }
    fn display_state(&self, c: &Minecart) -> Option<BlockStateId> {
        let registry = &self.pack.assets.registry;
        let with = |name: &str, props: &[(&str, &str)]| {
            let mut s = registry.state(registry.default_state(name)?)?.clone();
            for (k, v) in props {
                s.properties.insert((*k).to_owned(), (*v).to_owned());
            }
            registry.id(&s)
        };
        match c.kind {
            CartKind::Rideable => None,
            CartKind::Chest => with("minecraft:chest", &[("facing", "north")]),
            CartKind::Hopper => registry.default_state("minecraft:hopper"),
            CartKind::Furnace => with(
                "minecraft:furnace",
                &[
                    ("facing", "north"),
                    ("lit", if c.fuel > 0 { "true" } else { "false" }),
                ],
            ),
            CartKind::Tnt => registry.default_state("minecraft:tnt"),
            CartKind::CommandBlock => registry.default_state("minecraft:command_block"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_offset_matches_vanilla_for_each_cart_kind() {
        assert_eq!(display_offset(CartKind::Hopper), 1.0);
        assert_eq!(display_offset(CartKind::Chest), 6.0);
        assert_eq!(display_offset(CartKind::Furnace), 6.0);
        assert_eq!(display_offset(CartKind::Tnt), 6.0);
        assert_eq!(display_offset(CartKind::CommandBlock), 6.0);
        assert_eq!(display_offset(CartKind::Rideable), 6.0);
    }
}
