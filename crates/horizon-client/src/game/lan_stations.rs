//! Workstation menus for LAN guests: the screen a guest's use opens runs on
//! the host's menu engine (`horizon_client::menu`), the one single player
//! uses, with vanilla's packets: OpenScreen with the station's `MenuType`,
//! container clicks, `ServerboundContainerButtonClickPacket`, the content and
//! data slot sync after each (`broadcastChanges`), and the inputs back into
//! the inventory when the menu closes (`AbstractContainerMenu.clearContainer`).
use super::{
    Game,
    lan::LanHost,
    stations::{Kind as StationKind, Station}};
use horizon_client::{
    containers::BlockEntity,
    items::ItemStack,
    menu::{Kind, Menu},
};
use horizon_core::random::RandomSource;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// A guest's open workstation menu.
pub(super) struct GuestStation {
    window: i32,
    state: i32,
    kind: StationKind,
    pos: [i32; 3],
    menu: Box<Menu>,
    /// The data slots the client was last sent.
    sent_data: Vec<i32>,
    /// The game time of the menu's last sound (`lastSoundTime`).
    last_sound: u64,
}

/// A guest's open lectern: `LecternMenu` over the block entity, made on the
/// menu engine for each click (`Game::lectern_menu`).
pub(super) struct GuestLectern {
    window: i32,
    state: i32,
    key: (i32, i32, i32),
    /// The book and page the client was last sent (`broadcastChanges`).
    sent: Option<(Option<ItemStack>, i32)>,
}

