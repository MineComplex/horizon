//! Crafting for players: the inventory's 2x2 grid (window 0) and the
//! crafting table (`minecraft:crafting`) run the game's menu engine
//! (`crate::hc::menu`, `Kind::Inventory` / `Kind::Crafting`) over the
//! player's inventory, with vanilla's packets: OpenScreen, container clicks
//! and their corrections, and the grid back into the inventory when the menu
//! closes (`AbstractContainerMenu.clearContainer`).
use super::server_net::Network;
use crate::hc::{
    containers::BlockEntity,
    furnace::FurnaceCooking,
    items::ItemStack,
    menu::{Env, Input, Kind, Menu},
    server_net::{menu::Slots, player::ServerPlayer},
};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The player's menus the game runs: kept between clicks, so a drag (quick
/// craft) spans them as in vanilla.
#[derive(Default)]
pub(super) struct PlayerCrafting {
    /// The inventory menu; its grid lives in the hub's player.
    inventory: Option<Box<Menu>>,
    /// The open crafting table: its window, state id, menu and block.
    table: Option<(i32, i32, Box<Menu>, [i32; 3])>,
    /// The open furnace's or brewing stand's window and menu (`AbstractFurnaceMenu`,
    /// `BrewingStandMenu`); its slots live in the block entity and the hub's
    /// container menu keeps the client in sync.
    machine: Option<(i32, Box<Menu>)>,
}

/// ServerboundContainerClickPacket's input.
pub(super) fn input(id: i32) -> Input {
    match id {
        0 => Input::Pickup,
        1 => Input::QuickMove,
        2 => Input::Swap,
        3 => Input::Clone,
        4 => Input::Throw,
        5 => Input::QuickCraft,
        _ => Input::PickupAll,
    }
}

/// The armor's player-container slots, head to feet (`Inventory` 39..36).
const ARMOR: [usize; 4] = [39, 38, 37, 36];
const OFFHAND: usize = 40;

/// The player's inventory, armor, off hand, selection and carried stack into `menu`.
fn load_player(menu: &mut Menu, g: &ServerPlayer) {
    for i in 0..36 {
        menu.set_player_item(i, g.inventory[i].clone());
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        menu.set_player_item(slot, g.armor[k].clone());
    }
    menu.set_player_item(OFFHAND, g.offhand.clone());
    menu.selected = g.selected;
    menu.creative = g.creative();
    menu.levels = g.xp_level;
    menu.carried = g.carried.clone();
}

/// `load_player` back.
fn store_player(menu: &mut Menu, g: &mut ServerPlayer) {
    for i in 0..36 {
        g.inventory[i] = menu.player_item(i).cloned();
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        g.armor[k] = menu.player_item(slot).cloned();
    }
    g.offhand = menu.player_item(OFFHAND).cloned();
    g.carried = menu.carried.take();
}

/// Inventory.placeItemBackInInventory for each stack: what doesn't fit is
/// left for Player.drop (creative players keep it too).
fn place_back(menu: &mut Menu, env: &Env, stacks: Vec<ItemStack>) {
    menu.creative = false;
    for mut stack in stacks {
        menu.give(env, &mut stack);
        if stack.count > 0 {
            menu.dropped.push(stack);
        }
    }
}

