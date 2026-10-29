//! The cartography table (`CartographyTableMenu` on the menu engine,
//! `CartographyTableScreen`): zooming a map out with paper, cloning it with
//! an empty map, locking it with a glass pane. The engine reads the level's
//! maps through `LevelMaps`; the scaled and locked copies a take makes
//! (`MapItem.scaleMap`, `lockMap`) are registered after the click. The
//! screen draws the preview with its error and lock sprites.
use super::maps::{MapData, map_id};
use crate::hc::{items::ItemStack, menu::Maps};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

/// The level's maps as a menu sees them (`ContainerLevelAccess`): the
/// states of the maps the click can reach, and the copies it makes.
pub(super) struct LevelMaps {
    states: RefCell<HashMap<i32, (bool, u8)>>,
    next_id: Cell<i32>,
    /// (new id, source id, scaled) for each copy made.
    made: RefCell<Vec<(i32, i32, bool)>>,
}

impl Maps for LevelMaps {
    fn state(&self, id: i32) -> Option<(bool, u8)> {
        self.states.borrow().get(&id).copied()
    }

    fn derive(&self, id: i32, scale: bool) -> Option<i32> {
        let (_, s) = self.state(id)?;
        let new = self.next_id.get();
        self.next_id.set(new + 1);
        let made = if scale {
            (false, (s + 1).min(4))
        } else {
            (true, s)
        };
        self.states.borrow_mut().insert(new, made);
        self.made.borrow_mut().push((new, id, scale));
        Some(new)
    }
}

impl crate::game::Server<'_> {
    /// The maps of `stacks` (loaded as needed) for a menu over this level.
    pub(super) fn level_maps<'s>(
        &mut self,
        stacks: impl IntoIterator<Item = &'s ItemStack>,
    ) -> LevelMaps {
        let ids: Vec<i32> = stacks.into_iter().filter_map(map_id).collect();
        let mut states = HashMap::new();
        for id in ids {
            if let Some(m) = self.map(id) {
                states.insert(id, (m.locked, m.scale));
            }
        }
        LevelMaps {
            states: RefCell::new(states),
            next_id: Cell::new(self.peek_map_id()),
            made: RefCell::new(Vec::new()),
        }
    }
    /// The scaled and locked copies a menu made (`level.setMapData`).
    pub(super) fn register_made_maps(&mut self, maps: LevelMaps) {
        for (id, source, scale) in maps.made.into_inner() {
            let Some(data) = self
                .map(source)
                .map(|m| if scale { m.scaled() } else { m.locked_copy() })
            else {
                continue;
            };
            let new = self.new_map_id(data);
            debug_assert_eq!(new, id, "map ids follow peek_map_id");
        }
    }
}
impl MapData {
    /// `MapItemSavedData.scaled`: a fresh map one scale up around the same centre.
    pub(super) fn scaled(&self) -> MapData {
        let mut m = MapData::fresh(
            f64::from(self.center[0]),
            f64::from(self.center[1]),
            (self.scale + 1).min(4),
            &self.dimension,
        );
        m.tracking = self.tracking;
        m.unlimited = self.unlimited;
        m
    }
    /// `MapItemSavedData.locked`: the same pixels, locked.
    pub(super) fn locked_copy(&self) -> MapData {
        let mut m = MapData::fresh(
            f64::from(self.center[0]),
            f64::from(self.center[1]),
            self.scale,
            &self.dimension,
        );
        m.center = self.center;
        m.colors = self.colors.clone();
        m.tracking = self.tracking;
        m.unlimited = self.unlimited;
        m.locked = true;
        m
    }
}