/// The `MenuType`, the engine's menu and the input slots
/// (`clearContainer`'s container) of a station a guest can use.
fn menu_of(kind: StationKind) -> Option<(&'static str, Kind, std::ops::Range<usize>)> {
    Some(match kind {
        StationKind::Loom => ("minecraft:loom", Kind::Loom, 0..3),
        StationKind::Stonecutter => ("minecraft:stonecutter", Kind::Stonecutter, 0..1),
        StationKind::Grindstone => ("minecraft:grindstone", Kind::Grindstone, 0..2),
        StationKind::Anvil => ("minecraft:anvil", Kind::Anvil, 0..2),
        StationKind::Smithing => ("minecraft:smithing", Kind::Smithing, 0..3),
        StationKind::Enchanting => ("minecraft:enchantment", Kind::Enchanting, 0..2),
        StationKind::Beacon => ("minecraft:beacon", Kind::Beacon, 0..1),
        StationKind::Cartography => ("minecraft:cartography_table", Kind::Cartography, 0..2),
        _ => return None,
    })
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// A workstation a guest's use opened (`ServerPlayer.openMenu`); false
    /// when guests can't use that one yet.
    pub(super) fn lan_open_station(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        station: &Station,
    ) -> bool {
        let Some((menu_type, kind, _)) = menu_of(station.kind) else {
            return false;
        };
        // InventoryOpenEvent: cancelled, the use opens nothing.
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        // The container it replaces loses its opener.
        self.guest_container_closed(host, conn);
        let title = horizon_protocol::text::translatable(station.kind.title_key(), &[]);
        let Some(window) = host.hub.open_window(conn, menu_type, title) else {
            return true;
        };
        let mut menu = Box::new(Menu::new(kind));
        if let Some(g) = host.hub.guests.iter_mut().find(|g| g.conn == conn)
            && kind == Kind::Enchanting
        {
            // Player.enchantmentSeed, made when first needed.
            if g.xp_seed == 0 {
                g.xp_seed = self.world.random.next_int();
            }
            menu.data[3] = g.xp_seed;
        }
        if kind == Kind::Beacon {
            menu.data = self.beacon_data(host, station.pos).to_vec();
        }
        host.play.entry(conn).or_default().station = Some(GuestStation {
            window,
            state: 0,
            kind: station.kind,
            pos: station.pos,
            menu,
            sent_data: Vec::new(),
            last_sound: 0,
        });
        self.lan_send_station(host, conn);
        true
    }
    /// `LecternBlock.openScreen` for a guest: `minecraft:lectern` over the
    /// block entity, with its book and page.
    pub(super) fn lan_open_lectern(&mut self, host: &mut LanHost, conn: u32, key: (i32, i32, i32)) {
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let title = horizon_protocol::text::translatable("container.lectern", &[]);
        let Some(window) = host.hub.open_window(conn, "minecraft:lectern", title) else {
            return;
        };
        host.play.entry(conn).or_default().lectern = Some(GuestLectern {
            window,
            state: 0,
            key,
            sent: None,
        });
        self.lan_send_lectern(host, conn, true);
    }
    /// The lectern's book and page for the client: everything (`all`), or
    /// what changed since it was last sent.
    fn lan_send_lectern(&mut self, host: &mut LanHost, conn: u32, all: bool) {
        let Some(gl) = host.play.get_mut(&conn).and_then(|p| p.lectern.as_mut()) else {
            return;
        };
        let (book, page) = match self.world.block_entities.get(&gl.key) {
            Some(BlockEntity::Lectern(l)) => (l.book.clone(), l.page),
            _ => (None, 0),
        };
        let now = Some((book.clone(), page));
        if !all && gl.sent == now {
            return;
        }
        let page_changed = gl.sent.as_ref().is_none_or(|(_, p)| *p != page);
        gl.sent = now;
        gl.state = (gl.state + 1) & 0x7FFF;
        let (window, state) = (gl.window, gl.state);
        let carried = host
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn)
            .and_then(|g| g.carried.clone());
        let content = C::ContainerSetContent(play::ContainerSetContent {
            container: window,
            state,
            items: vec![host.hub.stack(&book)],
            carried: host.hub.stack(&carried),
        });
        host.hub.send_to(conn, &content);
        if all || page_changed {
            host.hub.send_to(
                conn,
                &C::ContainerSetData(play::ContainerSetData {
                    container: window,
                    id: 0,
                    value: page as i16,
                }),
            );
        }
    }
    /// The whole station menu and its changed data slots for the client
    /// (`sendAllDataToRemote` / `broadcastChanges`).
    fn lan_send_station(&mut self, host: &mut LanHost, conn: u32) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some(gs) = host.play.get_mut(&conn).and_then(|p| p.station.as_mut()) else {
            return;
        };
        gs.state = (gs.state + 1) & 0x7FFF;
        let own = gs.menu.slot_count() - 36;
        let mut items: Vec<Option<ItemStack>> =
            (0..own).map(|s| gs.menu.slot(s).cloned()).collect();
        let g = &host.hub.guests[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let packet = C::ContainerSetContent(play::ContainerSetContent {
            container: gs.window,
            state: gs.state,
            items: items.iter().map(|s| host.hub.stack(s)).collect(),
            carried: host.hub.stack(&carried),
        });
        gs.sent_data.resize(gs.menu.data.len(), i32::MIN);
        let mut data = Vec::new();
        for (k, &v) in gs.menu.data.iter().enumerate() {
            if gs.sent_data[k] != v {
                gs.sent_data[k] = v;
                data.push(C::ContainerSetData(play::ContainerSetData {
                    container: gs.window,
                    id: k as i16,
                    value: v as i16,
                }));
            }
        }
        host.hub.send_to(conn, &packet);
        for packet in data {
            host.hub.send_to(conn, &packet);
        }
    }
    /// `AbstractContainerMenu.removed` of the guest's station (`window`;
    /// None closes it whichever it is): the carried stack and the inputs go
    /// back to the inventory, what doesn't fit is dropped. False when no such
    /// window is open.
    pub(super) fn lan_station_closed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        // A lectern's menu keeps nothing of its own (the book is the block entity's).
        let lectern = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.lectern.take_if(|l| window.is_none_or(|w| l.window == w)));
        if lectern.is_some() {
            // AbstractContainerMenu.removed: the carried stack goes back.
            if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn)
                && let Some(carried) = host.hub.guests[gi].carried.take()
            {
                self.lan_place_back(host, gi, vec![carried]);
            }
            if window.is_some() {
                return true;
            }
        }
        let Some(gs) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.station.take_if(|s| window.is_none_or(|w| s.window == w)))
        else {
            return false;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return true;
        };
        // AbstractContainerMenu.removed: the carried stack first, then the inputs.
        let mut back: Vec<ItemStack> = host.hub.guests[gi].carried.take().into_iter().collect();
        let inputs = station_inputs(&gs);
        if gs.kind == StationKind::Beacon {
            // BeaconMenu.removed drops the payment (player.drop).
            if !back.is_empty() {
                self.lan_place_back(host, gi, std::mem::take(&mut back));
            }
            self.drop_guest_items(host, gi, inputs);
        } else {
            back.extend(inputs);
        }
        if !back.is_empty() {
            self.lan_place_back(host, gi, back);
        }
        true
    }
    /// `BeaconBlockEntity.dataAccess`: the pyramid's levels and the powers
    /// (`BeaconMenu.encodeEffect`: the `mob_effect` id + 1, 0 for none).
    fn beacon_data(&self, host: &LanHost, pos: [i32; 3]) -> [i32; 3] {
        let Some(BlockEntity::Beacon(b)) = self.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        else {
            return [0; 3];
        };
        let encode = |e: &Option<String>| {
            e.as_deref()
                .and_then(|n| host.hub.registry_id("minecraft:mob_effect", n))
                .map_or(0, |id| id + 1)
        };
        [b.levels, encode(&b.primary), encode(&b.secondary)]
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
}

/// What `clearContainer` gives back from a station's menu.
pub(super) fn station_inputs(gs: &GuestStation) -> Vec<ItemStack> {
    menu_of(gs.kind)
        .map(|(_, _, inputs)| inputs.filter_map(|s| gs.menu.slot(s).cloned()).collect())
        .unwrap_or_default()
}
