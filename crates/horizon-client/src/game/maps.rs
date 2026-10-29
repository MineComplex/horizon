//! Maps as the server sends them (`ClientboundMapItemDataPacket`, the
//! client's `MapItemSavedData`) and `MapRenderer`: the 128x128 texture in an
//! atlas slot, the two-handed first-person view and maps in item frames.
//! Making and revealing maps is the server's.
use super::Game;
use horizon_client::{
    blocks::MAP_COLORS,
    hud::{MAP_SLOTS, Rect},
    items::ItemStack,
    mesher::Vertex,
};
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
pub(super) struct MapData {
    pub scale: u8,
    pub locked: bool,
    pub colors: Vec<u8>,
    pub decorations: Vec<Decoration>,
    texture_dirty: bool,
}
impl MapData {
    /// A map the client has not heard of yet: blank.
    fn blank(scale: u8) -> Self {
        Self {
            scale,
            locked: false,
            colors: vec![0; 128 * 128],
            decorations: Vec::new(),
            texture_dirty: true,
        }
    }
    fn update_color(&mut self, x: i32, y: i32, color: u8) -> bool {
        let i = (x + y * 128) as usize;
        if self.colors[i] == color {
            return false;
        }
        self.colors[i] = color;
        self.texture_dirty = true;
        true
    }
}
#[derive(Default)]
pub(super) struct Maps {
    pub data: HashMap<i32, MapData>,
    /// (map id, atlas slot), most recently shown last.
    slots: Vec<(i32, usize)>,
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
    /// The map's data, as the server last sent it.
    pub(super) fn map(&mut self, id: i32) -> Option<&mut MapData> {
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
        let map = self
            .maps
            .data
            .entry(p.map_id)
            .or_insert_with(|| MapData::blank(p.scale.max(0) as u8));
        map.locked = p.locked;
        if let Some(decorations) = decorations {
            map.decorations = decorations;
        }
        let Some(patch) = &p.patch else {
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
