//! Workstation menus in the game: enchanting table, anvil, grindstone,
//! smithing table, stonecutter and the brewing stand screen. Layouts, sprites,
//! colours and sounds follow the vanilla screens (`EnchantmentScreen`,
//! `AnvilScreen`, `GrindstoneScreen`, `SmithingScreen`, `StonecutterScreen`,
//! `BrewingStandScreen`); item rules live in `crate::hc::stations`.
use super::{Game, Screen};
use crate::hc::{block_entity_item, brewing, containers::BlockEntity, enchant::Enchantments, items::ItemStack, stations::{self, AnvilResult, EnchantOffers}, text::Text};
use horizon_core::random::{LegacyRandom, RandomSource};

/// Slot ids of the open station: inputs from 600, the result at 650.
pub(super) const INPUT: usize = 600;
pub(super) const RESULT: usize = 650;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Enchanting,
    Anvil,
    Grindstone,
    Smithing,
    Stonecutter,
    /// Villager trading (game/trading.rs).
    Merchant,
    /// The beacon menu (game/beacon.rs).
    Beacon,
    /// The cartography table (game/cartography.rs).
    Cartography,
    /// The loom (game/loom.rs).
    Loom,
}
impl Kind {
    pub(super) fn from_block(name: &str) -> Option<Self> {
        Some(match name {
            "minecraft:enchanting_table" => Self::Enchanting,
            "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil" => {
                Self::Anvil
            }
            "minecraft:grindstone" => Self::Grindstone,
            "minecraft:smithing_table" => Self::Smithing,
            "minecraft:stonecutter" => Self::Stonecutter,
            "minecraft:beacon" => Self::Beacon,
            "minecraft:cartography_table" => Self::Cartography,
            "minecraft:loom" => Self::Loom,
            _ => return None,
        })
    }
    pub(super) fn inputs(self) -> usize {
        match self {
            Self::Smithing | Self::Loom => 3,
            Self::Stonecutter | Self::Beacon => 1,
            _ => 2,
        }
    }
    fn texture(self) -> &'static str {
        match self {
            Self::Enchanting => "gui/container/enchanting_table",
            Self::Anvil => "gui/container/anvil",
            Self::Grindstone => "gui/container/grindstone",
            Self::Smithing => "gui/container/smithing",
            Self::Stonecutter => "gui/container/stonecutter",
            Self::Merchant => "gui/container/villager",
            Self::Beacon => "gui/container/beacon",
            Self::Cartography => "gui/container/cartography_table",
            Self::Loom => "gui/container/loom",
        }
    }
    pub(super) fn title_key(self) -> &'static str {
        match self {
            Self::Enchanting => "container.enchant",
            Self::Anvil => "container.repair",
            Self::Grindstone => "container.grindstone_title",
            Self::Smithing => "container.upgrade",
            Self::Stonecutter => "container.stonecutter",
            Self::Merchant => "merchant.trades",
            Self::Beacon => "container.beacon",
            Self::Cartography => "container.cartography_table",
            Self::Loom => "container.loom",
        }
    }
    /// (titleLabelX, titleLabelY).
    fn title_pos(self) -> (f32, f32) {
        match self {
            Self::Anvil => (60.0, 6.0),
            Self::Smithing => (44.0, 15.0),
            Self::Stonecutter => (8.0, 5.0),
            Self::Cartography | Self::Loom => (8.0, 4.0),
            _ => (8.0, 6.0),
        }
    }
    fn input_pos(self, i: usize) -> (f32, f32) {
        let table: &[(f32, f32)] = match self {
            Self::Enchanting => &[(15.0, 47.0), (35.0, 47.0)],
            Self::Anvil => &[(27.0, 47.0), (76.0, 47.0)],
            Self::Grindstone => &[(49.0, 19.0), (49.0, 40.0)],
            Self::Smithing => &[(8.0, 48.0), (26.0, 48.0), (44.0, 48.0)],
            Self::Stonecutter => &[(20.0, 33.0)],
            Self::Merchant => &[(86.0, 37.0), (112.0, 37.0)],
            Self::Beacon => &[(109.0, 110.0)],
            Self::Cartography => &[(15.0, 15.0), (15.0, 52.0)],
            Self::Loom => &[(13.0, 26.0), (33.0, 26.0), (23.0, 45.0)],
        };
        table[i.min(table.len() - 1)]
    }
}

