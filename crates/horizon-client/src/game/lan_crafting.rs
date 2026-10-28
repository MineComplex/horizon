//! Crafting for LAN guests: the inventory's 2x2 grid (window 0) and the
//! crafting table (`minecraft:crafting`) run the host's menu engine
//! (`horizon_client::menu`, `Kind::Inventory` / `Kind::Crafting`) over the
//! guest's inventory, with vanilla's packets: OpenScreen, container clicks
//! and their corrections, and the grid back into the inventory when the menu
//! closes (`AbstractContainerMenu.clearContainer`).
use super::Game;
use horizon_client::menu::Env;







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
}
