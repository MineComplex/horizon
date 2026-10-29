//! The horse inventory for a server's players (`ServerPlayer.openHorseInventory`):
//! shift + use on a tamed horse, donkey, mule, camel or llama, or the
//! inventory key while riding one (`ServerboundPlayerCommandPacket`
//! OPEN_INVENTORY), opens `HorseInventoryMenu` over the mount with
//! `ClientboundMountScreenOpenPacket`. Clicks run on the menu engine
//! (`Kind::Mount`) with the mount's saddle, body item and chest copied in and
//! written back, and the slots go to the client after each change, as
//! `broadcastChanges` does.
use super::{net_crafting::input, server_net::Network};
use crate::hc::{
    items::ItemStack,
    menu::{Kind, Menu},
};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// A player's open mount menu.
pub(super) struct PlayerMount {
    window: i32,
    mob: u32,
    columns: usize,
    state: i32,
    /// The menu's slots and the carried stack the client was last sent.
    sent: Option<(Vec<Option<ItemStack>>, Option<ItemStack>)>,
}

impl crate::game::Sim<'_> {
    /// `ServerPlayer.openHorseInventory` for mount `mob`: the open menu
    /// closes, the client gets the mount screen and the menu's slots.
    pub(super) fn net_open_mount(&mut self, net: &mut Network, conn: u32, mob: u32) {
        let Some(index) = self.mobs.iter().position(|m| m.id == mob) else {
            return;
        };
        let columns = super::mount_menu::mount_columns(&self.mobs[index]);
        self.net_mount_closed(net, conn, None);
        self.net_station_closed(net, conn, None);
        self.net_crafting_closed(net, conn, None);
        self.player_container_closed(net, conn);
        let Some(window) = net.hub.open_mount_window(conn, columns as i32, mob as i32) else {
            return;
        };
        net.play.entry(conn).or_default().mount_window = Some(PlayerMount {
            window,
            mob,
            columns,
            state: 0,
            sent: None,
        });
        self.net_send_mount(net, conn);
    }
    /// `HorseInventoryMenu` over mount `index`, with the player's inventory
    /// loaded by the caller.
    fn mount_engine_menu(&self, index: usize, columns: usize) -> Menu {
        let env = self.menu_env();
        let mut menu = Menu::new(Kind::Mount(columns));
        menu.mount = Some(self.mobs[index].kind.id.to_string());
        let (saddle, body, items) = self.mount_contents(index);
        menu.set_slot(&env, 0, saddle);
        menu.set_slot(&env, 1, body.flatten());
        for (k, item) in items.into_iter().take(columns * 3).enumerate() {
            menu.set_slot(&env, 2 + k, item);
        }
        menu
    }
    /// The menu's slots (the mount's, then the inventory and the hotbar)
    /// and the carried stack, sent when they changed since the last time.
    fn net_send_mount(&mut self, net: &mut Network, conn: u32) {
        let Some(gm) = net.play.get(&conn).and_then(|p| p.mount_window.as_ref()) else {
            return;
        };
        let (mob, columns) = (gm.mob, gm.columns);
        let Some(index) = self.mobs.iter().position(|m| m.id == mob) else {
            return;
        };
        let Some(g) = net.hub.players.iter().find(|g| g.conn == conn) else {
            return;
        };
        let (saddle, body, items) = self.mount_contents(index);
        let mut slots = vec![saddle, body.flatten()];
        slots.extend(items.into_iter().take(columns * 3));
        slots.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let now = Some((slots, carried));
        let Some(gm) = net
            .play
            .get_mut(&conn)
            .and_then(|p| p.mount_window.as_mut())
        else {
            return;
        };
        if gm.sent == now {
            return;
        }
        gm.state = (gm.state + 1) & 0x7FFF;
        let (window, state) = (gm.window, gm.state);
        let (slots, carried) = now.clone().unwrap_or_default();
        gm.sent = now;
        let content = C::ContainerSetContent(play::ContainerSetContent {
            container: window,
            state,
            items: slots.iter().map(|s| net.hub.stack(s)).collect(),
            carried: net.hub.stack(&carried),
        });
        net.hub.send_to(conn, &content);
    }
    /// A click on the player's mount window; false for other windows.
    pub(super) fn net_mount_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some((mob, columns)) = net
            .play
            .get(&conn)
            .and_then(|pl| pl.mount_window.as_ref())
            .filter(|m| m.window == p.container)
            .map(|m| (m.mob, m.columns))
        else {
            return false;
        };
        if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
            && !net.hub.players[gi].spectator()
            && let Some(index) = self.mobs.iter().position(|m| m.id == mob)
        {
            let mut menu = self.mount_engine_menu(index, columns);
            self.player_menu_op(net, gi, &mut menu, |menu, env| {
                let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            });
            self.net_mount_store(index, columns, &menu);
        }
        // The client is told what the server has, whatever it predicted.
        if let Some(gm) = net
            .play
            .get_mut(&conn)
            .and_then(|pl| pl.mount_window.as_mut())
        {
            gm.sent = None;
        }
        self.net_send_mount(net, conn);
        true
    }
    fn net_mount_store(&mut self, index: usize, columns: usize, menu: &Menu) {
        let (_, body, _) = self.mount_contents(index);
        let body = body.map(|_| menu.slot(1).cloned());
        let items = crate::game::Sim::net_store_slots(menu, columns);
        self.mount_store(index, menu.slot(0).cloned(), body, items);
    }
    /// `AbstractContainerMenu.removed` of the player's mount menu (`window`;
    /// None closes it whichever it is): the carried stack goes back to the
    /// inventory. False when no such window is open.
    pub(super) fn net_mount_closed(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let closed = net
            .play
            .get_mut(&conn)
            .and_then(|p| {
                p.mount_window
                    .take_if(|m| window.is_none_or(|w| m.window == w))
            })
            .is_some();
        if closed
            && let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
            && let Some(carried) = net.hub.players[gi].carried.take()
        {
            self.net_place_back(net, gi, vec![carried]);
        }
        closed
    }
    /// Each tick: a mount menu whose mount died or is out of reach closes
    /// (`AbstractMountInventoryMenu.stillValid`: alive, within the entity
    /// interaction range plus 4 of the eyes), and the others follow the
    /// mount's and the inventory's slots.
    pub(super) fn tick_net_mount_menus(&mut self, net: &mut Network) {
        let open: Vec<(u32, u32, i32)> = net
            .play
            .iter()
            .filter_map(|(c, p)| p.mount_window.as_ref().map(|m| (*c, m.mob, m.window)))
            .collect();
        for (conn, mob, window) in open {
            let Some(g) = net.hub.players.iter().find(|g| g.conn == conn) else {
                continue;
            };
            let eye = [g.position[0], g.position[1] + g.eye_height(), g.position[2]];
            let valid = self.mobs.iter().find(|m| m.id == mob).is_some_and(|m| {
                let (w, h) = (f64::from(m.width()) / 2.0, f64::from(m.height()));
                let p = [m.position.x, m.position.y, m.position.z];
                let lo = [p[0] - w, p[1], p[2] - w];
                let hi = [p[0] + w, p[1] + h, p[2] + w];
                let d: f64 = (0..3)
                    .map(|k| (eye[k] - eye[k].clamp(lo[k], hi[k])).powi(2))
                    .sum();
                // Player.ENTITY_INTERACTION_RANGE 3, plus 4.
                m.alive() && d < 7.0 * 7.0
            });
            if valid {
                self.net_send_mount(net, conn);
            } else {
                self.net_mount_closed(net, conn, Some(window));
                net.hub.send_to(
                    conn,
                    &C::ContainerClose(play::ContainerClose { container: window }),
                );
            }
        }
    }
}

impl crate::game::Sim<'_> {
    /// The engine menu's mount slots back on mount `index`.
    pub(crate) fn net_store_slots(menu: &Menu, columns: usize) -> Vec<Option<ItemStack>> {
        (0..columns * 3)
            .map(|k| menu.slot(2 + k).cloned())
            .collect()
    }
}
