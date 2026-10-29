//! The merchant (villager trading) menu: `MerchantMenu` / `MerchantScreen`
//! layout (276x166, drawn 50 px left of the usual 176-wide origin), offer
//! buttons, payment slots, the result slot's `onTake`, villager experience and
//! level-ups with new offers from the next trade set.
use super::stations::{Kind, Station};
use crate::hc::mobs::{
    Species,
    render::{PROFESSIONS, VILLAGER_TYPES},
    villager,
};
use crate::hc::{items::ItemStack, merchant::{self, MerchantOffer, TradeContext}};


pub(super) struct MerchantState {
    /// The merchant's side of the menu: offers, experience, level, the
    /// selection hint and active offer (`crate::hc::merchant::Session`).
    pub(super) session: Box<merchant::Session>,
    /// The offer list's scroll offset.
    pub(super) scroll: usize,
    /// The offer button that keeps focus after a click (drawn highlighted).
    pub(super) focused: Option<usize>,
    /// The trading mob (its offers and experience are written back on
    /// close) and its custom name.
    pub(super) mob: Option<u32>,
    pub(super) name: Option<String>,
}
impl MerchantState {
}

/// `ExplorationMapFunction` in the game world: the nearest structure of the
/// tag through the generator's `/locate`, a new map centred on it with its
/// target decoration.
pub(super) struct GameMaps<'a> {
    pub(super) terrain: Option<std::sync::Arc<crate::hc::vanillagen::VanillaTerrain>>,
    /// Structure tag -> its structures.
    pub(super) structure_tag: &'a dyn Fn(&str) -> Vec<String>,
    /// Maps made, for the caller to register: (map id, centre x, z, scale, target).
    pub(super) made: std::cell::RefCell<Vec<(i32, [i32; 3], u8, String)>>,
    pub(super) next_id: std::cell::Cell<i32>,
}
impl merchant::MapMaker for GameMaps<'_> {
    fn exploration_map(
        &self,
        stack: ItemStack,
        map: &merchant::ExplorationMap,
        origin: [f64; 3],
    ) -> ItemStack {
        let Some(terrain) = self.terrain.as_ref() else {
            return stack;
        };
        let names = (self.structure_tag)(&map.destination);
        if names.is_empty() {
            return stack;
        }
        let from = [
            origin[0].floor() as i32,
            origin[1].floor() as i32,
            origin[2].floor() as i32,
        ];
        let Some((_, at)) = terrain.locate(&names, from, map.search_radius) else {
            return stack;
        };
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.made
            .borrow_mut()
            .push((id, at, map.zoom, map.decoration.clone()));
        // The trade's own map item (woodland_mansion_map, ...) keeps its id.
        let mut out = stack;
        let d = out.data_mut();
        d.map_id = Some(id);
        d.other.push((
            "minecraft:map_decorations".into(),
            horizon_world::nbt::Tag::compound([(
                "+",
                horizon_world::nbt::Tag::compound([
                    (
                        "type",
                        horizon_world::nbt::Tag::String(map.decoration.as_str().into()),
                    ),
                    (
                        "x",
                        horizon_world::nbt::Tag::Double(f64::from(at[0]).to_bits()),
                    ),
                    (
                        "z",
                        horizon_world::nbt::Tag::Double(f64::from(at[2]).to_bits()),
                    ),
                    (
                        "rotation",
                        horizon_world::nbt::Tag::Float(180.0f32.to_bits()),
                    ),
                ]),
            )]),
        ));
        out
    }
}

