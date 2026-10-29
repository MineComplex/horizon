//! The horse inventory (`HorseInventoryMenu`, `HorseInventoryScreen`): the
//! saddle and body armour slots (a llama's carpet), the chest of donkeys,
//! mules and llamas and the mount drawn following the cursor. It opens with
//! E while riding a tamed mount or with shift + use on a tamed one. On a
//! server the screen shows the server's menu (`ClientboundMountScreenOpenPacket`)
//! and its clicks go out as container clicks; a server's players get theirs
//! from game/lan_mount_menu.rs.
use super::{Screen};
use crate::hc::{items::ItemStack, mobs::{Mob, Species, equine, llama}};

/// Menu slot ids: the saddle, the body armour and the chest from 710.
pub(super) const SADDLE: usize = 700;
pub(super) const ARMOR: usize = 701;
pub(super) const CHEST: usize = 710;
/// `AbstractChestedHorse.INV_CHEST_COUNT` and `getInventoryColumns`.
const CHEST_SIZE: usize = 15;
const COLUMNS: usize = 5;

/// `AbstractHorse.getInventoryColumns`: five for a donkey's or mule's chest,
/// a llama's strength for its chest, else none.
pub(super) fn mount_columns(m: &Mob) -> usize {
    let species = m.species();
    if llama::is_llama(species) {
        llama::inventory_columns(m).clamp(0, 5) as usize
    } else if m.extra.chested && matches!(species, Species::Donkey | Species::Mule) {
        COLUMNS
    } else {
        0
    }
}
/// Whether the mount has a horse inventory (`AbstractHorse`).
pub(super) fn has_mount_menu(species: Species) -> bool {
    equine::is_equine(species) || llama::is_llama(species)
}
/// The saddle slot is active (`canUseSlot(SADDLE)` and `CAN_EQUIP_SADDLE`):
/// not on llamas.
fn shows_saddle(species: Species) -> bool {
    !llama::is_llama(species)
}
/// The body slot is active: horse armour, or a llama's carpet.
fn shows_body(species: Species) -> bool {
    can_wear_horse_armor(species) || llama::is_llama(species)
}

/// The open mount menu: copies of the mount's slots, written back on every change.
#[derive(Default)]
pub(super) struct MountMenu {
    open: Option<Open>,
}
impl MountMenu {
    /// The mount whose menu is open.
    pub(super) fn opened_mob(&self) -> Option<u32> {
        self.open.as_ref().map(|o| o.mob)
    }
}
struct Open {
    mob: u32,
    species: Species,
    columns: usize,
    saddle: Option<ItemStack>,
    armor: Option<ItemStack>,
    items: Vec<Option<ItemStack>>,
    /// `shouldRenderSaddleSlot` / `shouldRenderArmorSlot`.
    show_saddle: bool,
    show_armor: bool,
}

/// `EntityTypeTags.CAN_WEAR_HORSE_ARMOR`.
fn can_wear_horse_armor(species: Species) -> bool {
    matches!(species, Species::Horse | Species::ZombieHorse)
}

