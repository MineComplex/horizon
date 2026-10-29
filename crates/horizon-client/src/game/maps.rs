//! Maps (`EmptyMapItem`, `MapItem`, `MapItemSavedData`, `MapRenderer`):
//! creating a map centred on the player's grid cell, revealing it while held
//! (`MapItem.update`: dominant map colour per pixel, height and water-depth
//! shading, the dithered edge), the player marker, `data/minecraft/maps/<id>.dat` and
//! `maps/last_id.dat`, the 128x128 texture in an atlas slot, the two-handed
//! first-person view and maps in item frames.
use super::Game;
use horizon_client::{
    blocks::MAP_COLORS,
    hud::{MAP_SLOTS, Rect},
    items::ItemStack,
    mesher::Vertex,
};
use horizon_world::nbt::Tag;
use std::collections::HashMap;

/// A decoration: sprite, map position (-128..127, half pixels) and rotation
/// (0..15), and the player it marks (an item's own decorations mark none).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Decoration {
    pub sprite: &'static str,
    pub x: i8,
    pub y: i8,
    pub rot: u8,
    pub holder: Option<u32>,
}
/// A player carrying a map, or seeing it framed
/// (`MapItemSavedData.HoldingPlayer`): the column sweep's step, and what
/// changed since the last `ClientboundMapItemDataPacket` it was sent.
struct Holder {
    /// The changed pixels' box (min x, min y, max x, max y).
    dirty: Option<[i32; 4]>,
}
pub(super) struct MapData {
    pub center: [i32; 2],
    pub scale: u8,
    pub dimension: String,
    pub locked: bool,
    pub unlimited: bool,
    pub colors: Vec<u8>,
    pub decorations: Vec<Decoration>,
    holders: Vec<Holder>,
    /// Changed since the last save / texture upload.
    dirty: bool,
    texture_dirty: bool,
}
impl MapData {
    /// `MapItemSavedData.createFresh`: centred on the scale's grid cell.
    pub(super) fn fresh(x: f64, z: f64, scale: u8, dimension: &str) -> Self {
        let size = 128 * (1 << scale);
        let cell = |v: f64| (v.floor() as i32 + 64).div_euclid(size) * size + size / 2 - 64;
        Self {
            center: [cell(x), cell(z)],
            scale,
            dimension: dimension.to_owned(),
            locked: false,
            unlimited: false,
            colors: vec![0; 128 * 128],
            decorations: Vec::new(),
            holders: Vec::new(),
            dirty: true,
            texture_dirty: true,
        }
    }
    fn update_color(&mut self, x: i32, y: i32, color: u8) -> bool {
        let i = (x + y * 128) as usize;
        if self.colors[i] == color {
            return false;
        }
        self.colors[i] = color;
        self.dirty = true;
        self.texture_dirty = true;
        // setColorsDirty: every holder's next packet carries it.
        for h in &mut self.holders {
            h.dirty = Some(match h.dirty {
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                None => [x, y, x, y],
            });
        }
        true
    }
    fn from_nbt(tag: &Tag) -> Option<Self> {
        let d = tag.get("data")?;
        let int = |k: &str| match d.get(k) {
            Some(Tag::Int(v)) => Some(*v),
            Some(Tag::Byte(v)) => Some(i32::from(*v)),
            _ => None,
        };
        let dimension = match d.get("dimension") {
            Some(Tag::String(s)) => s.to_string_lossy(),
            _ => "minecraft:overworld".to_owned(),
        };
        let mut colors = match d.get("colors") {
            Some(Tag::ByteArray(c)) => c.clone(),
            _ => Vec::new(),
        };
        colors.resize(128 * 128, 0);
        Some(Self {
            center: [int("xCenter")?, int("zCenter")?],
            scale: int("scale").unwrap_or(0).clamp(0, 4) as u8,
            dimension,
            locked: int("locked").unwrap_or(0) != 0,
            unlimited: int("unlimitedTracking").unwrap_or(0) != 0,
            colors,
            decorations: Vec::new(),
            holders: Vec::new(),
            dirty: false,
            texture_dirty: true,
        })
    }
}
#[derive(Default)]
pub(super) struct Maps {
    pub data: HashMap<i32, MapData>,
    /// The highest map id handed out (`maps/last_id.dat`), loaded on first use.
    last_id: Option<i32>,
    /// (map id, atlas slot), most recently shown last.
    slots: Vec<(i32, usize)>,
    missing: std::collections::HashSet<i32>,
}
/// `MapColor.getColorFromPackedId` as RGBA bytes (NONE is transparent).
fn packed_rgba(packed: u8) -> [u8; 4] {
    let id = usize::from(packed >> 2);
    if id == 0 || id >= MAP_COLORS.len() {
        return [0; 4];
    }
    let modifier = [180u32, 220, 255, 135][usize::from(packed & 3)];
    let col = MAP_COLORS[id];
    let ch = |s: u32| ((col >> s & 255) * modifier / 255) as u8;
    [ch(16), ch(8), ch(0), 255]
}
/// `has(DataComponents.MAP_ID)`: filled maps and the explorer map items
/// (`buried_treasure_map`, `woodland_mansion_map`, ..., all `MapItem`s).
pub(super) fn map_id(stack: &ItemStack) -> Option<i32> {
    stack.data().and_then(|d| d.map_id)
}
/// `MapDecorationTypes` Horizon draws: (type, sprite).
const DECORATION_TYPES: [(&str, &str); 24] = [
    ("player", "map/decorations/player"),
    ("frame", "map/decorations/frame"),
    ("red_marker", "map/decorations/red_marker"),
    ("blue_marker", "map/decorations/blue_marker"),
    ("target_x", "map/decorations/target_x"),
    ("target_point", "map/decorations/target_point"),
    ("player_off_map", "map/decorations/player_off_map"),
    ("player_off_limits", "map/decorations/player_off_limits"),
    ("mansion", "map/decorations/woodland_mansion"),
    ("monument", "map/decorations/ocean_monument"),
    ("red_x", "map/decorations/red_x"),
    ("village_desert", "map/decorations/desert_village"),
    ("village_plains", "map/decorations/plains_village"),
    ("village_savanna", "map/decorations/savanna_village"),
    ("village_snowy", "map/decorations/snowy_village"),
    ("village_taiga", "map/decorations/taiga_village"),
    ("jungle_temple", "map/decorations/jungle_temple"),
    ("swamp_hut", "map/decorations/swamp_hut"),
    ("trial_chambers", "map/decorations/trial_chambers"),
    ("abandoned_camp", "map/decorations/abandoned_camp"),
    ("ancient_city", "map/decorations/ancient_city"),
    ("desert_pyramid", "map/decorations/desert_pyramid"),
    ("mineshaft", "map/decorations/mineshaft"),
    ("ocean_ruin_warm", "map/decorations/warm_ocean_ruins"),
];
/// `MapDecorationType.showOnItemFrame` is false for the player arrows and the two markers.
fn shown_on_frame(sprite: &str) -> bool {
    !matches!(
        decoration_type(sprite),
        Some("player" | "red_marker" | "blue_marker" | "player_off_map" | "player_off_limits")
    )
}
/// The sprite of a decoration type that a `minecraft:map_decorations`
/// entry or a server's packet names.
fn decoration_sprite(id: &str) -> Option<&'static str> {
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    DECORATION_TYPES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, sprite)| *sprite)
}
/// The type a decoration's sprite stands for, as its registry names it.
fn decoration_type(sprite: &str) -> Option<&'static str> {
    DECORATION_TYPES
        .iter()
        .find(|(_, s)| *s == sprite)
        .map(|(name, _)| *name)
}
/// `MapItemSavedData.addTargetDecoration`: the `"+"` entry of
/// `minecraft:map_decorations`, rotation 180.
fn target_decoration(decoration: &str, at: [i32; 3]) -> Tag {
    Tag::compound([(
        "+",
        Tag::compound([
            ("type", Tag::String(decoration.into())),
            ("x", Tag::Double(f64::from(at[0]).to_bits())),
            ("z", Tag::Double(f64::from(at[2]).to_bits())),
            ("rotation", Tag::Float(180.0f32.to_bits())),
        ]),
    )])
}
impl Game<'_> {
    /// The atlas slot showing map `id`, its texture uploaded when it changed.
    pub(super) fn map_texture(&mut self, id: i32) -> Option<Rect> {
        self.map(id)?;
        let slots = &mut self.maps.slots;
        if let Some(i) = slots.iter().position(|e| e.0 == id) {
            let e = slots.remove(i);
            slots.push(e);
        } else {
            // A free slot, else the least recently shown one.
            let slot = if slots.len() < MAP_SLOTS {
                slots.len()
            } else {
                slots.remove(0).1
            };
            slots.push((id, slot));
            if let Some(m) = self.maps.data.get_mut(&id) {
                m.texture_dirty = true;
            }
        }
        let slot = self.maps.slots.last()?.1;
        let rect = self
            .front
            .gui
            .atlas
            .sprites
            .get(&format!("horizon/map{slot}"))
            .copied()?;
        let m = self.maps.data.get_mut(&id)?;
        if m.texture_dirty {
            m.texture_dirty = false;
            let rgba: Vec<u8> = m.colors.iter().flat_map(|&c| packed_rgba(c)).collect();
            let (w, h) = (
                self.front.gui.atlas.width as f32,
                self.front.gui.atlas.height as f32,
            );
            self.front.map_patches.push(horizon_client::font::Patch {
                x: (rect.u0 * w).round() as u32,
                y: (rect.v0 * h).round() as u32,
                width: 128,
                height: 128,
                rgba,
            });
        }
        Some(rect)
    }
    /// `MapRenderer.render`: the map texture and its decorations under `m`
    /// (map pixels 0..128, y down), drawn at depth offsets toward the viewer.
    /// `show_only_frame` (a map in an item frame) leaves out the decorations that
    /// do not show on frames.
    pub(super) fn map_vertices(
        &mut self,
        id: i32,
        show_only_frame: bool,
        m: glam::Mat4,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        let Some(rect) = self.map_texture(id) else {
            return;
        };
        let quad = |corners: [[f32; 3]; 4], uv: [[f32; 2]; 4], out: &mut Vec<Vertex>| {
            for (c, uv) in corners.into_iter().zip(uv) {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: m.transform_point3(glam::Vec3::from_array(c)).to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light,

                    glint_uv: [0.0, 0.0],
                });
            }
        };
        let z = -0.01;
        quad(
            [
                [0.0, 128.0, z],
                [128.0, 128.0, z],
                [128.0, 0.0, z],
                [0.0, 0.0, z],
            ],
            [
                [rect.u0, rect.v1],
                [rect.u1, rect.v1],
                [rect.u1, rect.v0],
                [rect.u0, rect.v0],
            ],
            out,
        );
        let decorations = self
            .maps
            .data
            .get(&id)
            .map(|m| m.decorations.clone())
            .unwrap_or_default();
        let mut count = 0;
        for d in &decorations {
            if show_only_frame && !shown_on_frame(d.sprite) {
                continue;
            }
            let Some(s) = self.front.gui.atlas.sprites.get(d.sprite).copied() else {
                continue;
            };
            let dm = glam::Mat4::from_translation(glam::Vec3::new(
                f32::from(d.x) / 2.0 + 64.0,
                f32::from(d.y) / 2.0 + 64.0,
                -0.02 - count as f32 * 0.001,
            )) * glam::Mat4::from_rotation_z(
                (f32::from(d.rot) * 360.0 / 16.0).to_radians(),
            ) * glam::Mat4::from_scale(glam::Vec3::splat(4.0))
                * glam::Mat4::from_translation(glam::Vec3::new(-0.125, 0.125, 0.0));
            let p = |x: f32, y: f32| dm.transform_point3(glam::Vec3::new(x, y, 0.0)).to_array();
            let mut q = |corners: [[f32; 3]; 4], uv: [[f32; 2]; 4]| {
                for (c, uv) in corners.into_iter().zip(uv) {
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: m.transform_point3(glam::Vec3::from_array(c)).to_array(),
                        uv,
                        layer: 0,
                        color: 0xFFFF_FFFF,
                        light,

                        glint_uv: [0.0, 0.0],
                    });
                }
            };
            q(
                [p(-1.0, 1.0), p(1.0, 1.0), p(1.0, -1.0), p(-1.0, -1.0)],
                [[s.u0, s.v0], [s.u1, s.v0], [s.u1, s.v1], [s.u0, s.v1]],
            );
            count += 1;
        }
    }
    /// `renderTwoHandedMap`: map `id` held in both hands, in view space (the
    /// arms under it are not drawn).
    pub(super) fn two_handed_map_geometry(
        &mut self,
        to_world: glam::Mat4,
        attack: f32,
        inverse_height: f32,
        id: i32,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        let deg = |d: f32| d.to_radians();
        let sqrt_attack = attack.sqrt();
        let y_swing = -0.2 * (attack * std::f32::consts::PI).sin();
        let z_swing = -0.4 * (sqrt_attack * std::f32::consts::PI).sin();
        // calculateMapTilt
        let tilt = (1.0 - self.pitch / 45.0 + 0.1).clamp(0.0, 1.0);
        let tilt = -(tilt * std::f32::consts::PI).cos() * 0.5 + 0.5;
        let base = to_world
            * glam::Mat4::from_translation(glam::Vec3::new(0.0, -y_swing / 2.0, z_swing))
            * glam::Mat4::from_translation(glam::Vec3::new(
                0.0,
                0.04 + inverse_height * -1.2 + tilt * -0.5,
                -0.72,
            ))
            * glam::Mat4::from_rotation_x(deg(tilt * -85.0));
        let swing = (sqrt_attack * std::f32::consts::PI).sin();
        let m = base
            * glam::Mat4::from_rotation_x(deg(swing * 20.0))
            * glam::Mat4::from_scale(glam::Vec3::splat(2.0));
        self.held_map(m, id, light, out);
    }
    /// `renderMap`: map `id` at pose `m`, its background (with a 7-pixel
    /// border) then the map.
    pub(super) fn held_map(&mut self, m: glam::Mat4, id: i32, light: u32, out: &mut Vec<Vertex>) {
        let deg = |d: f32| d.to_radians();
        let m = m
            * glam::Mat4::from_rotation_y(deg(180.0))
            * glam::Mat4::from_rotation_z(deg(180.0))
            * glam::Mat4::from_scale(glam::Vec3::splat(0.38))
            * glam::Mat4::from_translation(glam::Vec3::new(-0.5, -0.5, 0.0))
            * glam::Mat4::from_scale(glam::Vec3::splat(0.0078125));
        let has_data = self.map(id).is_some();
        let background = if has_data {
            "map/map_background_checkerboard"
        } else {
            "map/map_background"
        };
        if let Some(r) = self.front.gui.atlas.sprites.get(background).copied() {
            for (c, uv) in [
                ([-7.0, 135.0, 0.0], [r.u0, r.v1]),
                ([135.0, 135.0, 0.0], [r.u1, r.v1]),
                ([135.0, -7.0, 0.0], [r.u1, r.v0]),
                ([-7.0, -7.0, 0.0], [r.u0, r.v0]),
            ] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: m.transform_point3(glam::Vec3::from_array(c)).to_array(),
                    uv,
                    layer: 0,
                    color: 0xFFFF_FFFF,
                    light,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
        if has_data {
            self.map_vertices(id, false, m, light, out);
        }
    }
}
impl crate::game::Sim<'_> {
    fn dimension_id(&self) -> &'static str {
        match self.dim {
            super::Dim::Nether => "minecraft:the_nether",
            super::Dim::End => "minecraft:the_end",
            _ => "minecraft:overworld",
        }
    }
    fn map_path(&self, name: &str) -> std::path::PathBuf {
        self.world_root.join("data/minecraft/maps").join(name)
    }
    /// The map's data, loaded from `data/minecraft/maps/<id>.dat` on first use.
    pub(super) fn map(&mut self, id: i32) -> Option<&mut MapData> {
        if !self.maps.data.contains_key(&id) && !self.maps.missing.contains(&id) {
            let loaded = horizon_client::save::read_dat(&self.map_path(&format!("{id}.dat")))
                .and_then(|t| MapData::from_nbt(&t));
            match loaded {
                Some(m) => {
                    self.maps.data.insert(id, m);
                }
                None => {
                    self.maps.missing.insert(id);
                }
            }
        }
        self.maps.data.get_mut(&id)
    }
    /// `ClientboundMapItemDataPacket.handle`: the server's colours and
    /// decorations for a map (one carried or framed on a server).
    pub(super) fn remote_map_item_data(
        &mut self,
        p: &horizon_protocol::packets::play::MapItemData,
    ) {
        let decorations = p.decorations.as_ref().map(|list| {
            let types = self
                .remote
                .as_ref()
                .and_then(|r| r.builtin.get("minecraft:map_decoration_type"));
            list.iter()
                .filter_map(|d| {
                    Some(Decoration {
                        sprite: decoration_sprite(types?.name(d.kind)?)?,
                        x: d.x,
                        y: d.y,
                        rot: (d.rot & 15) as u8,
                        holder: None,
                    })
                })
                .collect()
        });
        let dimension = self.dimension_id();
        let map = self
            .maps
            .data
            .entry(p.map_id)
            .or_insert_with(|| MapData::fresh(0.0, 0.0, p.scale.max(0) as u8, dimension));
        map.locked = p.locked;
        if let Some(decorations) = decorations {
            map.decorations = decorations;
        }
        let Some(patch) = &p.patch else {
            map.dirty = false;
            return;
        };
        for x in 0..patch.width {
            for y in 0..patch.height {
                let Some(&color) = patch.colors.get((x + y * patch.width) as usize) else {
                    continue;
                };
                let (px, py) = (patch.start_x + x, patch.start_y + y);
                if (0..128).contains(&px) && (0..128).contains(&py) {
                    map.update_color(px, py, color);
                }
            }
        }
        // The server owns a remote map: only the texture has to change.
        map.dirty = false;
    }
    fn next_map_id(&mut self) -> i32 {
        let last = *self.maps.last_id.get_or_insert_with(|| {
            horizon_client::save::read_dat(&self.world_root.join("data/minecraft/maps/last_id.dat"))
                .and_then(|t| match t.get("data")?.get("map")? {
                    Tag::Int(v) => Some(*v),
                    _ => None,
                })
                .unwrap_or(-1)
        });
        self.maps.last_id = Some(last + 1);
        last + 1
    }
    /// The id the next new map gets, without taking it.
    pub(super) fn peek_map_id(&mut self) -> i32 {
        let id = self.next_map_id();
        self.maps.last_id = Some(id - 1);
        id
    }
    /// `ExplorationMapFunction.run` for an item a generated chest still holds
    /// unsearched (`structure_blocks::take_pending_map`): the nearest
    /// structure of the tag around `origin`, a new map centred on it with its
    /// biome preview and target decoration. False when nothing is found: the
    /// loot table's `filtered` map_id check would have discarded the item.
    pub(super) fn resolve_pending_map(&mut self, item: &mut ItemStack, origin: [f64; 3]) -> bool {
        let Some(map) = horizon_client::structure_blocks::take_pending_map(item) else {
            return true;
        };
        let names = self.structure_tag(&map.destination);
        let from = origin.map(|v| v.floor() as i32);
        let found = self
            .end
            .locator()
            .filter(|_| !names.is_empty())
            .and_then(|t| t.locate(&names, from, map.search_radius));
        let Some((_, at)) = found else {
            return false;
        };
        let mut data = MapData::fresh(
            f64::from(at[0]),
            f64::from(at[2]),
            map.zoom,
            self.dimension_id(),
        );
        data.unlimited = true;
        let id = self.new_map_id(data);
        self.render_biome_preview(id);
        let d = item.data_mut();
        d.map_id = Some(id);
        d.other.retain(|(k, _)| k != "minecraft:map_decorations");
        d.other.push((
            "minecraft:map_decorations".into(),
            target_decoration(&map.decoration, at),
        ));
        true
    }
    /// The unsearched exploration maps of a container being opened
    /// (`RandomizableContainer.unpackLootTable` runs the function then).
    pub(super) fn resolve_container_maps(&mut self, key: (i32, i32, i32), origin: [f64; 3]) {
        use horizon_client::containers::BlockEntity;
        let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get(&key) else {
            return;
        };
        let pending: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.as_ref()
                    .is_some_and(horizon_client::structure_blocks::has_pending_map)
            })
            .map(|(i, _)| i)
            .collect();
        for i in pending {
            let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get_mut(&key) else {
                return;
            };
            let Some(mut item) = slots[i].take() else {
                continue;
            };
            let keep = self.resolve_pending_map(&mut item, origin);
            if let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get_mut(&key) {
                slots[i] = keep.then_some(item);
            }
        }
    }
    /// `MapItem.renderBiomePreviewMap`: the map's biomes at sea level, water
    /// outlines (`#water_on_map_outlines`) drawn in orange and brown.
    pub(super) fn render_biome_preview(&mut self, id: i32) {
        let Some(terrain) = self.end.locator() else {
            return;
        };
        let dim = self.dimension_id();
        let zoom = self.streaming.biome_zoom;
        let watery_biomes: std::collections::HashSet<String> = self
            .tag_entries("worldgen/biome", "minecraft:water_on_map_outlines")
            .into_iter()
            .collect();
        let Some(m) = self.maps.data.get(&id) else {
            return;
        };
        if m.dimension != dim {
            return;
        }
        let scale = 1i32 << m.scale;
        let start_x = m.center[0] / scale - 64;
        let start_z = m.center[1] / scale - 64;
        let sea_level = 63;
        let quarts: Vec<[i32; 3]> = (0..128)
            .flat_map(|row| {
                (0..128).map(move |column| {
                    zoom.quart_at_block(horizon_core::position::BlockPos::new(
                        (start_x + column) * scale,
                        sea_level,
                        (start_z + row) * scale,
                    ))
                })
            })
            .collect();
        let watery: Vec<bool> = terrain
            .noise_biomes(&quarts)
            .into_iter()
            .map(|b| b.is_some_and(|b| watery_biomes.contains(&b)))
            .collect();
        let is_watery = |x: i32, z: i32| watery[(z * 128 + x) as usize];
        // MapColor ids and MapColor.Brightness ids.
        const ORANGE: u8 = 15;
        const BROWN: u8 = 26;
        const LOW: u8 = 0;
        const NORMAL: u8 = 1;
        const HIGH: u8 = 2;
        const LOWEST: u8 = 3;
        let Some(m) = self.maps.data.get_mut(&id) else {
            return;
        };
        for mx in 1..127 {
            for mz in 1..127 {
                let mut water_count = 0;
                for dx in -1..2 {
                    for dz in -1..2 {
                        if (dx != 0 || dz != 0) && is_watery(mx + dx, mz + dz) {
                            water_count += 1;
                        }
                    }
                }
                let mut brightness = LOWEST;
                let mut color = 0;
                if is_watery(mx, mz) {
                    color = ORANGE;
                    if water_count > 7 && mz % 2 == 0 {
                        let sin = horizon_core::geometry::trig::sin(f64::from(mz as f32 + 0.0));
                        match (mx + (sin * 7.0) as i32) / 8 % 5 {
                            0 | 4 => brightness = LOW,
                            1 | 3 => brightness = NORMAL,
                            2 => brightness = HIGH,
                            _ => {}
                        }
                    } else if water_count > 7 {
                        color = 0;
                    } else if water_count > 5 {
                        brightness = NORMAL;
                    } else if water_count > 1 {
                        brightness = LOW;
                    }
                } else if water_count > 0 {
                    color = BROWN;
                    brightness = if water_count > 3 { NORMAL } else { LOWEST };
                }
                if color != 0 {
                    m.update_color(mx, mz, color << 2 | brightness);
                }
            }
        }
    }
    /// Registers `data` under a free map id.
    pub(super) fn new_map_id(&mut self, data: MapData) -> i32 {
        let id = self.next_map_id();
        self.maps.data.insert(id, data);
        id
    }
    /// The maps the player carries, for `status`: how many of their pixels
    /// are drawn, and their decorations.
    pub(super) fn maps_status(&self) -> Vec<serde_json::Value> {
        self.inventory
            .iter()
            .chain(std::iter::once(&self.offhand))
            .flatten()
            .filter_map(map_id)
            .filter_map(|id| {
                let m = self.maps.data.get(&id)?;
                Some(serde_json::json!({
                    "id": id,
                    "pixels": m.colors.iter().filter(|&&c| c != 0).count(),
                    "decorations": m.decorations.iter().map(|d| d.sprite).collect::<Vec<_>>(),
                }))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_decoration_type_has_its_sprite() {
        // MapDecorationTypes: (name, asset).
        for (name, asset) in [
            ("mansion", "woodland_mansion"),
            ("monument", "ocean_monument"),
            ("red_x", "red_x"),
            ("village_desert", "desert_village"),
            ("village_plains", "plains_village"),
            ("village_savanna", "savanna_village"),
            ("village_snowy", "snowy_village"),
            ("village_taiga", "taiga_village"),
            ("jungle_temple", "jungle_temple"),
            ("swamp_hut", "swamp_hut"),
            ("trial_chambers", "trial_chambers"),
            ("abandoned_camp", "abandoned_camp"),
            ("ancient_city", "ancient_city"),
            ("desert_pyramid", "desert_pyramid"),
            ("mineshaft", "mineshaft"),
            ("ocean_ruin_warm", "warm_ocean_ruins"),
        ] {
            assert_eq!(
                decoration_sprite(&format!("minecraft:{name}")),
                Some(format!("map/decorations/{asset}").as_str()),
                "{name}"
            );
        }
        assert_eq!(decoration_sprite("minecraft:nothing"), None);
    }
}