pub(super) struct Station {
    pub(super) kind: Kind,
    pub(super) pos: [i32; 3],
    pub(super) inputs: Vec<Option<ItemStack>>,
    pub(super) result: Option<ItemStack>,
    pub(super) offers: EnchantOffers,
    bookcases: i32,
    pub(super) anvil: AnvilResult,
    /// Anvil rename box text (`AnvilMenu.itemName`).
    pub(super) name: String,
    /// The box itself (`AnvilScreen`'s `EditBox` of 50, unbordered).
    pub(super) rename: crate::hc::ui::EditBox,
    /// Stonecutter: the menu's recipe list (crate::hc::menu), its
    /// `selectedRecipeIndex` and the screen's first shown index.
    pub(super) stone: crate::hc::menu::Stonecutter,
    pub(super) selected: Option<usize>,
    start: usize,
    /// Anvil: the input the rename box was last filled from.
    anvil_last: Option<ItemStack>,
    /// Anvil on a server: the name last sent (the client menu's `itemName`).
    pub(super) anvil_sent: Option<String>,
    pub(super) last_sound: u64,
    /// Villager trading state.
    pub(super) merchant: Option<Box<super::trading::MerchantState>>,
    /// Beacon menu: the selected primary and secondary power (EFFECTS index).
    pub(super) beacon: [Option<usize>; 2],
    /// A server's beacon menu: the pyramid's levels (its data slot 0).
    pub(super) beacon_levels: Option<i32>,
    /// The loom's patterns, selection and scroller (game/loom.rs).
    pub(super) loom: super::loom::LoomState,
}
impl Station {
    fn new(kind: Kind, pos: [i32; 3]) -> Self {
        Self {
            kind,
            pos,
            inputs: vec![None; kind.inputs()],
            result: None,
            offers: EnchantOffers::default(),
            bookcases: 0,
            anvil: AnvilResult::default(),
            name: String::new(),
            rename: anvil_box(),
            stone: Default::default(),
            selected: None,
            start: 0,
            anvil_last: None,
            anvil_sent: None,
            last_sound: 0,
            merchant: None,
            beacon: [None; 2],
            beacon_levels: None,
            loom: Default::default(),
        }
    }
    pub(super) fn new_kind(kind: Kind, pos: [i32; 3]) -> Self {
        Self::new(kind, pos)
    }
    /// `StonecutterScreen.containerChanged`: without recipes to show, the
    /// list scrolls back to the top.
    pub(super) fn stonecutter_changed(&mut self) {
        if self.inputs[0].is_none() || self.stone.recipes.is_empty() {
            self.start = 0;
        }
    }
}

/// The 108x19 enchantment button `i` (relative to the GUI origin).
fn enchant_button(i: usize) -> (f32, f32, f32, f32) {
    (60.0, 14.0 + 19.0 * i as f32, 108.0, 19.0)
}
fn inside(p: (f32, f32), x: f32, y: f32, w: f32, h: f32) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}
/// `EnchantmentNames` word list.
const WORDS: &[&str] = &[
    "the",
    "elder",
    "scrolls",
    "klaatu",
    "berata",
    "niktu",
    "xyzzy",
    "bless",
    "curse",
    "light",
    "darkness",
    "fire",
    "air",
    "earth",
    "water",
    "hot",
    "dry",
    "cold",
    "wet",
    "ignite",
    "snuff",
    "embiggen",
    "twist",
    "shorten",
    "stretch",
    "fiddle",
    "destroy",
    "imbue",
    "galvanize",
    "enchant",
    "free",
    "limited",
    "range",
    "of",
    "towards",
    "inside",
    "sphere",
    "cube",
    "self",
    "other",
    "ball",
    "mental",
    "physical",
    "grow",
    "shrink",
    "demon",
    "elemental",
    "spirit",
    "animal",
    "creature",
    "beast",
    "humanoid",
    "undead",
    "fresh",
    "stale",
    "phnglui",
    "mglwnafh",
    "cthulhu",
    "rlyeh",
    "wgahnagl",
    "fhtagn",
    "baguette",
];
const GREY: u32 = 0xFF40_4040;