impl crate::game::Sim<'_> {
    /// Whether the mount menu is the open screen.
    pub(super) fn mount_active(&self) -> bool {
        self.screen == Screen::Inventory && self.mount_menu.open.is_some()
    }
    fn mount_mob_index(&self, id: u32) -> Option<usize> {
        self.mobs.iter().position(|m| m.id == id)
    }
    /// E while riding a tamed mount opens its inventory instead of the player's.
    pub(super) fn open_ridden_mount_menu(&mut self) -> bool {
        let Some(id) = self.riding else {
            return false;
        };
        let Some(index) = self.mount_mob_index(id) else {
            return false;
        };
        let m = &self.mobs[index];
        if !has_mount_menu(m.species()) || !m.extra.tame || !m.alive() {
            return false;
        }
        self.open_mount_menu(index);
        true
    }
    /// The entity type of the open mount menu's mount (`minecraft:horse`).
    pub(super) fn mount_menu_entity(&self) -> Option<String> {
        let open = self.mount_menu.open.as_ref()?;
        let m = self.mobs.iter().find(|m| m.id == open.mob)?;
        Some(m.kind.id.to_string())
    }
    /// The chest columns of the open mount menu (the server's window's).
    pub(super) fn mount_menu_columns(&self) -> Option<usize> {
        self.mount_menu.open.as_ref().map(|o| o.columns)
    }
    /// The menu ends when the mount dies or is more than 4 blocks away
    /// (`AbstractMountInventoryMenu.stillValid`).
    pub(super) fn tick_mount_menu(&mut self) {
        // A server's menu ends when the server closes it.
        
        let Some(open) = self.mount_menu.open.as_ref() else {
            return;
        };
        let valid = self.mount_mob_index(open.mob).is_some_and(|i| {
            let m = &self.mobs[i];
            let c = m.center();
            m.alive()
                && self.eye_position().is_some_and(|e| {
                    let d = (e[0] - c[0]).powi(2) + (e[1] - c[1]).powi(2) + (e[2] - c[2]).powi(2);
                    d <= 16.0 + 4.0
                })
        });
        if !valid {
            self.close_container();
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }
    /// Closing the screen keeps what is in the slots on the mount.
    pub(super) fn mount_closed(&mut self) {
        self.mount_sync();
        self.mount_menu.open = None;
    }
    /// Writes the menu slots to the mount, with the equip sounds. A
    /// server's menu is the server's to write.
    fn mount_sync(&mut self) {
        
        let Some(open) = self.mount_menu.open.as_ref() else {
            return;
        };
        let Some(index) = self.mount_mob_index(open.mob) else {
            return;
        };
        let (saddle, armor) = (open.saddle.clone(), open.armor.clone());
        let body = open.show_armor.then_some(armor);
        let items = open.items.clone();
        self.mount_store(index, saddle, body, items);
    }
    /// The slots of mount `index` as its menu shows them: the saddle, the
    /// body item (None when the slot isn't active), the chest.
    pub(super) fn mount_contents(
        &self,
        index: usize,
    ) -> (
        Option<ItemStack>,
        Option<Option<ItemStack>>,
        Vec<Option<ItemStack>>,
    ) {
        let m = &self.mobs[index];
        let species = m.species();
        let body =
            shows_body(species).then(|| m.extra.head.as_deref().map(|i| ItemStack::new(i, 1)));
        let mut items = m.extra.items.clone();
        items.resize(mount_columns(m) * 3, None);
        (m.saddle_item(), body, items)
    }
    /// The menu's slots back on mount `index` (`body` None: not its slot),
    /// with the equip sound of what was put on (`Equippable.equipSound`).
    pub(super) fn mount_store(
        &mut self,
        index: usize,
        saddle: Option<ItemStack>,
        body: Option<Option<ItemStack>>,
        items: Vec<Option<ItemStack>>,
    ) {
        let m = &self.mobs[index];
        let at = m.center();
        let put_on = if saddle.is_some() && !m.extra.saddled {
            saddle.as_ref()
        } else {
            body.as_ref()
                .and_then(Option::as_ref)
                .filter(|_| m.extra.head.is_none())
        };
        let sound = put_on.map(|s| {
            self.res
                .items()
                .get(&s.item)
                .and_then(|i| i.equip_sound.clone())
                .unwrap_or_else(|| "minecraft:item.armor.equip_generic".into())
        });
        let m = &mut self.mobs[index];
        m.set_saddle(saddle);
        if let Some(body) = body {
            m.extra.head = body.map(|a| a.item);
        }
        let mut items = items;
        let keep = m.extra.items.len().max(CHEST_SIZE);
        items.resize(keep, None);
        m.extra.items = items;
        let (x, z) = (at[0].floor() as i32 >> 4, at[2].floor() as i32 >> 4);
        self.world.modified.insert((x, z));
        if let Some(sound) = sound {
            self.sounds.play(&sound, Some(at), 0.5, 1.0);
        }
    }
    pub(super) fn mount_slot_mut(&mut self, i: usize) -> &mut Option<ItemStack> {
        match self.mount_menu.open.as_mut() {
            Some(open) if i == SADDLE => &mut open.saddle,
            Some(open) if i == ARMOR => &mut open.armor,
            Some(open) if i >= CHEST && i - CHEST < CHEST_SIZE => &mut open.items[i - CHEST],
            _ => &mut self.scratch_slot,
        }
    }
}

impl crate::game::Sim<'_> {
    /// `AbstractHorse.openCustomInventoryScreen` for a tamed adult mount: the
    /// turn's player has it open (the server then opens it on their client,
    /// game/lan_mount_menu.rs).
    pub(super) fn open_mount_menu(&mut self, index: usize) {
        let m = &self.mobs[index];
        let species = m.species();
        if !has_mount_menu(species) {
            return;
        }
        let mut items = m.extra.items.clone();
        items.resize(CHEST_SIZE, None);
        let show_armor = shows_body(species);
        self.mount_menu.open = Some(Open {
            mob: m.id,
            species,
            columns: mount_columns(m),
            saddle: m.saddle_item(),
            armor: m
                .extra
                .head
                .as_deref()
                .filter(|_| show_armor)
                .map(|i| ItemStack::new(i, 1)),
            items,
            show_saddle: shows_saddle(species),
            show_armor,
        });
    }
}
