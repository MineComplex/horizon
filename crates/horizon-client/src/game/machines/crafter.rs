//! The crafter (`CrafterBlock`, `CrafterBlockEntity`, `CrafterMenu`,
//! `CrafterScreen`): redstone-triggered crafting of its 3x3 grid, ejecting
//! the result and remainders into the container in front or as items, the
//! crafting animation ticks, and the menu with its toggleable slots.
use super::*;
use horizon_client::{
    containers::Crafter,
    hud::{HudBatch, rgba},
};
use horizon_protocol::packets::play::{self, Serverbound as S};

const RESULT_SLOT: usize = 309;

impl Game<'_> {
    // ---- the menu ----

    /// `CrafterScreen`'s background, disabled slots and powered arrow (the
    /// verified `screen_layout::crafter`) and its centred title.
    pub(in crate::game) fn draw_crafter_screen(&self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let Some(c) = self.container.and_then(|k| self.crafter_at(k)) else {
            return;
        };
        let powered = self
            .container
            .map(|k| self.world.block(k.0, k.1, k.2))
            .is_some_and(|state| self.containers().prop(state, "triggered") == Some("true"));
        let draws =
            horizon_client::screen_layout::crafter(&horizon_client::screen_layout::CrafterView {
                width: 176,
                height: 166,
                left: 0,
                top: 0,
                disabled: c.disabled,
                powered,
            });
        // The background goes first, then the labels, then the slot decorations.
        let (background, rest) = draws.split_at(2.min(draws.len()));
        self.paint_screen(batch, ox, oy, background, true);
        let atlas = &self.gui.atlas;
        let grey = rgba(64, 64, 64, 255);
        let title = self.container_title(self.translate("container.crafter", "Crafter"));
        let tw = HudBatch::text_width(atlas, &title);
        batch.text(
            atlas,
            &title,
            ox + ((176.0 - tw) / 2.0).trunc(),
            oy + 6.0,
            grey,
            false,
        );
        let inventory = self.translate("container.inventory", "Inventory");
        batch.text(atlas, &inventory, ox + 8.0, oy + 72.0, grey, false);
        self.paint_screen(batch, ox, oy, rest, true);
    }
}
impl crate::game::Game<'_> {
    /// `Level.setBlock` of one boolean property of the block at `pos`.
    pub(crate) fn set_bool_prop(&mut self, pos: Pos, key: &str, value: bool, flags: i32) {
        let registry = self.pack.assets.registry;
        let old = self.world.block(pos.0, pos.1, pos.2);
        let Some(mut state) = registry.state(old).cloned() else {
            return;
        };
        if !state.properties.contains_key(key) {
            return;
        }
        state.properties.insert(key.to_owned(), value.to_string());
        let Ok(id) = registry.resolve(&state) else {
            return;
        };
        self.with_level(|b, level, ticks| {
            b.set_block(level, ticks, BlockPos::new(pos.0, pos.1, pos.2), id, flags)
        });
        let _ = self.process_level_events();
    }
    fn crafter_at(&self, pos: Pos) -> Option<&Crafter> {
        match self.world.block_entities.get(&pos)? {
            BlockEntity::Crafter(c) => Some(c),
            _ => None,
        }
    }
    /// The grid's current recipe result (`CrafterMenu.refreshRecipeResult`).
    pub(in crate::game) fn crafter_result(&self) -> Option<ItemStack> {
        let key = self.container?;
        let c = self.crafter_at(key)?;
        self.res.recipes().craft(&c.items, 3)
    }
    fn crafter_open(&self) -> bool {
        matches!(self.container_kind(), Some(BlockEntity::Crafter(_)))
    }
    /// `CrafterMenu` slot layout: the grid at (26, 17), the result at (134, 35).
    pub(in crate::game) fn crafter_slot_position(&self, i: usize) -> Option<(f32, f32)> {
        if !self.crafter_open() {
            return None;
        }
        match i {
            300..309 => Some((
                26.0 + ((i - 300) % 3) as f32 * 18.0,
                17.0 + ((i - 300) / 3) as f32 * 18.0,
            )),
            RESULT_SLOT => Some((134.0, 35.0)),
            _ => None,
        }
    }
    /// `gui.togglable_slot` over an empty, enabled grid slot.
    pub(in crate::game) fn crafter_tooltip(&self, slot: usize) -> Option<String> {
        let c = self.container.and_then(|k| self.crafter_at(k))?;
        let i = slot.checked_sub(300).filter(|i| *i < 9)?;
        (self.carried.is_none() && c.items[i].is_none() && !c.disabled[i] && !self.spectator())
            .then(|| self.translate("gui.togglable_slot", "Click to disable slot"))
    }
    /// `CrafterScreen.slotClicked`: an empty grid slot is enabled by a click
    /// (or a number key with a hotbar item) when disabled, and disabled by a
    /// click with nothing carried; the menu then gets the click itself.
    pub(in crate::game) fn crafter_slot_clicked(
        &mut self,
        slot: usize,
        button: i32,
        input: horizon_client::menu::Input,
    ) {
        use horizon_client::menu::Input;
        if !self.crafter_open() || !(300..309).contains(&slot) || self.spectator() {
            return;
        }
        let i = slot - 300;
        let Some(c) = self.container.and_then(|k| self.crafter_at(k)) else {
            return;
        };
        if c.items[i].is_some() {
            return;
        }
        let disabled = c.disabled[i];
        let enable = match input {
            Input::Pickup if disabled => Some(true),
            Input::Pickup if self.carried.is_none() => Some(false),
            Input::Swap if disabled => usize::try_from(button)
                .ok()
                .and_then(|b| {
                    if b == 40 {
                        Some(&self.offhand)
                    } else {
                        self.inventory.get(b)
                    }
                })
                .is_some_and(Option::is_some)
                .then_some(true),
            _ => None,
        };
        let Some(enabled) = enable else {
            return;
        };
        // ServerboundContainerSlotStateChangedPacket: CrafterBlockEntity.setSlotState.
        self.menu_session(|menu, _, _| {
            menu.set_slot_state(i, enabled);
            Some(true)
        });
        if let Some((container, _)) = self.remote.as_ref().and_then(|r| r.menu.clone()) {
            self.send(S::ContainerSlotStateChanged(
                play::ContainerSlotStateChanged {
                    slot: i as i32,
                    container,
                    enabled,
                },
            ));
        }
        let pitch = if enabled { 1.0 } else { 0.75 };
        self.sounds
            .play("minecraft:ui.button.click", None, 0.4, pitch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: &str, count: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }
    #[test]
    fn signal_counts_filled_and_disabled_slots() {
        let mut c = Crafter::default();
        assert_eq!(c.redstone_signal(), 0);
        c.items[0] = stack("minecraft:stick", 3);
        c.set_slot_state(4, false);
        assert_eq!(c.redstone_signal(), 2);
        // A filled slot cannot be disabled.
        c.set_slot_state(0, false);
        assert!(!c.disabled[0]);
        c.set_slot_state(4, true);
        assert_eq!(c.redstone_signal(), 1);
    }
    #[test]
    fn hoppers_fill_the_smaller_stacks_first() {
        let mut c = Crafter::default();
        let stick = ItemStack::new("minecraft:stick", 1);
        c.items[0] = stack("minecraft:stick", 2);
        c.items[1] = stack("minecraft:stick", 1);
        // A later empty slot counts as smaller: stacks level out front to back.
        assert!(!c.can_place(0, &stick, 64));
        assert!(!c.can_place(1, &stick, 64));
        assert!(c.can_place(2, &stick, 64));
        c.disabled[5] = true;
        assert!(!c.can_place(5, &stick, 64));
        // With every later slot at least as big, a slot accepts again.
        for i in 2..9 {
            c.items[i] = stack("minecraft:stick", 2);
        }
        assert!(c.can_place(1, &stick, 64));
        // A full stack accepts nothing.
        c.items[2] = stack("minecraft:stick", 64);
        assert!(!c.can_place(2, &stick, 64));
    }
}