impl crate::game::Sim<'_> {
    pub(super) fn menu_env(&self) -> Env<'_> {
        Env {
            items: self.res.items(),
            recipes: self.res.recipes(),
            brewing: &self.res.stations().brewing,
            banners: &self.res.stations().banners,
            station_recipes: &self.res.stations().recipes,
            enchantments: &self.res.stations().enchantments,
            names: &self.pack.names,
            trade: None,
            maps: None,
        }
    }
    /// Runs `op` on the player's menu with their inventory copied in and back.
    pub(super) fn player_menu_op<R>(
        &mut self,
        net: &mut Network,
        gi: usize,
        menu: &mut Menu,
        op: impl FnOnce(&mut Menu, &Env) -> R,
    ) -> R {
        // The cartography table reads and makes the game level's maps.
        let maps = (menu.kind() == Kind::Cartography).then(|| {
            let g = &net.hub.players[gi];
            let held: Vec<ItemStack> = (0..menu.slot_count())
                .filter_map(|s| menu.slot(s).cloned())
                .chain(g.inventory.iter().flatten().cloned())
                .chain(g.carried.clone())
                .collect();
            self.level_maps(&held)
        });
        let env = Env {
            maps: maps.as_ref().map(|m| m as &dyn crate::hc::menu::Maps),
            ..self.menu_env()
        };
        load_player(menu, &net.hub.players[gi]);
        let result = op(menu, &env);
        if let Some(maps) = maps {
            self.register_made_maps(maps);
        }
        let dropped = std::mem::take(&mut menu.dropped);
        let equipped = std::mem::take(&mut menu.equipped);
        let brewed = std::mem::take(&mut menu.brewed);
        menu.drops.clear();
        menu.crafted.clear();
        store_player(menu, &mut net.hub.players[gi]);
        if !brewed.is_empty() {
            let conn = net.hub.players[gi].conn;
            self.as_player(net, conn, |g| {
                for stack in &brewed {
                    g.trigger_brewed_potion(stack);
                }
            });
        }
        if !equipped.is_empty() {
            let conn = net.hub.players[gi].conn;
            self.as_player(net, conn, |g| {
                for (slot, stack) in equipped {
                    g.player_equipped(slot, stack.as_ref());
                }
            });
        }
        self.drop_player_items(net, gi, dropped);
        result
    }
    /// A click on the player's inventory menu (window 0): `InventoryMenu`,
    /// crafting grid included. False for other windows.
    pub(super) fn net_inventory_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        if p.container != 0 {
            return false;
        }
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return true;
        };
        if net.hub.players[gi].spectator() {
            return true;
        }
        let crafting = net.play.entry(conn).or_default();
        let mut menu = crafting
            .crafting
            .inventory
            .take()
            .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
        let grid = net.hub.players[gi].craft_grid.clone();
        let changed = self.player_menu_op(net, gi, &mut menu, |menu, env| {
            for (k, s) in grid.into_iter().enumerate() {
                menu.set_slot(env, 1 + k, s);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        });
        let g = &mut net.hub.players[gi];
        for k in 0..4 {
            g.craft_grid[k] = menu.slot(1 + k).cloned();
        }
        g.craft_result = menu.slot(0).cloned();
        net.play.entry(conn).or_default().crafting.inventory = Some(menu);
        net.hub.inventory_clicked(conn, p, &self.world, &changed);
        true
    }
    /// A click on the player's open furnace, blast furnace, smoker or brewing stand:
    /// the game's `AbstractFurnaceMenu` (fuel slot, take-only result, quick moves by
    /// recipe and fuel) or `BrewingStandMenu` (one bottle per slot, ingredient and
    /// blaze powder slots) over the block entity's slots. False for other windows.
    pub(super) fn net_machine_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let Some(pos) = net.hub.players[gi]
            .menu
            .as_ref()
            .filter(|m| p.container != 0 && m.window == p.container)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some(pos),
                _ => None,
            })
        else {
            return false;
        };
        let (kind, items) = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                let recipe = match f.kind.recipe_type() {
                    "minecraft:blasting" => "blasting",
                    "minecraft:smoking" => "smoking",
                    _ => "smelting",
                };
                (Kind::Furnace(recipe), f.items.to_vec())
            }
            Some(BlockEntity::Brewing(b)) => (Kind::Brewing, b.items.to_vec()),
            Some(BlockEntity::Crafter(c)) => (Kind::CrafterBlock, c.items.to_vec()),
            _ => return false,
        };
        let furnace = matches!(kind, Kind::Furnace(_));
        // CrafterBlockEntity.containerData: the disabled slots, then triggered.
        let crafter_data = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Crafter(c)) => Some((c.disabled, c.triggered)),
            _ => None,
        };
        if net.hub.players[gi].spectator() {
            return true;
        }
        let mut menu = net
            .play
            .entry(conn)
            .or_default()
            .crafting
            .machine
            .take()
            .filter(|(window, _)| *window == p.container)
            .map_or_else(|| Box::new(Menu::new(kind)), |(_, m)| m);
        let changed = self.player_menu_op(net, gi, &mut menu, |menu, env| {
            for (k, s) in items.iter().enumerate() {
                menu.set_slot(env, k, s.clone());
            }
            // After the slots: setting a disabled slot (CrafterBlockEntity.setItem) enables it.
            if let Some((disabled, triggered)) = crafter_data {
                for (k, d) in disabled.iter().enumerate() {
                    menu.data[k] = i32::from(*d);
                }
                menu.data[9] = i32::from(triggered);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        });
        match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                for k in 0..3 {
                    f.items[k] = menu.slot(k).cloned();
                }
                // AbstractFurnaceBlockEntity.setItem: only a new input restarts cooking.
                f.input_clicked(self.res.recipes(), items[0].clone());
            }
            // BrewingStandBlockEntity.serverTick notices a new ingredient itself.
            Some(BlockEntity::Brewing(b)) => {
                for k in 0..5 {
                    b.items[k] = menu.slot(k).cloned();
                }
            }
            // CrafterBlockEntity.setItem: filling a disabled slot enables it.
            Some(BlockEntity::Crafter(c)) => {
                for k in 0..9 {
                    c.items[k] = menu.slot(k).cloned();
                    c.disabled[k] = menu.data[k] == 1;
                }
            }
            _ => {}
        }
        // FurnaceResultSlot.checkTakeAchievements: the player took from the output.
        let count = |s: Option<&ItemStack>| s.map_or(0, |s| s.count);
        if furnace && count(menu.slot(2)) < count(items[2].as_ref()) {
            let at = net.hub.players[gi].position;
            self.award_furnace_recipes(pos, at);
        }
        // Slot.setChanged on a changed container slot: BlockEntity.setChanged,
        // so the chunk is saved and comparators read the block again.
        if changed.iter().any(|&s| s < items.len()) {
            self.container_changed(pos);
        }
        net.play.entry(conn).or_default().crafting.machine = Some((p.container, menu));
        net.hub.inventory_clicked(conn, p, &self.world, &changed);
        true
    }
    /// `ServerGamePacketListenerImpl.handleContainerSlotStateChanged`: a
    /// player's crafter slot toggle (`CrafterBlockEntity.setSlotState`) in
    /// their open crafter window.
    pub(super) fn net_crafter_slot_state(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: i32,
        slot: i32,
        enabled: bool,
    ) {
        let Some(pos) = net
            .hub
            .player(conn)
            .filter(|g| !g.spectator())
            .and_then(|g| g.menu.as_ref())
            .filter(|m| window != 0 && m.window == window)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some(pos),
                _ => None,
            })
        else {
            return;
        };
        if let (Some(BlockEntity::Crafter(c)), Ok(slot)) = (
            self.world.block_entities.get_mut(&pos),
            usize::try_from(slot),
        ) {
            c.set_slot_state(slot, enabled);
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
    }
    /// ServerGamePacketListenerImpl.handlePlaceRecipe: the player's recipe
    /// book places a known recipe in their inventory grid or crafting table
    /// (`ServerPlaceRecipe`), and answers with a ghost recipe when the items
    /// are missing.
    pub(super) fn net_place_recipe(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &horizon_protocol::packets::recipes::PlaceRecipe,
    ) {
        use crate::hc::menu::place::PostPlace;
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return;
        };
        let g = &net.hub.players[gi];
        if g.spectator() {
            return;
        }
        let Some(id) = self
            .res
            .stations()
            .recipe_index
            .recipe_of_display(p.recipe)
            .map(str::to_owned)
        else {
            return;
        };
        // An unknown or impossible recipe is refused outright.
        if !g.recipe_book.knows(&id)
            || self
                .res
                .recipes()
                .placement(&id)
                .is_none_or(|placement| placement.impossible())
        {
            return;
        }
        let creative = g.creative();
        let placed = if p.container == 0 {
            let crafting = net.play.entry(conn).or_default();
            let mut menu = crafting
                .crafting
                .inventory
                .take()
                .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
            let grid = net.hub.players[gi].craft_grid.clone();
            let placed = self.player_menu_op(net, gi, &mut menu, |menu, env| {
                for (k, s) in grid.into_iter().enumerate() {
                    menu.set_slot(env, 1 + k, s);
                }
                menu.place_recipe(env, &id, p.use_max, creative, None)
            });
            let g = &mut net.hub.players[gi];
            for k in 0..4 {
                g.craft_grid[k] = menu.slot(1 + k).cloned();
            }
            g.craft_result = menu.slot(0).cloned();
            net.play.entry(conn).or_default().crafting.inventory = Some(menu);
            net.hub.send_inventory(gi);
            placed
        } else {
            let Some((window, state, mut menu, pos)) = net
                .play
                .get_mut(&conn)
                .and_then(|play| play.crafting.table.take_if(|(w, ..)| *w == p.container))
            else {
                return;
            };
            let placed = self.player_menu_op(net, gi, &mut menu, |menu, env| {
                menu.place_recipe(env, &id, p.use_max, creative, None)
            });
            net.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
            self.net_send_table_content(net, conn);
            placed
        };
        if placed == PostPlace::Ghost {
            net.hub.send_ghost_recipe(conn, p.container, &id);
        }
    }
    /// The player opened a crafting table (`CraftingTableBlock.getMenuProvider`).
    pub(super) fn net_open_crafting(&mut self, net: &mut Network, conn: u32, pos: [i32; 3]) {
        // InventoryOpenEvent: cancelled, the table opens nothing.
        self.net_crafting_closed(net, conn, None);
        self.net_station_closed(net, conn, None);
        // The container it replaces loses its opener.
        self.player_container_closed(net, conn);
        let title = horizon_protocol::text::translatable("container.crafting", &[]);
        let Some(window) = net.hub.open_window(conn, "minecraft:crafting", title) else {
            return;
        };
        let menu = Box::new(Menu::new(Kind::Crafting));
        net.play.entry(conn).or_default().crafting.table = Some((window, 0, menu, pos));
        self.net_send_table_content(net, conn);
    }
    /// A click on the player's crafting table window; false for other windows.
    pub(super) fn net_crafting_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some(play) = net.play.get_mut(&conn) else {
            return false;
        };
        let Some((window, state, mut menu, pos)) = play
            .crafting
            .table
            .take_if(|(window, ..)| *window == p.container)
        else {
            return false;
        };
        // A table that is no longer valid takes no clicks (handleContainerClick).
        if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
            && !net.hub.players[gi].spectator()
            && self.net_table_valid(&net.hub.players[gi], pos)
        {
            self.player_menu_op(net, gi, &mut menu, |menu, env| {
                let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            });
        }
        net.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
        self.net_send_table_content(net, conn);
        true
    }
    /// `CraftingMenu.stillValid`: the block is still a crafting table and the
    /// player is within reach of it.
    fn net_table_valid(&self, g: &ServerPlayer, pos: [i32; 3]) -> bool {
        let name = &self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name;
        &**name == "minecraft:crafting_table" && g.within_block_range(pos, 4.0)
    }
    /// `ServerPlayer.tick`'s `stillValid`: a player's crafting table closes
    /// once it is gone or out of reach, giving its grid back.
    pub(super) fn net_tables_tick(&mut self, net: &mut Network) {
        let invalid: Vec<(u32, i32)> = net
            .play
            .iter()
            .filter_map(|(c, p)| p.crafting.table.as_ref().map(|t| (*c, t.0, t.3)))
            .filter(|&(c, _, pos)| {
                net.hub
                    .players
                    .iter()
                    .find(|g| g.conn == c)
                    .is_some_and(|g| net.hub.here(g) && !self.net_table_valid(g, pos))
            })
            .map(|(c, window, _)| (c, window))
            .collect();
        for (conn, window) in invalid {
            self.net_crafting_closed(net, conn, Some(window));
            net.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose { container: window }),
            );
            if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) {
                self.player_carried_back(net, gi);
            }
        }
    }
    /// The whole crafting table menu for the client (`sendAllDataToRemote`).
    fn net_send_table_content(&mut self, net: &mut Network, conn: u32) {
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some((window, state, menu, _)) = net
            .play
            .get_mut(&conn)
            .and_then(|p| p.crafting.table.as_mut())
        else {
            return;
        };
        *state = (*state + 1) & 0x7FFF;
        let (window, state) = (*window, *state);
        let mut items: Vec<Option<ItemStack>> = (0..10).map(|s| menu.slot(s).cloned()).collect();
        let g = &net.hub.players[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let items = items.iter().map(|s| net.hub.stack(s)).collect();
        let carried = net.hub.stack(&carried);
        net.hub.send_to(
            conn,
            &C::ContainerSetContent(play::ContainerSetContent {
                container: window,
                state,
                items,
                carried,
            }),
        );
    }
    /// `AbstractContainerMenu.removed` of a crafting menu: the closed window
    /// (`window`; None closes the table whichever it is, and 0 the inventory
    /// menu) gives its grid back to the inventory, what doesn't fit is
    /// dropped. False when no such window is open.
    pub(super) fn net_crafting_closed(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let crafting = &mut net.play.entry(conn).or_default().crafting;
        let grid: Vec<ItemStack> = if window == Some(0) {
            crafting.inventory = None;
            let g = &mut net.hub.players[gi];
            g.craft_result = None;
            g.craft_grid.iter_mut().filter_map(Option::take).collect()
        } else {
            match crafting
                .table
                .take_if(|(w, ..)| window.is_none_or(|window| *w == window))
            {
                Some((_, _, menu, _)) => (1..10).filter_map(|s| menu.slot(s).cloned()).collect(),
                None => return false,
            }
        };
        if !grid.is_empty() {
            self.net_place_back(net, gi, grid);
        }
        true
    }
    /// `Inventory.placeItemBackInInventory` of `stacks` for player `gi`, what
    /// doesn't fit dropped (`clearContainer`).
    pub(super) fn net_place_back(&mut self, net: &mut Network, gi: usize, stacks: Vec<ItemStack>) {
        let mut menu = Menu::new(Kind::Inventory);
        self.player_menu_op(net, gi, &mut menu, |menu, env| {
            place_back(menu, env, stacks)
        });
    }
    /// A player left: what their crafting grids and workstation held goes back into their
    /// inventory before it is saved (dropped where they stood if full).
    pub(super) fn net_crafting_left(&mut self, net: &mut Network, g: &mut ServerPlayer) {
        // The player's play state stays: their statistics are awarded and saved
        // after this (`HostEvent::Left`), which removes it.
        let play = net.play.get_mut(&g.conn);
        let mut grid: Vec<ItemStack> = g.craft_grid.iter_mut().filter_map(Option::take).collect();
        g.craft_result = None;
        if let Some(play) = play {
            if let Some((_, _, menu, _)) = play.crafting.table.take() {
                grid.extend((1..10).filter_map(|s| menu.slot(s).cloned()));
            }
            // A workstation's inputs too.
            if let Some(station) = play.station.take() {
                grid.extend(super::net_stations::station_inputs(&station));
            }
        }
        if grid.is_empty() {
            return;
        }
        let mut menu = Menu::new(Kind::Inventory);
        load_player(&mut menu, g);
        place_back(&mut menu, &self.menu_env(), grid);
        let dropped = std::mem::take(&mut menu.dropped);
        store_player(&mut menu, g);
        for stack in dropped {
            self.spawn_item(stack, g.position, [0.0, 0.1, 0.0], 40);
        }
    }
}
