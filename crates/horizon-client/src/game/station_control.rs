//! Control-socket commands for menus and item components, so scripts can
//! drive the workstation screens (`horizon-ctl click-slot 600`, ...).
use super::{Game, Screen};
use horizon_client::{Result, items::ItemStack};
use serde_json::{Value, json};

pub(super) const HELP: &[&str] = &[
    "xp LEVELS                      add (or remove) experience levels",
    "click-slot ID [right] [shift]  click a menu slot (0-35 inventory, 300+ container, 600+ station, 650 result)",
    "click-gui X Y [right] [shift]  click at GUI pixel X Y relative to the menu's top-left corner",
    "book [next|back|take|sign|done] the open book or lectern: its page, or press one of its buttons",
    "type TEXT | type-back          type into the anvil name box / delete one character",
    "enchant ID LEVEL               enchant the held item (books store the enchantment)",
    "potion ITEM POTION             give a potion item (potion, splash_potion, lingering_potion, tipped_arrow)",
    "menu                           JSON dump of the open menu, carried stack and inventory",
    "effect ID SECONDS [AMPLIFIER]  give a status effect (effect clear removes all)",
    "effects                        list active status effects",
    "mount-menu                     open the inventory of the horse, donkey, mule or llama being ridden",
    "close                          close the open menu",
];
fn id(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}
fn stack_json(s: &ItemStack) -> Value {
    json!({
        "item": &*s.item,
        "count": s.count,
        "damage": s.damage,
        "enchantments": s.enchantments().iter().map(|(e, l)| json!([&**e, l])).collect::<Vec<_>>(),
        "stored_enchantments": s.stored_enchantments().iter().map(|(e, l)| json!([&**e, l])).collect::<Vec<_>>(),
        "repair_cost": s.repair_cost(),
        "custom_name": s.custom_name(),
        "potion": s.potion(),
        "trim": s.data().and_then(|d| d.trim.as_ref()).map(|(m, p)| json!([&**m, &**p])),
    })
}

impl Game<'_> {
    fn click_at(&mut self, x: f32, y: f32, right: bool, shift: bool) {
        use winit::keyboard::KeyCode;
        let saved = self.front.cursor;
        self.front.cursor = (x, y);
        let had_shift = self.physical_keys.contains(&KeyCode::ShiftLeft);
        if shift {
            self.physical_keys.insert(KeyCode::ShiftLeft);
        }
        self.click_inventory(right);
        if shift && !had_shift {
            self.physical_keys.remove(&KeyCode::ShiftLeft);
        }
        self.front.cursor = saved;
    }
    pub(super) fn gui_origin(&self) -> Result<(f32, f32)> {
        let size = self
            .front
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
            .ok_or("no renderer yet")?;
        Ok(self.inventory_origin(&horizon_client::hud::HudBatch::new(size)))
    }
    /// Commands not known to control.rs; None when unknown here too.
    pub(super) fn station_command(&mut self, words: &[&str]) -> Result<Option<Value>> {
        let arg = |i: usize| {
            words
                .get(i)
                .copied()
                .ok_or_else(|| format!("{} needs more arguments", words[0]))
        };
        let flag = |name: &str| words.iter().skip(1).any(|w| *w == name);
        let ok = || Ok(Some(json!({ "ok": true })));
        match words.first().copied().unwrap_or("") {
            // The player is the server's: these run its commands there.
            "xp" => {
                let n: i32 = arg(1)?.parse()?;
                self.server_command(&format!("experience add @s {n} levels"))
            }
            "click-slot" => {
                let slot: usize = arg(1)?.parse()?;
                let (ox, oy) = self.gui_origin()?;
                let (sx, sy) = self.slot_position(slot);
                self.click_at(ox + sx + 8.0, oy + sy + 8.0, flag("right"), flag("shift"));
                ok()
            }
            "click-gui" => {
                let (x, y): (f32, f32) = (arg(1)?.parse()?, arg(2)?.parse()?);
                let (ox, oy) = self.gui_origin()?;
                self.click_at(ox + x, oy + y, flag("right"), flag("shift"));
                ok()
            }
            "type" => {
                let text = words[1..].join(" ");
                self.station_text(Some(&text), false);
                ok()
            }
            "type-back" => {
                self.station_text(None, true);
                ok()
            }
            "enchant" => {
                let e = id(arg(1)?);
                let level: u32 = arg(2)?.parse()?;
                if self.sim.res.stations().enchantments.get(&e).is_none() {
                    return Err(format!("unknown enchantment {e}").into());
                }
                self.server_command(&format!("enchant @s {e} {level}"))
            }
            "book" => Ok(Some(self.book_control(words.get(1).copied())?)),
            "potion" => {
                let (item, potion) = (id(arg(1)?), id(arg(2)?));
                self.server_command(&format!(
                    "give @s {item}[potion_contents={{potion:\"{potion}\"}}]"
                ))
            }
            "menu" => {
                let slots: Vec<Value> = self
                    .slots()
                    .into_iter()
                    .filter_map(|i| {
                        let s = if i == 200 {
                            self.craft_result()
                        } else {
                            self.slot_mut(i).clone()
                        };
                        s.map(|s| json!({ "slot": i, "stack": stack_json(&s) }))
                    })
                    .collect();
                Ok(Some(json!({
                    "screen": if self.screen == Screen::Inventory { "inventory" } else { "other" },
                    "station": self.station.as_ref().map(|s| format!("{:?}", s.kind)),
                    "name": self.station.as_ref().map(|s| s.name.clone()),
                    "info": self.station_info(),
                    "carried": self.carried.as_ref().map(stack_json),
                    "level": self.experience.level,
                    "xp_seed": self.level.xp_seed,
                    "slots": slots,
                })))
            }
            "effect" => {
                if arg(1)? == "clear" {
                    return self.server_command("effect clear @s");
                }
                let def = horizon_client::brewing::effect(arg(1)?).ok_or("unknown effect")?;
                let seconds: i32 = arg(2)?.parse()?;
                let amp: i32 = words.get(3).map_or(Ok(0), |a| a.parse())?;
                self.server_command(&format!(
                    "effect give @s minecraft:{} {seconds} {amp}",
                    def.id
                ))
            }
            "effects" => Ok(Some(
                json!({ "effects": self.effects_json(), "health": self.player.as_ref().map(|p| p.health.health()) }),
            )),
            "mount-menu" => {
                // MultiPlayerGameMode.isServerControlledInventory: the inventory
                // key while riding asks the server (OPEN_INVENTORY).
                if !self.riding_mount_with_menu() {
                    return Err("not riding a horse, donkey, mule or llama".into());
                }
                if let Some(r) = self.remote.as_mut() {
                    r.pending_keys.push(winit::keyboard::KeyCode::KeyE);
                }
                ok()
            }
            "close" => {
                self.close_container();
                self.screen = Screen::Playing;
                self.capture(true);
                ok()
            }
            _ => Ok(None),
        }
    }
}
