//! The horse inventory for a server's players (`ServerPlayer.openHorseInventory`):
//! shift + use on a tamed horse, donkey, mule, camel or llama, or the
//! inventory key while riding one (`ServerboundPlayerCommandPacket`
//! OPEN_INVENTORY), opens `HorseInventoryMenu` over the mount with
//! `ClientboundMountScreenOpenPacket`. Clicks run on the menu engine
//! (`Kind::Mount`) with the mount's saddle, body item and chest copied in and
//! written back, and the slots go to the client after each change, as
//! `broadcastChanges` does.
use super::{Game, lan::LanHost};
use horizon_client::items::ItemStack;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// A guest's open mount menu.
pub(super) struct GuestMount {
    window: i32,
    mob: u32,
    columns: usize,
    state: i32,
    /// The menu's slots and the carried stack the client was last sent.
    sent: Option<(Vec<Option<ItemStack>>, Option<ItemStack>)>,
}

impl Game<'_> {
}
impl crate::game::Sim<'_> {
    /// `ServerPlayer.openHorseInventory` for mount `mob`: the open menu
    /// closes, the client gets the mount screen and the menu's slots.
    pub(super) fn lan_open_mount(&mut self, host: &mut LanHost, conn: u32, mob: u32) {
        let Some(index) = self.mobs.iter().position(|m| m.id == mob) else {
            return;
        };
        let columns = super::mount_menu::mount_columns(&self.mobs[index]);
        self.lan_mount_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let Some(window) = host.hub.open_mount_window(conn, columns as i32, mob as i32) else {
            return;
        };
        host.play.entry(conn).or_default().mount_window = Some(GuestMount {
            window,
            mob,
            columns,
            state: 0,
            sent: None,
        });
        self.lan_send_mount(host, conn);
    }
    /// The menu's slots (the mount's, then the inventory and the hotbar)
    /// and the carried stack, sent when they changed since the last time.
    fn lan_send_mount(&mut self, host: &mut LanHost, conn: u32) {
        let Some(gm) = host.play.get(&conn).and_then(|p| p.mount_window.as_ref()) else {
            return;
        };
        let (mob, columns) = (gm.mob, gm.columns);
        let Some(index) = self.mobs.iter().position(|m| m.id == mob) else {
            return;
        };
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return;
        };
        let (saddle, body, items) = self.mount_contents(index);
        let mut slots = vec![saddle, body.flatten()];
        slots.extend(items.into_iter().take(columns * 3));
        slots.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let now = Some((slots, carried));
        let Some(gm) = host
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
            items: slots.iter().map(|s| host.hub.stack(s)).collect(),
            carried: host.hub.stack(&carried),
        });
        host.hub.send_to(conn, &content);
    }
    /// `AbstractContainerMenu.removed` of the guest's mount menu (`window`;
    /// None closes it whichever it is): the carried stack goes back to the
    /// inventory. False when no such window is open.
    pub(super) fn lan_mount_closed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let closed = host
            .play
            .get_mut(&conn)
            .and_then(|p| {
                p.mount_window
                    .take_if(|m| window.is_none_or(|w| m.window == w))
            })
            .is_some();
        if closed
            && let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn)
            && let Some(carried) = host.hub.guests[gi].carried.take()
        {
            self.lan_place_back(host, gi, vec![carried]);
        }
        closed
    }
}
