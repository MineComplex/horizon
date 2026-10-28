//! Trading for LAN guests: the villager screen a guest opens runs the same
//! merchant menu as the host's (`horizon_client::menu`, `Kind::Merchant`),
//! with vanilla's packets: OpenScreen (`minecraft:merchant`), MerchantOffers,
//! the select trade packet, container clicks and the content sync after each.
use super::{Game, lan::LanHost, stations::Kind as StationKind, trading::MerchantState};
use horizon_client::{
    items::ItemStack,
    
    merchant};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// A guest's open trading screen.
pub(super) struct GuestMerchant {
    pub(super) window: i32,
    state: i32,
    /// The payment slots and the result.
    slots: [Option<ItemStack>; 3],
    pub(super) merchant: Box<MerchantState>,
    /// The offers the client was last sent.
    sent: Vec<merchant::MerchantOffer>,
    sent_level: i32,
}

/// A guest's UUID as gossip targets it.
fn uuid_words(uuid: u128) -> [i32; 4] {
    [
        (uuid >> 96) as i32,
        (uuid >> 64) as i32,
        (uuid >> 32) as i32,
        uuid as i32,
    ]
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// A trading screen the guest's interaction opened (it opened as a
    /// station while the guest acted): handed to the guest.
    pub(super) fn lan_open_merchant(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        mut state: MerchantState,
    ) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let uuid = uuid_words(host.hub.guests[gi].uuid);
        if let Some(id) = state.mob
            && state.session.villager
        {
            // startTrading with the guest's reputation. The session already
            // captured their Hero effect while they acted as the player.
            state.session.reputation = self.villager_reputation_of(id, uuid);
            state.session.update_special_prices();
        }
        // InventoryOpenEvent: cancelled, no trades.
        let title = self.merchant_display_name(&state);
        // The container or workstation the trading screen replaces is closed.
        self.guest_container_closed(host, conn);
        self.lan_station_closed(host, conn, None);
        let Some(window) = host.hub.open_merchant_window(conn, title) else {
            self.merchant_closed(&mut state);
            return;
        };
        let mut gm = GuestMerchant {
            window,
            state: 0,
            slots: [None, None, None],
            merchant: Box::new(state),
            sent: Vec::new(),
            sent_level: 0,
        };
        self.lan_send_offers(host, conn, &mut gm, true);
        self.lan_send_merchant_content(host, conn, &mut gm);
        host.play.entry(conn).or_default().merchant = Some(gm);
    }
    /// `Entity.getDisplayName` of the merchant, the screen's title: its name
    /// (custom, else the profession's) with the show_entity hover event and
    /// its UUID as insertion.
    fn merchant_display_name(&mut self, state: &MerchantState) -> horizon_world::nbt::Tag {
        use horizon_world::nbt::Tag;
        let s = &state.session;
        let (kind, key) = if s.villager {
            let p = s.profession.trim_start_matches("minecraft:");
            (
                "minecraft:villager",
                format!("entity.minecraft.villager.{p}"),
            )
        } else {
            (
                "minecraft:wandering_trader",
                "entity.minecraft.wandering_trader".to_owned(),
            )
        };
        let name = match &state.name {
            Some(n) => horizon_protocol::text::literal(n),
            None => Tag::compound([("translate", Tag::String(key.as_str().into()))]),
        };
        let uuid = state.mob.map_or([0; 4], |id| self.mob_uuid(id));
        let w = uuid.map(|v| v as u32);
        let text = format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:04x}{:08x}",
            w[0],
            w[1] >> 16,
            w[1] & 0xffff,
            w[2] >> 16,
            w[2] & 0xffff,
            w[3]
        );
        let mut out = match name.clone() {
            Tag::Compound(c) => c,
            other => {
                let mut c = horizon_world::nbt::Compound::new();
                c.insert("text".into(), other);
                c
            }
        };
        out.insert(
            "hover_event".into(),
            Tag::compound([
                ("action", Tag::String("show_entity".into())),
                ("id", Tag::String(kind.into())),
                ("uuid", Tag::IntArray(uuid.to_vec())),
                ("name", name),
            ]),
        );
        out.insert("insertion".into(), Tag::String(text.as_str().into()));
        Tag::Compound(out)
    }
    /// The screen title: the custom name, else the profession (the client
    /// adds the level itself).
    /// `ServerPlayer.sendMerchantOffers`, when the offers changed.
    fn lan_send_offers(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        gm: &mut GuestMerchant,
        force: bool,
    ) {
        let s = &gm.merchant.session;
        if !force && s.offers == gm.sent && s.level == gm.sent_level {
            return;
        }
        let offers: Vec<play::NetOffer> = s
            .offers
            .iter()
            .filter_map(|o| {
                let cost = |st: &ItemStack, hub: &mut horizon_client::lan::hub::Hub| {
                    hub.stack(&Some(st.clone())).map(play::NetCost::from)
                };
                Some(play::NetOffer {
                    cost_a: cost(&o.base_cost_a.stack, &mut host.hub)?,
                    result: host.hub.stack(&Some(o.result.clone()))?,
                    cost_b: match &o.cost_b {
                        Some(b) => Some(cost(&b.stack, &mut host.hub)?),
                        None => None,
                    },
                    out_of_stock: o.out_of_stock(),
                    uses: o.uses,
                    max_uses: o.max_uses,
                    xp: o.xp,
                    special_price: o.special_price_diff,
                    price_multiplier: o.price_multiplier,
                    demand: o.demand,
                })
            })
            .collect();
        host.hub.send_to(
            conn,
            &C::MerchantOffers(play::MerchantOffers {
                container: gm.window,
                offers,
                level: s.level,
                xp: s.xp,
                show_progress: s.villager,
                can_restock: s.villager,
            }),
        );
        gm.sent = s.offers.clone();
        gm.sent_level = s.level;
    }
    fn lan_send_merchant_content(&mut self, host: &mut LanHost, conn: u32, gm: &mut GuestMerchant) {
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let mut items: Vec<Option<ItemStack>> = gm.slots.to_vec();
        let g = &host.hub.guests[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let net: Vec<_> = items.iter().map(|s| host.hub.stack(s)).collect();
        let carried = host.hub.stack(&carried);
        gm.state = (gm.state + 1) & 0x7FFF;
        host.hub.send_to(
            conn,
            &C::ContainerSetContent(play::ContainerSetContent {
                container: gm.window,
                state: gm.state,
                items: net,
                carried,
            }),
        );
    }
    /// Takes a station a guest's action opened: a trading screen or a
    /// workstation menu goes to the guest (game/lan_stations.rs).
    pub(super) fn lan_station_opened(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        station: super::stations::Station,
    ) {
        let mut station = station;
        if station.kind == StationKind::Merchant
            && let Some(m) = station.merchant.take()
        {
            self.lan_open_merchant(host, conn, *m);
            return;
        }
        self.lan_open_station(host, conn, &station);
    }
}
