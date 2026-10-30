//! Workstation menus for players: the screen a player's use opens runs on
//! the game's menu engine (`crate::hc::menu`), with vanilla's packets: OpenScreen with the station's `MenuType`,
//! container clicks, `ServerboundContainerButtonClickPacket`, the content and
//! data slot sync after each (`broadcastChanges`), and the inputs back into
//! the inventory when the menu closes (`AbstractContainerMenu.clearContainer`).
use super::{
    net_crafting::input,
    server_net::Network,
    stations::{Kind as StationKind, Station},
};
use crate::hc::{
    containers::BlockEntity,
    items::ItemStack,
    menu::{Kind, Menu},
};
use horizon_core::random::RandomSource;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// A player's open workstation menu.
pub(super) struct PlayerStation {
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

/// A player's open lectern: `LecternMenu` over the block entity, made on the
/// menu engine for each click (`crate::game::Server::lectern_menu`).
pub(super) struct PlayerLectern {
    window: i32,
    state: i32,
    key: (i32, i32, i32),
    /// The book and page the client was last sent (`broadcastChanges`).
    sent: Option<(Option<ItemStack>, i32)>,
}

/// The `MenuType`, the engine's menu and the input slots
/// (`clearContainer`'s container) of a station a player can use.
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

impl crate::game::Server<'_> {
    /// A workstation a player's use opened (`ServerPlayer.openMenu`); false
    /// when players can't use that one yet.
    pub(super) fn net_open_station(
        &mut self,
        net: &mut Network,
        conn: u32,
        station: &Station,
    ) -> bool {
        let Some((menu_type, kind, _)) = menu_of(station.kind) else {
            return false;
        };
        // InventoryOpenEvent: cancelled, the use opens nothing.
        if !self.plugin_inventory_open(net, conn, menu_type, 0) {
            return true;
        }
        self.net_station_closed(net, conn, None);
        self.net_crafting_closed(net, conn, None);
        // The container it replaces loses its opener.
        self.player_container_closed(net, conn);
        let title = horizon_protocol::text::translatable(station.kind.title_key(), &[]);
        let Some(window) = net.hub.open_window(conn, menu_type, title) else {
            return true;
        };
        let mut menu = Box::new(Menu::new(kind));
        if let Some(g) = net.hub.players.iter_mut().find(|g| g.conn == conn)
            && kind == Kind::Enchanting
        {
            // Player.enchantmentSeed, made when first needed.
            if g.xp_seed == 0 {
                g.xp_seed = self.world.random.next_int();
            }
            menu.data[3] = g.xp_seed;
        }
        if kind == Kind::Beacon {
            menu.data = self.beacon_data(net, station.pos).to_vec();
        }
        net.play.entry(conn).or_default().station = Some(PlayerStation {
            window,
            state: 0,
            kind: station.kind,
            pos: station.pos,
            menu,
            sent_data: Vec::new(),
            last_sound: 0,
        });
        self.net_send_station(net, conn);
        true
    }
    /// Runs `op` on the player's open station (window `window`, any when
    /// None) with their inventory in it, then sends what changed. False when
    /// no such window is open.
    fn net_station_op(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: Option<i32>,
        op: impl FnOnce(&mut Menu, &crate::hc::menu::Env),
    ) -> bool {
        let Some(mut gs) = net
            .play
            .get_mut(&conn)
            .and_then(|p| p.station.take_if(|s| window.is_none_or(|w| s.window == w)))
        else {
            return false;
        };
        if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
            && !net.hub.players[gi].spectator()
        {
            self.player_menu_op(net, gi, &mut gs.menu, op);
            self.net_station_sounds(&mut gs);
            if let Some(amount) = gs.menu.ground.take() {
                self.grindstone_taken(gs.pos, amount);
            }
            self.net_station_combined(net, conn, &mut gs);
            if let Some(button) = gs.menu.enchant_button.take() {
                self.net_enchant(net, gi, &mut gs, button);
            }
            if std::mem::take(&mut gs.menu.enchant_changed) {
                self.net_enchant_offers(net, gi, &mut gs);
            }
        }
        net.play.entry(conn).or_default().station = Some(gs);
        self.net_send_station(net, conn);
        true
    }
    /// `LecternBlock.openScreen` for a player: `minecraft:lectern` over the
    /// block entity, with its book and page.
    pub(super) fn net_open_lectern(&mut self, net: &mut Network, conn: u32, key: (i32, i32, i32)) {
        self.net_station_closed(net, conn, None);
        self.net_crafting_closed(net, conn, None);
        self.player_container_closed(net, conn);
        let title = horizon_protocol::text::translatable("container.lectern", &[]);
        let Some(window) = net.hub.open_window(conn, "minecraft:lectern", title) else {
            return;
        };
        net.play.entry(conn).or_default().lectern = Some(PlayerLectern {
            window,
            state: 0,
            key,
            sent: None,
        });
        self.net_send_lectern(net, conn, true);
    }
    /// The lectern's book and page for the client: everything (`all`), or
    /// what changed since it was last sent.
    fn net_send_lectern(&mut self, net: &mut Network, conn: u32, all: bool) {
        let Some(gl) = net.play.get_mut(&conn).and_then(|p| p.lectern.as_mut()) else {
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
        let carried = net
            .hub
            .players
            .iter()
            .find(|g| g.conn == conn)
            .and_then(|g| g.carried.clone());
        let content = C::ContainerSetContent(play::ContainerSetContent {
            container: window,
            state,
            items: vec![net.hub.stack(&book)],
            carried: net.hub.stack(&carried),
        });
        net.hub.send_to(conn, &content);
        if all || page_changed {
            net.hub.send_to(
                conn,
                &C::ContainerSetData(play::ContainerSetData {
                    container: window,
                    id: 0,
                    value: page as i16,
                }),
            );
        }
    }
    /// Runs `op` on the player's lectern menu (window `window`) with their
    /// inventory, the block entity takes the book and page back, and the
    /// client gets what changed. False when no such window is open.
    fn net_lectern_op(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: i32,
        op: impl FnOnce(&mut Menu, &crate::hc::menu::Env),
    ) -> bool {
        let Some(key) = net
            .play
            .get(&conn)
            .and_then(|p| p.lectern.as_ref())
            .filter(|l| l.window == window)
            .map(|l| l.key)
        else {
            return false;
        };
        if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
            && !net.hub.players[gi].spectator()
            && let Some(mut menu) = self.lectern_menu(key)
        {
            // Player.mayBuild: survival and creative.
            let may_build = matches!(net.hub.players[gi].game_mode, 0 | 1);
            self.player_menu_op(net, gi, &mut menu, |menu, env| {
                menu.may_build = may_build;
                op(menu, env);
            });
            self.lectern_menu_done(key, &menu);
        }
        self.net_send_lectern(net, conn, true);
        true
    }
    /// A click on the player's lectern window; false for other windows.
    pub(super) fn net_lectern_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        self.net_lectern_op(net, conn, p.container, |menu, env| {
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
        })
    }
    /// The rest of a taken anvil or smithing result's `onTake`: the player
    /// pays the levels, the anvil wears (level event 1030 or 1029), the
    /// smithing table sounds (1044).
    fn net_station_combined(&mut self, net: &mut Network, conn: u32, gs: &mut PlayerStation) {
        let change = std::mem::take(&mut gs.menu.level_change);
        if change != 0 {
            self.as_player(net, conn, |g| g.give_levels(change));
        }
        if !std::mem::take(&mut gs.menu.combined) {
            return;
        }
        if gs.kind == StationKind::Smithing {
            self.level_event_at(1044, gs.pos, 0);
        } else {
            let creative = gs.menu.creative;
            // A broken anvil closes the menu on the next stillValid check.
            self.anvil_used(gs.pos, creative);
        }
    }
    /// `EnchantmentMenu.slotsChanged` with a level: the costs and clues for
    /// the player's seed and the bookshelves around the table.
    fn net_enchant_offers(&mut self, net: &Network, gi: usize, gs: &mut PlayerStation) {
        let seed = net.hub.players[gi].xp_seed;
        let bookcases = self.count_bookshelves(gs.pos);
        let offers = crate::hc::stations::enchant_offers(
            &self.res.stations().enchantments,
            self.res.items(),
            seed,
            bookcases,
            gs.menu.slot(0),
        );
        for i in 0..3 {
            gs.menu.data[i] = offers.costs[i];
            let clue = offers.clues[i].as_ref();
            gs.menu.data[4 + i] = clue
                .and_then(|(e, _)| net.hub.registry_id("minecraft:enchantment", e))
                .unwrap_or(-1);
            gs.menu.data[7 + i] = clue.map_or(-1, |(_, l)| *l as i32);
        }
        gs.menu.data[3] = seed;
    }
    /// `EnchantmentMenu.clickMenuButton`'s `access.execute`: the item gets
    /// the offer's enchantments, the player pays its levels and lapis and
    /// gets a new seed, the table sounds, and the offers are made again.
    fn net_enchant(&mut self, net: &mut Network, gi: usize, gs: &mut PlayerStation, button: usize) {
        let Some(item) = gs.menu.slot(0).cloned() else {
            return;
        };
        let g = &net.hub.players[gi];
        let (seed, creative, conn) = (g.xp_seed, g.creative(), g.conn);
        let Some(enchanted) = crate::hc::stations::apply_enchantment(
            &self.res.stations().enchantments,
            self.res.items(),
            seed,
            button,
            gs.menu.data[button],
            &item,
        ) else {
            return;
        };
        let need = button as i32 + 1;
        // EnchantItemEvent: a cancelled one enchants nothing.
        if !self.plugin_enchant(net, conn, &item, button) {
            return;
        }
        // Player.onEnchantmentPerformed, and EnchantmentMenu's awardStat(ENCHANT_ITEM).
        self.as_player(net, conn, |game| {
            game.give_levels(-need);
            game.stats.custom("enchant_item", 1);
            // CriteriaTriggers.ENCHANTED_ITEM, with the levels it cost.
            game.trigger_enchanted_item(&enchanted, need);
        });
        net.hub.players[gi].xp_seed = self.world.random.next_int();
        let env = self.menu_env();
        let lapis = gs.menu.slot(1).cloned().filter(|_| !creative).map(|mut l| {
            l.count = l.count.saturating_sub(need as u32);
            l
        });
        gs.menu.set_slot(&env, 0, Some(enchanted));
        if !creative {
            gs.menu.set_slot(&env, 1, lapis.filter(|l| l.count > 0));
        }
        gs.menu.enchant_changed = false;
        self.net_enchant_offers(net, gi, gs);
        let pitch = self.world.random.next_float() * 0.1 + 0.9;
        let at = gs.pos.map(|v| f64::from(v) + 0.5);
        self.sounds.play(
            "minecraft:block.enchantment_table.use",
            Some(at),
            1.0,
            pitch,
        );
    }
    /// `ServerGamePacketListenerImpl.handleRenameItem`: the player's anvil
    /// takes the name (`AnvilMenu.setItemName`).
    pub(super) fn net_station_rename(&mut self, net: &mut Network, conn: u32, name: &str) {
        let anvil = net
            .play
            .get(&conn)
            .and_then(|p| p.station.as_ref())
            .is_some_and(|s| s.kind == StationKind::Anvil);
        if anvil {
            self.net_station_op(net, conn, None, |menu, env| {
                menu.rename(env, name);
            });
        }
    }
    /// The sounds the menu played, at the station (heard by everyone around,
    /// as `level.playSound(null, pos, ...)`), once per tick.
    fn net_station_sounds(&mut self, gs: &mut PlayerStation) {
        let sounds = std::mem::take(&mut gs.menu.sounds);
        if sounds.is_empty() || gs.last_sound == self.ticks {
            return;
        }
        gs.last_sound = self.ticks;
        let at = gs.pos.map(|v| f64::from(v) + 0.5);
        for sound in sounds {
            self.sounds.play(sound, Some(at), 1.0, 1.0);
        }
    }
    /// A click on the player's station window; false for other windows.
    pub(super) fn net_station_click(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        self.net_station_op(net, conn, Some(p.container), |menu, env| {
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
        })
    }
    /// `ServerGamePacketListenerImpl.handleContainerButtonClick`:
    /// `clickMenuButton`, then `broadcastChanges` when the menu took it.
    pub(super) fn net_station_button(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: i32,
        id: i32,
    ) {
        // LecternMenu's page buttons and Take Book.
        if self.net_lectern_op(net, conn, window, |menu, env| {
            menu.button(env, id);
        }) {
            return;
        }
        self.net_station_op(net, conn, Some(window), |menu, env| {
            menu.button(env, id);
        });
    }
    /// The whole station menu and its changed data slots for the client
    /// (`sendAllDataToRemote` / `broadcastChanges`).
    fn net_send_station(&mut self, net: &mut Network, conn: u32) {
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some(gs) = net.play.get_mut(&conn).and_then(|p| p.station.as_mut()) else {
            return;
        };
        gs.state = (gs.state + 1) & 0x7FFF;
        let own = gs.menu.slot_count() - 36;
        let mut items: Vec<Option<ItemStack>> =
            (0..own).map(|s| gs.menu.slot(s).cloned()).collect();
        let g = &net.hub.players[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let packet = C::ContainerSetContent(play::ContainerSetContent {
            container: gs.window,
            state: gs.state,
            items: items.iter().map(|s| net.hub.stack(s)).collect(),
            carried: net.hub.stack(&carried),
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
        net.hub.send_to(conn, &packet);
        for packet in data {
            net.hub.send_to(conn, &packet);
        }
    }
    /// `AbstractContainerMenu.removed` of the player's station (`window`;
    /// None closes it whichever it is): the carried stack and the inputs go
    /// back to the inventory, what doesn't fit is dropped. False when no such
    /// window is open.
    pub(super) fn net_station_closed(
        &mut self,
        net: &mut Network,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        // A lectern's menu keeps nothing of its own (the book is the block entity's).
        let lectern = net
            .play
            .get_mut(&conn)
            .and_then(|p| p.lectern.take_if(|l| window.is_none_or(|w| l.window == w)));
        if lectern.is_some() {
            // AbstractContainerMenu.removed: the carried stack goes back.
            if let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn)
                && let Some(carried) = net.hub.players[gi].carried.take()
            {
                self.net_place_back(net, gi, vec![carried]);
            }
            if window.is_some() {
                return true;
            }
        }
        let Some(gs) = net
            .play
            .get_mut(&conn)
            .and_then(|p| p.station.take_if(|s| window.is_none_or(|w| s.window == w)))
        else {
            return false;
        };
        let Some(gi) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return true;
        };
        // AbstractContainerMenu.removed: the carried stack first, then the inputs.
        let mut back: Vec<ItemStack> = net.hub.players[gi].carried.take().into_iter().collect();
        let inputs = station_inputs(&gs);
        if gs.kind == StationKind::Beacon {
            // BeaconMenu.removed drops the payment (player.drop).
            if !back.is_empty() {
                self.net_place_back(net, gi, std::mem::take(&mut back));
            }
            self.drop_player_items(net, gi, inputs);
        } else {
            back.extend(inputs);
        }
        if !back.is_empty() {
            self.net_place_back(net, gi, back);
        }
        true
    }
    /// `BeaconBlockEntity.dataAccess`: the pyramid's levels and the powers
    /// (`BeaconMenu.encodeEffect`: the `mob_effect` id + 1, 0 for none).
    fn beacon_data(&self, net: &Network, pos: [i32; 3]) -> [i32; 3] {
        let Some(BlockEntity::Beacon(b)) = self.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        else {
            return [0; 3];
        };
        let encode = |e: &Option<String>| {
            e.as_deref()
                .and_then(|n| net.hub.registry_id("minecraft:mob_effect", n))
                .map_or(0, |id| id + 1)
        };
        [b.levels, encode(&b.primary), encode(&b.secondary)]
    }
    /// `ServerGamePacketListenerImpl.handleSetBeaconPacket`: the player's
    /// beacon menu takes the powers (`BeaconMenu.updateEffects`), and they
    /// become the block entity's (`dataAccess.set`: the power_select sound
    /// while the beam is up).
    pub(super) fn net_station_beacon(
        &mut self,
        net: &mut Network,
        conn: u32,
        primary: Option<i32>,
        secondary: Option<i32>,
    ) {
        let Some(pos) = net
            .play
            .get(&conn)
            .and_then(|p| p.station.as_ref())
            .filter(|s| s.kind == StationKind::Beacon)
            .map(|s| s.pos)
        else {
            return;
        };
        // An id the registry doesn't have fails the packet's codec.
        let mut powers: [Option<(i32, String)>; 2] = [None, None];
        for (k, id) in [primary, secondary].into_iter().enumerate() {
            if let Some(id) = id {
                let Some(name) = net.hub.registry_name("minecraft:mob_effect", id) else {
                    return;
                };
                powers[k] = Some((id, name.to_owned()));
            }
        }
        let data = self.beacon_data(net, pos);
        let mut updated = false;
        self.net_station_op(net, conn, None, |menu, env| {
            menu.data.copy_from_slice(&data);
            let power = |k: usize| powers[k].as_ref().map(|(id, n)| (*id, n.as_str()));
            updated = menu.beacon_update(env, power(0), power(1));
        });
        if !updated {
            return;
        }
        let [primary, secondary] = powers.map(|p| p.map(|(_, n)| n));
        let mut select_sound = false;
        if let Some(BlockEntity::Beacon(b)) =
            self.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
        {
            b.primary = primary;
            b.secondary = secondary;
            select_sound = !b.sections.is_empty();
        }
        if select_sound {
            self.beacon_sound(pos, "minecraft:block.beacon.power_select");
        }
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    }
    /// `ServerPlayer.doTick`'s `stillValid`: a player's station closes once its
    /// block is gone or the player is out of reach (`isWithinBlockInteractionRange(pos, 4)`).
    pub(super) fn net_stations_tick(&mut self, net: &mut Network) {
        self.net_lecterns_tick(net);
        let conns: Vec<u32> = net
            .play
            .iter()
            .filter(|(_, p)| p.station.is_some())
            .map(|(c, _)| *c)
            .collect();
        for conn in conns {
            let Some(g) = net.hub.players.iter().find(|g| g.conn == conn) else {
                continue;
            };
            if !net.hub.here(g) {
                continue;
            }
            let Some(gs) = net.play.get(&conn).and_then(|p| p.station.as_ref()) else {
                continue;
            };
            let [x, y, z] = gs.pos;
            let name = &self.res.facts().get(self.world.block(x, y, z)).name;
            let block = StationKind::from_block(name) == Some(gs.kind);
            if !(block && g.within_block_range(gs.pos, 4.0)) {
                let window = gs.window;
                self.net_station_closed(net, conn, Some(window));
                net.hub.send_to(
                    conn,
                    &C::ContainerClose(play::ContainerClose { container: window }),
                );
            } else if gs.kind == StationKind::Beacon {
                // The beacon's data slots follow its block entity.
                let data = self.beacon_data(net, gs.pos);
                if let Some(gs) = net.play.get_mut(&conn).and_then(|p| p.station.as_mut())
                    && gs.menu.data[..] != data[..]
                {
                    gs.menu.data.copy_from_slice(&data);
                    self.net_send_station(net, conn);
                }
            }
        }
    }
}

impl crate::game::Server<'_> {
    /// The players' lecterns each tick: `stillValid` (the block entity, its
    /// book, the reach) closes them, and `broadcastChanges` sends a book or
    /// page another player changed.
    fn net_lecterns_tick(&mut self, net: &mut Network) {
        let open: Vec<(u32, i32, (i32, i32, i32))> = net
            .play
            .iter()
            .filter_map(|(c, p)| p.lectern.as_ref().map(|l| (*c, l.window, l.key)))
            .collect();
        for (conn, window, key) in open {
            let Some(g) = net.hub.players.iter().find(|g| g.conn == conn) else {
                continue;
            };
            if !net.hub.here(g) {
                continue;
            }
            let has_book = matches!(
                self.world.block_entities.get(&key),
                Some(BlockEntity::Lectern(l)) if l.book.is_some()
            );
            // Container.stillValidBlockEntity: the interaction range and its buffer.
            let near = g.within_block_range([key.0, key.1, key.2], 4.0);
            if has_book && near {
                self.net_send_lectern(net, conn, false);
            } else {
                self.net_station_closed(net, conn, Some(window));
                net.hub.send_to(
                    conn,
                    &C::ContainerClose(play::ContainerClose { container: window }),
                );
            }
        }
    }
}

/// What `clearContainer` gives back from a station's menu.
pub(super) fn station_inputs(gs: &PlayerStation) -> Vec<ItemStack> {
    menu_of(gs.kind)
        .map(|(_, _, inputs)| inputs.filter_map(|s| gs.menu.slot(s).cloned()).collect())
        .unwrap_or_default()
}