/// A text style's `0xRRGGBB` colour from a HUD colour (`rgba`).
fn rgb_of(rgba: u32) -> u32 {
    (rgba & 0xFF) << 16 | (rgba >> 8 & 0xFF) << 8 | (rgba >> 16 & 0xFF)
}
impl crate::game::Sim<'_> {
    /// `Component.translatable`: the selected language (which `lang` layers
    /// over `en_us`), the caller's English text last.
    pub(super) fn translate(&self, key: &str, fallback: &str) -> String {
        crate::hc::lang::get_or(key, fallback)
    }
    fn brewing_open(&self) -> bool {
        matches!(self.container_kind(), Some(BlockEntity::Brewing(_)))
    }
    /// Opens a workstation menu for the block; false when it is not one.
    pub(super) fn open_station(&mut self, pos: [i32; 3], name: &str) -> bool {
        let Some(kind) = Kind::from_block(name) else {
            return false;
        };
        if self.level.xp_seed == 0 {
            self.level.xp_seed = self.world.random.next_int();
        }
        self.station = Some(Station::new(kind, pos));
        self.station_changed();
        self.beacon_opened();
        // The block's interaction statistic.
        let stat = match name {
            "minecraft:crafting_table" => "interact_with_crafting_table",
            "minecraft:furnace" => "interact_with_furnace",
            "minecraft:blast_furnace" => "interact_with_blast_furnace",
            "minecraft:smoker" => "interact_with_smoker",
            "minecraft:brewing_stand" => "interact_with_brewingstand",
            "minecraft:beacon" => "interact_with_beacon",
            "minecraft:grindstone" => "interact_with_grindstone",
            "minecraft:smithing_table" => "interact_with_smithing_table",
            "minecraft:stonecutter" => "interact_with_stonecutter",
            "minecraft:loom" => "interact_with_loom",
            "minecraft:cartography_table" => "interact_with_cartography_table",
            n if n.ends_with("anvil") => "interact_with_anvil",
            _ => "",
        };
        if !stat.is_empty() {
            self.stats.custom(stat, 1);
        }
        true
    }
    /// `AbstractContainerMenu.clearContainer`: inputs go back to the player.
    pub(super) fn close_station(&mut self) {
        if self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == Kind::Merchant)
        {
            // MerchantMenu.removed: the carried stack and the payment go back.
            self.menu_session(|menu, env, _| {
                menu.merchant_close(env);
                Some(true)
            });
        }
        let Some(mut station) = self.station.take() else {
            return;
        };
        if false && station.kind != Kind::Merchant {
            // A server's workstation: its removed() gives the inputs back.
            return;
        }
        if let Some(m) = station.merchant.as_mut() {
            self.merchant_closed(m);
        }
        for stack in station.inputs.into_iter().flatten() {
            self.give_or_drop(stack);
        }
    }
    pub(super) fn station_slot_mut(&mut self, i: usize) -> &mut Option<ItemStack> {
        match self.station.as_mut() {
            Some(s) if i == RESULT => &mut s.result,
            Some(s) if i >= INPUT && i < INPUT + s.inputs.len() => &mut s.inputs[i - INPUT],
            _ => &mut self.scratch_slot,
        }
    }
    pub(super) fn count_bookshelves(&self, pos: [i32; 3]) -> i32 {
        let tags = &self.res.items().block_tags;
        let is = |p: [i32; 3], tag: &str| {
            let name = &self
                .res
                .facts()
                .get(self.world.block(p[0], p[1], p[2]))
                .name;
            tags.get(tag).is_some_and(|t| t.contains(&**name))
        };
        stations::bookshelf_offsets()
            .into_iter()
            .filter(|o| {
                is(
                    [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]],
                    "minecraft:enchantment_power_provider",
                ) && is(
                    [pos[0] + o[0] / 2, pos[1] + o[1], pos[2] + o[2] / 2],
                    "minecraft:enchantment_power_transmitter",
                )
            })
            .count() as i32
    }
    /// The anvil name the menu works with (`AnvilScreen.onNameChanged`).
    pub(super) fn anvil_item_name(&self, s: &Station) -> String {
        match &s.inputs[0] {
            Some(input)
                if input.custom_name().is_none()
                    && s.name == self.pack.names.display_item(&input.item) =>
            {
                String::new()
            }
            Some(_) => s.name.clone(),
            None => String::new(),
        }
    }
    /// `slotsChanged` / `createResult` for the open station.
    pub(super) fn station_changed(&mut self) {
        let Some(mut s) = self.station.take() else {
            return;
        };
        let creative = self.creative;
        let data = self.res.stations();
        let items = self.res.items();
        match s.kind {
            // A server's table: its data slots carry the offers, and its
            // client menu only clears them without an enchantable item.
            Kind::Enchanting if false => {
                let enchantable = s.inputs[0].as_ref().is_some_and(|i| {
                    data.enchantments.enchantable(&i.item) > 0 && i.enchantments().is_empty()
                });
                if !enchantable {
                    s.offers = Default::default();
                }
            }
            Kind::Enchanting => {
                s.bookcases = self.count_bookshelves(s.pos);
                s.offers = stations::enchant_offers(
                    &data.enchantments,
                    items,
                    self.level.xp_seed,
                    s.bookcases,
                    s.inputs[0].as_ref(),
                );
            }
            Kind::Anvil => {
                let name = self.anvil_item_name(&s);
                let hover = s.inputs[0]
                    .as_ref()
                    .map(|i| stations::hover_name(&|id| self.pack.names.display_item(id), i))
                    .unwrap_or_default();
                s.anvil = stations::anvil_result(
                    &data.enchantments,
                    items,
                    s.inputs[0].as_ref(),
                    s.inputs[1].as_ref(),
                    Some(&name),
                    &hover,
                    creative,
                );
                s.result = s.anvil.result.clone();
            }
            Kind::Grindstone => {
                s.result = stations::grindstone_result(
                    &data.enchantments,
                    items,
                    s.inputs[0].as_ref(),
                    s.inputs[1].as_ref(),
                );
            }
            Kind::Smithing => {
                s.result = data.recipes.smithing_result(
                    &data.enchantments,
                    s.inputs[0].as_ref(),
                    s.inputs[1].as_ref(),
                    s.inputs[2].as_ref(),
                );
            }
            // The menu engine keeps the merchant's result (crate::game::Sim::menu_session).
            Kind::Merchant => {}
            Kind::Beacon => {}
            // The menu engine keeps the cartography table's, the loom's and
            // the stonecutter's state (crate::game::Sim::menu_session).
            Kind::Cartography | Kind::Loom | Kind::Stonecutter => {}
        }
        self.station = Some(s);
    }
    pub(super) fn max_stack(&self, stack: &ItemStack) -> u32 {
        self.res
            .items()
            .get(&stack.item)
            .map_or(64, |i| i.max_stack)
    }
    /// `Slot.mayPlace` and `getMaxStackSize` for station slots.
    fn slot_rules(&self, slot: usize, stack: &ItemStack) -> (bool, u32) {
        if slot >= super::mount_menu::SADDLE {
            return self.mount_slot_rules(slot, stack);
        }
        if let Some(BlockEntity::Crafter(c)) = self.container_kind()
            && (300..309).contains(&slot)
        {
            // CrafterSlot.mayPlace: not a disabled slot.
            return (!c.disabled[slot - 300], 64);
        }
        if (300..400).contains(&slot) && !self.brewing_open() && self.refuses(slot, stack) {
            // ShulkerBoxSlot.mayPlace.
            return (false, 64);
        }
        let Some(s) = self.station.as_ref() else {
            return (true, 64);
        };
        if slot < 36 {
            return (true, 64);
        }
        if slot == RESULT {
            return (false, 64);
        }
        (s.kind != Kind::Loom, 64)
    }
    /// `AbstractContainerMenu.doClick` PICKUP on a filtered slot.
    pub(super) fn click_filtered(&mut self, slot: usize, right: bool) {
        let carried = self.carried.take();
        let current = self.slot_mut(slot).take();
        let (new_slot, new_carried) = match (carried, current) {
            (None, None) => (None, None),
            (None, Some(mut s)) => {
                let take = if right { s.count.div_ceil(2) } else { s.count };
                s.count -= take;
                let taken = ItemStack {
                    count: take,
                    ..s.clone()
                };
                ((s.count > 0).then_some(s), Some(taken))
            }
            (Some(mut c), None) => {
                let (ok, cap) = self.slot_rules(slot, &c);
                if ok {
                    let n = if right { 1 } else { c.count }.min(cap.min(self.max_stack(&c)));
                    let placed = ItemStack {
                        count: n,
                        ..c.clone()
                    };
                    c.count -= n;
                    (Some(placed), (c.count > 0).then_some(c))
                } else {
                    (None, Some(c))
                }
            }
            (Some(mut c), Some(mut s)) => {
                let (ok, cap) = self.slot_rules(slot, &c);
                if ok && c.same_item_same_components(&s) {
                    let room = cap.min(self.max_stack(&s)).saturating_sub(s.count);
                    let n = if right { 1 } else { c.count }.min(room);
                    s.count += n;
                    c.count -= n;
                    (Some(s), (c.count > 0).then_some(c))
                } else if ok && c.count <= cap.min(self.max_stack(&c)) {
                    (Some(c), Some(s))
                } else if c.same_item_same_components(&s) {
                    let n = s.count.min(self.max_stack(&c).saturating_sub(c.count));
                    s.count -= n;
                    c.count += n;
                    ((s.count > 0).then_some(s), Some(c))
                } else {
                    (Some(s), Some(c))
                }
            }
        };
        *self.slot_mut(slot) = new_slot;
        self.carried = new_carried;
    }
    /// `moveItemStackTo` over `targets` (menu order), merging first, then empty slots.
    pub(super) fn move_to(
        &mut self,
        stack: &mut ItemStack,
        targets: &[usize],
        reverse: bool,
    ) -> bool {
        let order: Vec<usize> = if reverse {
            targets.iter().rev().copied().collect()
        } else {
            targets.to_vec()
        };
        let start = stack.count;
        if self.max_stack(stack) > 1 {
            for &t in &order {
                if stack.count == 0 {
                    break;
                }
                let (_, cap) = self.slot_rules(t, stack);
                let max = cap.min(self.max_stack(stack));
                if let Some(s) = self.slot_mut(t).as_mut()
                    && s.same_item_same_components(stack)
                {
                    let n = max.saturating_sub(s.count).min(stack.count);
                    s.count += n;
                    stack.count -= n;
                }
            }
        }
        for &t in &order {
            if stack.count == 0 {
                break;
            }
            let (ok, cap) = self.slot_rules(t, stack);
            if self.slot_mut(t).is_none() && (t < 36 || ok) {
                let n = cap.min(self.max_stack(stack)).min(stack.count);
                *self.slot_mut(t) = Some(ItemStack {
                    count: n,
                    ..stack.clone()
                });
                stack.count -= n;
            }
        }
        stack.count != start
    }
    /// After an anvil or smithing click on the menu engine: the anvil's cost
    /// and state back into the station and the rename box following the
    /// input (`AnvilScreen.slotChanged`), then the rest of a taken result's
    /// `onTake` (the levels, the anvil's wear, the use sound) unless a
    /// server does it.
    pub(super) fn combiner_session_end(
        &mut self,
        menu: &crate::hc::menu::Menu,
        predicting: bool,
    ) {
        let Some(s) = self.station.as_mut() else {
            return;
        };
        let (pos, kind) = (s.pos, s.kind);
        if kind == Kind::Anvil {
            s.anvil.cost = menu.data[0];
            s.anvil.repair_item_count_cost = menu.anvil.repair_count;
            s.anvil.only_renaming = menu.anvil.only_renaming;
            s.anvil.result = s.result.clone();
        }
        self.station_after_input();
        if !menu.combined || predicting {
            return;
        }
        if kind == Kind::Smithing {
            self.level_event_at(1044, pos, 0);
            return;
        }
        self.give_levels(menu.level_change);
        if self.anvil_used(pos, self.creative) {
            self.close_station();
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }
    /// `AnvilMenu.onTake`'s level part: out of creative a 12% chance
    /// (`AnvilBlock.damage`) to wear the anvil a stage, or break a damaged one
    /// (level event 1029); otherwise level event 1030. True when it broke.
    pub(super) fn anvil_used(&mut self, pos: [i32; 3], creative: bool) -> bool {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let name = self.res.facts().get(state).name.clone();
        let anvil = matches!(
            &*name,
            "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil"
        );
        if !creative && anvil && self.world.random.next_float() < 0.12 {
            match &*name {
                "minecraft:anvil" => {
                    let _ = self.set_state_named(pos, "minecraft:chipped_anvil", &["facing"]);
                }
                "minecraft:chipped_anvil" => {
                    let _ = self.set_state_named(pos, "minecraft:damaged_anvil", &["facing"]);
                }
                _ => {
                    let _ = self.world.set(pos[0], pos[1], pos[2], self.world.air);
                    self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
                    self.level_event_at(1029, pos, 0);
                    return true;
                }
            }
        }
        self.level_event_at(1030, pos, 0);
        false
    }
    /// The grindstone result's `onTake` with a level (`amount`: the menu's
    /// `getExperienceAmount` sum): the orbs at the block's centre
    /// (`ExperienceOrb.award`), then level event 1042, the grindstone's sound
    /// for everyone around.
    pub(super) fn grindstone_taken(&mut self, pos: [i32; 3], amount: i32) {
        let xp = stations::grindstone_xp_roll(amount, &mut self.world.random);
        if xp > 0 {
            self.spawn_orbs(pos.map(|v| f64::from(v) + 0.5), xp);
        }
        self.level_event_at(1042, pos, 0);
    }
    fn play_at(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32) {
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.sounds.play(sound, Some(at), volume, pitch);
    }
    pub(super) fn give_or_drop(&mut self, stack: ItemStack) {
        if let Some(rest) = self.give(stack)
            && let Some(eye) = self.eye_position()
        {
            // Inventory.placeItemBackInInventory: `player.drop(stack, false)`.
            self.throw_from_eyes(rest, eye, false);
        }
    }
    /// `Player.giveExperienceLevels`.
    pub(super) fn give_levels(&mut self, levels: i32) {
        let xp = &mut self.experience;
        xp.level = xp.level.saturating_add(levels);
        if xp.level < 0 {
            xp.level = 0;
            xp.progress = 0.0;
            xp.total = 0;
        }
    }
    /// Whether the result slot may be taken (`mayPickup`).
    fn may_take_result(&self) -> bool {
        let Some(s) = self.station.as_ref() else {
            return false;
        };
        match s.kind {
            Kind::Anvil => {
                (self.creative || self.experience.level >= s.anvil.cost) && s.anvil.cost > 0
            }
            _ => true,
        }
    }
    /// Recompute after slot changes; the anvil box follows its input item.
    fn station_after_input(&mut self) {
        let Some(s) = self.station.as_mut() else {
            return;
        };
        if s.kind == Kind::Anvil && s.inputs[0] != s.anvil_last {
            // AnvilScreen.slotChanged: the box shows the new input's name.
            s.anvil_last = s.inputs[0].clone();
            s.name = s.inputs[0]
                .as_ref()
                .map(|i| stations::hover_name(&|id| self.pack.names.display_item(id), i))
                .unwrap_or_default();
            let name = s.name.clone();
            s.rename.set_value(&name);
        }
        self.station_changed();
    }
    /// Whether keyboard text goes to the anvil rename box.
    pub(super) fn station_typing(&self) -> bool {
        self.screen == Screen::Inventory
            && self
                .station
                .as_ref()
                .is_some_and(|s| s.kind == Kind::Anvil && s.inputs[0].is_some())
    }
    /// `ItemStack.getHoverName`: the custom name, a written book's title, the
    /// item name component, else the item's or potion's name.
    fn stack_hover_name(&self, stack: &ItemStack) -> String {
        if let Some(n) = stack.custom_name() {
            n.to_owned()
        } else if let Some(title) = stack
            .data()
            .and_then(|d| d.written.as_ref())
            .map(|w| w.title.as_str())
            .filter(|t| !t.trim().is_empty())
        {
            title.to_owned()
        } else if let Some(n) = stack.item_name() {
            n
        } else if stack.data().is_some_and(|d| d.ominous_banner) {
            self.translate("block.minecraft.ominous_banner", "Ominous Banner")
        } else if stack.data().and_then(|d| d.potion.as_ref()).is_some()
            || stack.item.ends_with("potion")
            || &*stack.item == "minecraft:tipped_arrow"
        {
            let key = brewing::potion_name_key(&stack.item, stack.potion());
            self.pack
                .names
                .display_get(&key)
                .unwrap_or_else(|| self.pack.names.display_item(&stack.item))
        } else {
            self.pack.names.display_item(&stack.item)
        }
    }
    /// Menu state for the control socket.
    pub(super) fn station_info(&self) -> serde_json::Value {
        let Some(s) = self.station.as_ref() else {
            return match self.container_kind() {
                Some(BlockEntity::Brewing(b)) => {
                    serde_json::json!({ "brew_time": b.brew_time, "fuel": b.fuel })
                }
                _ => serde_json::Value::Null,
            };
        };
        let recipes = &self.res.stations().recipes.stonecutting;
        serde_json::json!({
            "costs": s.offers.costs,
            "clues": s.offers.clues.iter().map(|c| c.as_ref().map(|(e, l)| format!("{e} {l}"))).collect::<Vec<_>>(),
            "bookcases": s.bookcases,
            "anvil_cost": s.anvil.cost,
            "result": s.result.as_ref().map(|r| format!("{} x{}", r.item, r.count)),
            "recipes": s.stone.recipes.iter().map(|&i| recipes[i].id.clone()).collect::<Vec<_>>(),
            "selected": s.selected,
        })
    }
    /// Options.advancedItemTooltips (F3+H).
    fn options_advanced_tooltips(&self) -> bool {
        self.settings.file.bool("advancedItemTooltips")
    }
    /// `BrewingStandBlockEntity.serverTick` of the brewing stand at `pos`, in
    /// its place among the level's block entity tickers.
    pub(super) fn tick_brewing_stand(
        &mut self,
        pos: (i32, i32, i32),
    ) -> crate::hc::Result<()> {
        let Some(BlockEntity::Brewing(b)) = self.world.block_entities.get_mut(&pos) else {
            return Ok(());
        };
        let out = b.tick(
            &self.res.stations().brewing,
            self.res.items(),
            self.res.providers(),
        );
        if out != brewing::BrewTick::default() {
            let (x, y, z) = pos;
            self.world.modified.insert((x >> 4, z >> 4));
            if out.brewed {
                self.play_at("minecraft:block.brewing_stand.brew", [x, y, z], 1.0, 1.0);
            }
            for d in out.drops {
                self.pop_resource([x, y, z], d);
            }
            if let Some(bottles) = out.bottles {
                let state = self.world.block(x, y, z);
                if let Some(mut s) = self.pack.assets.registry.state(state).cloned()
                    && AsRef::<str>::as_ref(&s.name) == "minecraft:brewing_stand"
                {
                    for (i, has) in bottles.iter().enumerate() {
                        s.properties
                            .insert(format!("has_bottle_{i}"), has.to_string());
                    }
                    if let Ok(id) = self.pack.assets.registry.resolve(&s) {
                        self.world.set(x, y, z, id)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The anvil's rename box (`AnvilScreen`: `setMaxLength(50)`).
const ANVIL_NAME_CHARS: usize = 50;

