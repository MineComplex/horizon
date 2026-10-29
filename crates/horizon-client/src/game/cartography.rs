//! The cartography table (`CartographyTableMenu` on the menu engine,
//! `CartographyTableScreen`): zooming a map out with paper, cloning it with
//! an empty map, locking it with a glass pane. The engine reads the level's
//! maps through `LevelMaps`; the scaled and locked copies a take makes
//! (`MapItem.scaleMap`, `lockMap`) are registered after the click. The
//! screen draws the preview with its error and lock sprites.
use super::{Game, maps::map_id};
use horizon_client::hud::HudBatch;

impl Game<'_> {
    /// `CartographyTableScreen.extractBackground` after the background.
    pub(super) fn draw_cartography(&mut self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let Some((map, extra)) = self
            .station
            .as_ref()
            .map(|s| (s.inputs[0].clone(), s.inputs[1].clone()))
        else {
            return;
        };
        let extra = extra.map(|e| e.item.to_string()).unwrap_or_default();
        let (duplicate, scaling, locking) = (
            extra == "minecraft:map",
            extra == "minecraft:paper",
            extra == "minecraft:glass_pane",
        );
        let id = map.as_ref().and_then(map_id);
        let state = id.and_then(|id| self.map(id).map(|m| (m.locked, m.scale)));
        let mut locked = false;
        let error = "gui/sprites/container/cartography_table/error";
        if let Some((is_locked, scale)) = state {
            if is_locked {
                locked = true;
                if scaling || locking {
                    batch.sprite(&self.front.gui.atlas, error, ox + 35.0, oy + 31.0);
                }
            }
            if scaling && scale >= 4 {
                locked = true;
                batch.sprite(&self.front.gui.atlas, error, ox + 35.0, oy + 31.0);
            }
        }
        let sprite = |batch: &mut HudBatch, game: &Self, name: &str, x: f32, y: f32| {
            batch.sprite(
                &game.front.gui.atlas,
                &format!("gui/sprites/container/cartography_table/{name}"),
                ox + x,
                oy + y,
            );
        };
        let preview = |batch: &mut HudBatch, game: &mut Self, x: f32, y: f32, scale: f32| {
            if let Some(rect) = id
                .filter(|_| state.is_some())
                .and_then(|id| game.map_texture(id))
            {
                let size = 128.0 * scale;
                batch.rect_uv(ox + x, oy + y, size, size, rect, u32::MAX);
            }
        };
        if scaling && !locked {
            sprite(batch, self, "scaled_map", 67.0, 13.0);
            preview(batch, self, 85.0, 31.0, 0.226);
        } else if duplicate {
            sprite(batch, self, "duplicated_map", 83.0, 13.0);
            preview(batch, self, 86.0, 16.0, 0.34);
            sprite(batch, self, "duplicated_map", 67.0, 29.0);
            preview(batch, self, 70.0, 32.0, 0.34);
        } else if locking {
            sprite(batch, self, "map", 67.0, 13.0);
            preview(batch, self, 71.0, 17.0, 0.45);
            sprite(batch, self, "locked", 118.0, 60.0);
        } else {
            sprite(batch, self, "map", 67.0, 13.0);
            preview(batch, self, 71.0, 17.0, 0.45);
        }
    }
}