impl crate::game::Sim<'_> {
    pub(super) fn random_sequences_path(&self) -> std::path::PathBuf {
        self.world_root.join("data/minecraft/random_sequences.dat")
    }
    /// `RandomSequences` as `random_sequences.dat` (not on a server's guest).
    pub(super) fn save_random_sequences(&mut self) {
        if self.rand_sequences.ids().is_empty() {
            return;
        }
        let tag = horizon_world::nbt::Tag::compound([
            ("data", self.rand_sequences.to_nbt()),
            (
                "DataVersion",
                horizon_world::nbt::Tag::Int(horizon_world::DATA_VERSION),
            ),
        ]);
        if let Err(e) = crate::hc::save::write_dat(&self.random_sequences_path(), tag) {
            log::warn!("the villager random sequences were not written: {e}");
        }
    }
    /// `AbstractVillager.addOffersFromTradeSet` with the world's random
    /// sequence of the set, at the merchant's position.
    fn offers_from_set(
        &mut self,
        set: &str,
        variant: &str,
        origin: [f64; 3],
    ) -> Vec<MerchantOffer> {
        let Some(sequence) = self.res.stations().trades.sequence(set) else {
            return Vec::new();
        };
        let mut random = self.rand_sequences.lend(&sequence, self.level.seed);
        let next_map = self.peek_map_id();
        let tag = |t: &str| self.structure_tag(t);
        let maps = GameMaps {
            terrain: self.end.locator(),
            structure_tag: &tag,
            made: Default::default(),
            next_id: std::cell::Cell::new(next_map),
        };
        let ctx = TradeContext {
            items: self.res.items(),
            enchantments: &self.res.stations().enchantments,
            variant,
            origin,
            maps: Some(&maps),
        };
        let offers = self
            .res
            .stations()
            .trades
            .offers_from_set(&ctx, set, &mut random);
        let made = maps.made.into_inner();
        self.rand_sequences.give_back(sequence, random);
        self.register_exploration_maps(made);
        offers
    }
    /// `AbstractVillager.getOffers`: generated on first use.
    pub(super) fn ensure_offers(&mut self, index: usize) {
        let species = self.mobs[index].species();
        let unset = self.mobs[index]
            .extra
            .trade
            .as_ref()
            .is_none_or(|t| t.unset);
        if unset {
            let m = &self.mobs[index];
            let variant = format!(
                "minecraft:{}",
                VILLAGER_TYPES[m.extra.variant as usize % VILLAGER_TYPES.len()]
            );
            let origin = [m.position.x, m.position.y, m.position.z];
            let offers = if species == Species::WanderingTrader {
                let mut v = Vec::new();
                for set in ["buying", "uncommon", "common"] {
                    v.extend(self.offers_from_set(
                        &format!("minecraft:wandering_trader/{set}"),
                        &variant,
                        origin,
                    ));
                }
                v
            } else {
                let profession = PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()];
                let level = i32::from(m.extra.variant3.max(1));
                let set = merchant::Trades::set_for(profession, level);
                if self.res.stations().trades.sets.contains_key(&set) {
                    self.offers_from_set(&set, &variant, origin)
                } else {
                    Vec::new()
                }
            };
            let trade = self.mobs[index].extra.trade.get_or_insert_default();
            trade.offers = offers;
            trade.unset = false;
        }
    }
    /// `Villager.mobInteract` / `WanderingTrader.mobInteract`: shake the
    /// head without offers, else open the trading screen.
    pub(super) fn interact_trader(
        &mut self,
        index: usize,
        item: &str,
    ) -> crate::hc::Result<bool> {
        let m = &self.mobs[index];
        let species = m.species();
        if !matches!(species, Species::Villager | Species::WanderingTrader)
            || !m.alive()
            || item == "minecraft:villager_spawn_egg"
            || self.station.is_some()
            || m.extra.flag2
            || (species == Species::Villager && villager::sleeping(m))
        {
            return Ok(false);
        }
        if m.is_baby() {
            // A baby villager shakes its head; a baby wandering trader does
            // nothing of its own.
            if species == Species::WanderingTrader {
                return Ok(false);
            }
            let mut events = Vec::new();
            villager::set_unhappy(&mut self.mobs[index], &mut events);
            self.handle_mob_events(events)?;
            return Ok(true);
        }
        self.stats.custom("talked_to_villager", 1);
        self.ensure_offers(index);
        let m = &self.mobs[index];
        let trade = m.extra.trade.as_deref().cloned().unwrap_or_default();
        if trade.offers.is_empty() {
            if species == Species::Villager {
                let mut events = Vec::new();
                villager::set_unhappy(&mut self.mobs[index], &mut events);
                self.handle_mob_events(events)?;
            }
            return Ok(true);
        }
        let villager = species == Species::Villager;
        let profession = if villager {
            PROFESSIONS[m.extra.variant2 as usize % PROFESSIONS.len()]
        } else {
            "wandering_trader"
        };
        let level = if villager {
            i32::from(m.extra.variant3.max(1)).min(5)
        } else {
            1
        };
        let mut session = merchant::Session::new(trade.offers, villager);
        session.xp = trade.xp;
        session.level = level;
        session.profession = format!("minecraft:{profession}");
        session.variant = format!(
            "minecraft:{}",
            VILLAGER_TYPES[m.extra.variant as usize % VILLAGER_TYPES.len()]
        );
        session.random = m.random.clone();
        session.origin = [m.position.x, m.position.y, m.position.z];
        session.ambient_sound_time = m.ambient_timer;
        let (id, name) = (m.id, m.custom_name.clone());
        if villager {
            session.reputation = self.villager_reputation(id);
            session.hero = self.hero_amplifier();
            for seq in merchant::Session::sequences_needed(
                &self.res.stations().trades,
                &session.profession,
                level,
            ) {
                let r = self.rand_sequences.lend(&seq, self.level.seed);
                session.sequences.insert(seq, r);
            }
        }
        // startTrading: updateSpecialPrices, then the screen opens.
        session.update_special_prices();
        // setTradingPlayer: a wandering trader stands still while trading, a
        // villager's brain follows and looks at the player
        // (LookAndFollowTradingPlayerSink).
        self.mobs[index].extra.flag2 = true;
        self.open_merchant_state(MerchantState {
            session: Box::new(session),
            scroll: 0,
            focused: None,
            mob: Some(id),
            name,
        });
        Ok(true)
    }
    /// `Villager.setTradingPlayer(null)`: the session goes back to the mob
    /// (special prices stay as they were), the borrowed sequences to the world.
    pub(super) fn merchant_closed(&mut self, state: &mut MerchantState) {
        let session = &mut state.session;
        for (id, r) in std::mem::take(&mut session.sequences) {
            self.rand_sequences.give_back(id, r);
        }
        let traded = std::mem::take(&mut session.last_traded);
        let Some(id) = state.mob else { return };
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if session.villager {
            m.extra.variant3 = session.level.clamp(1, 5) as u8;
        }
        m.extra.flag2 = false;
        m.random = session.random.clone();
        m.ambient_timer = session.ambient_sound_time;
        let trade = m.extra.trade.get_or_insert_default();
        trade.xp = session.xp;
        trade.offers = session.offers.clone();
        if traded && session.villager {
            self.villager_traded(id);
        }
    }
    fn open_merchant_state(&mut self, state: MerchantState) {
        let mut s = Station::new_kind(Kind::Merchant, [0, 0, 0]);
        s.merchant = Some(Box::new(state));
        self.station = Some(s);
        self.screen = super::Screen::Inventory;
        self.capture(false);
        self.right = false;
    }
    /// The trading mob's random before a menu click (it ticks meanwhile).
    pub(super) fn merchant_sync_in(&self, session: &mut merchant::Session, mob: Option<u32>) {
        if let Some(m) = mob.and_then(|id| self.mobs.iter().find(|m| m.id == id)) {
            session.random = m.random.clone();
            session.ambient_sound_time = m.ambient_timer;
            session.origin = [m.position.x, m.position.y, m.position.z];
        }
    }
    /// After a menu click: the session back to the station, its random to
    /// the mob, and what it did (orbs, level-ups, the yes/no voice).
    pub(super) fn merchant_sync_out(&mut self, session: Box<merchant::Session>) {
        let mob = self
            .station
            .as_ref()
            .and_then(|s| s.merchant.as_ref())
            .and_then(|m| m.mob);
        let (session, traded) = self.merchant_apply(session, mob);
        self.merchant_traded(mob, &traded);
        if let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) {
            m.session = session;
        }
    }
    /// What a menu click's session did, applied to the world and its mob;
    /// with the result of each trade made, for `merchant_traded`.
    pub(super) fn merchant_apply(
        &mut self,
        session: Box<merchant::Session>,
        mob: Option<u32>,
    ) -> (Box<merchant::Session>, Vec<ItemStack>) {
        let mut session = session;
        let mut traded = Vec::new();
        let events = std::mem::take(&mut session.events);
        let (villager, origin) = (session.villager, session.origin);
        if let Some(m) = mob.and_then(|id| self.mobs.iter_mut().find(|m| m.id == id)) {
            m.random = session.random.clone();
            m.ambient_timer = session.ambient_sound_time;
            if villager {
                m.extra.variant3 = session.level.clamp(1, 5) as u8;
            }
        }
        for e in events {
            match e {
                merchant::Event::Orb(v) => {
                    self.add_orb([origin[0], origin[1] + 0.5, origin[2]], v as u32)
                }
                merchant::Event::LevelUp(_) => {
                    if let Some(m) = mob.and_then(|id| self.mobs.iter_mut().find(|m| m.id == id)) {
                        m.add_effect("minecraft:regeneration", 200, 0);
                    }
                }
                merchant::Event::TradeUpdated(yes, pitch) => {
                    let sound = match (villager, yes) {
                        (true, true) => "minecraft:entity.villager.yes",
                        (true, false) => "minecraft:entity.villager.no",
                        (false, true) => "minecraft:entity.wandering_trader.yes",
                        (false, false) => "minecraft:entity.wandering_trader.no",
                    };
                    self.sounds.play(sound, Some(origin), 1.0, pitch);
                }
                merchant::Event::Traded(index) => {
                    traded.extend(session.offers.get(index).map(|o| o.result.clone()));
                }
                merchant::Event::TradeSound => {}
            }
        }
        (session, traded)
    }
    /// `MerchantResultSlot.checkTakeAchievements` and `AbstractVillager.notifyTrade`
    /// for each trade made: the statistic and the `villager_trade` trigger.
    pub(super) fn merchant_traded(&mut self, mob: Option<u32>, results: &[ItemStack]) {
        for result in results {
            self.stats.custom("traded_with_villager", 1);
            let villager = mob
                .and_then(|id| self.mobs.iter().find(|m| m.id == id))
                .map(|m| self.mob_entity(m));
            if let Some(villager) = villager {
                self.trigger_trade(&villager, result);
            }
        }
    }
    /// `Villager.customServerAiStep`: the TRADE reputation event of the last
    /// trade, a tick later; the prices follow the new reputation.
    pub(super) fn merchant_tick(&mut self) {
        let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) else {
            return;
        };
        if !m.session.villager || !m.session.last_traded {
            return;
        }
        m.session.last_traded = false;
        let Some(id) = m.mob else { return };
        self.villager_traded(id);
        let reputation = self.villager_reputation(id);
        self.menu_session(|menu, env, _| {
            let s = menu.merchant.as_mut()?;
            s.reputation = reputation;
            s.update_special_prices();
            menu.merchant_update(env);
            Some(true)
        });
    }
}
