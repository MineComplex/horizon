//! Crafting for LAN guests: the inventory's 2x2 grid (window 0) and the
//! crafting table (`minecraft:crafting`) run the host's menu engine
//! (`horizon_client::menu`, `Kind::Inventory` / `Kind::Crafting`) over the
//! guest's inventory, with vanilla's packets: OpenScreen, container clicks
//! and their corrections, and the grid back into the inventory when the menu
//! closes (`AbstractContainerMenu.clearContainer`).
use super::{Game, lan::LanHost};
use horizon_client::{
    items::ItemStack,
    lan::guest::Guest,
    menu::{Env, Kind, Menu}};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The guest's menus the game runs: kept between clicks, so a drag (quick
/// craft) spans them as in vanilla.
#[derive(Default)]
pub(super) struct GuestCrafting {
    /// The inventory menu; its grid lives in the hub's guest.
    inventory: Option<Box<Menu>>,
    /// The open crafting table: its window, state id, menu and block.
    table: Option<(i32, i32, Box<Menu>, [i32; 3])>,
    /// The open furnace's or brewing stand's window and menu (`AbstractFurnaceMenu`,
    /// `BrewingStandMenu`); its slots live in the block entity and the hub's
    /// container menu keeps the client in sync.
    machine: Option<(i32, Box<Menu>)>,
}


/// The armor's player-container slots, head to feet (`Inventory` 39..36).
const ARMOR: [usize; 4] = [39, 38, 37, 36];
const OFFHAND: usize = 40;

/// The guest's inventory, armor, off hand, selection and carried stack into `menu`.
fn load_player(menu: &mut Menu, g: &Guest) {
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
fn store_player(menu: &mut Menu, g: &mut Guest) {
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

impl Game<'_> {}
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
    /// Runs `op` on the guest's menu with their inventory copied in and back.
    pub(super) fn guest_menu_op<R>(
        &mut self,
        host: &mut LanHost,
        gi: usize,
        menu: &mut Menu,
        op: impl FnOnce(&mut Menu, &Env) -> R,
    ) -> R {
        // The cartography table reads and makes the host level's maps.
        let maps = (menu.kind() == Kind::Cartography).then(|| {
            let g = &host.hub.guests[gi];
            let held: Vec<ItemStack> = (0..menu.slot_count())
                .filter_map(|s| menu.slot(s).cloned())
                .chain(g.inventory.iter().flatten().cloned())
                .chain(g.carried.clone())
                .collect();
            self.level_maps(&held)
        });
        let env = Env {
            maps: maps.as_ref().map(|m| m as &dyn horizon_client::menu::Maps),
            ..self.menu_env()
        };
        load_player(menu, &host.hub.guests[gi]);
        let result = op(menu, &env);
        if let Some(maps) = maps {
            self.register_made_maps(maps);
        }
        let dropped = std::mem::take(&mut menu.dropped);
        let equipped = std::mem::take(&mut menu.equipped);
        let brewed = std::mem::take(&mut menu.brewed);
        menu.drops.clear();
        menu.crafted.clear();
        store_player(menu, &mut host.hub.guests[gi]);
        if !brewed.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                for stack in &brewed {
                    g.trigger_brewed_potion(stack);
                }
            });
        }
        if !equipped.is_empty() {
            let conn = host.hub.guests[gi].conn;
            self.as_guest(host, conn, |g| {
                for (slot, stack) in equipped {
                    g.player_equipped(slot, stack.as_ref());
                }
            });
        }
        self.drop_guest_items(host, gi, dropped);
        result
    }
    /// The guest opened a crafting table (`CraftingTableBlock.getMenuProvider`).
    pub(super) fn lan_open_crafting(&mut self, host: &mut LanHost, conn: u32, pos: [i32; 3]) {
        // InventoryOpenEvent: cancelled, the table opens nothing.
        self.lan_crafting_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        // The container it replaces loses its opener.
        self.guest_container_closed(host, conn);
        let title = horizon_protocol::text::translatable("container.crafting", &[]);
        let Some(window) = host.hub.open_window(conn, "minecraft:crafting", title) else {
            return;
        };
        let menu = Box::new(Menu::new(Kind::Crafting));
        host.play.entry(conn).or_default().crafting.table = Some((window, 0, menu, pos));
        self.lan_send_table_content(host, conn);
    }
    /// The whole crafting table menu for the client (`sendAllDataToRemote`).
    fn lan_send_table_content(&mut self, host: &mut LanHost, conn: u32) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some((window, state, menu, _)) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.crafting.table.as_mut())
        else {
            return;
        };
        *state = (*state + 1) & 0x7FFF;
        let (window, state) = (*window, *state);
        let mut items: Vec<Option<ItemStack>> = (0..10).map(|s| menu.slot(s).cloned()).collect();
        let g = &host.hub.guests[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let items = items.iter().map(|s| host.hub.stack(s)).collect();
        let carried = host.hub.stack(&carried);
        host.hub.send_to(
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
    pub(super) fn lan_crafting_closed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let crafting = &mut host.play.entry(conn).or_default().crafting;
        let grid: Vec<ItemStack> = if window == Some(0) {
            crafting.inventory = None;
            let g = &mut host.hub.guests[gi];
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
            self.lan_place_back(host, gi, grid);
        }
        true
    }
    /// `Inventory.placeItemBackInInventory` of `stacks` for guest `gi`, what
    /// doesn't fit dropped (`clearContainer`).
    pub(super) fn lan_place_back(&mut self, host: &mut LanHost, gi: usize, stacks: Vec<ItemStack>) {
        let mut menu = Menu::new(Kind::Inventory);
        self.guest_menu_op(host, gi, &mut menu, |menu, env| {
            place_back(menu, env, stacks)
        });
    }
}
