//! The creative inventory's rules without its screen: item names and clicks on a tab's
//! display slots (`CreativeModeInventoryScreen.slotClicked`).
use crate::items::ItemStack;
use std::{collections::HashMap, fs, path::Path};

/// Display names from the pinned en_us language file.
#[derive(Default)]
pub struct Names {
    pub map: HashMap<String, String>,
}
impl Names {
    pub fn load(root: &Path) -> Self {
        let read = |namespace: &str| {
            fs::read(root.join(format!("assets/{namespace}/lang/en_us.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<HashMap<String, String>>(&b).ok())
                .unwrap_or_default()
        };
        Self {
            map: read("minecraft"),
        }
    }
    pub fn item(&self, id: &str) -> String {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        self.map
            .get(&format!("item.{namespace}.{path}"))
            .or_else(|| self.map.get(&format!("block.{namespace}.{path}")))
            .cloned()
            .unwrap_or_else(|| path.replace('_', " "))
    }
    /// An item's display name in the selected language: `item.*` then
    /// `block.*`, then the pinned English map (`Names::item`).
    pub fn display_item(&self, id: &str) -> String {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        if let Some(lang) = crate::lang::current() {
            for prefix in ["item", "block"] {
                if let Some(name) = lang.get(&format!("{prefix}.{namespace}.{path}")) {
                    return name.to_owned();
                }
            }
        }
        self.item(id)
    }
    /// A translation key's text in the selected language, else the pinned
    /// English map (`HoverName` and tooltip keys such as potion names).
    pub fn display_get(&self, key: &str) -> Option<String> {
        crate::lang::current()
            .and_then(|lang| lang.get(key).map(str::to_owned))
            .or_else(|| self.get(key).map(str::to_owned))
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }
}

/// What a click on an item tab's display slot does besides changing the
/// carried stack.
#[derive(Clone, PartialEq, Debug)]
pub enum DisplayClick {
    Nothing,
    /// SWAP: the hotbar slot gets a full stack of the item.
    Hotbar(usize, ItemStack),
    /// SWAP with button 40 (Key F): the off-hand gets a full stack of the item.
    Offhand(ItemStack),
    /// THROW: a copy of the item is dropped.
    Drop(ItemStack),
}

/// `CreativeModeInventoryScreen.slotClicked` on a display slot
/// (`slot.container == CONTAINER`): `shown` is the slot's item (None past
/// the tab's end) and `max` gives an item's max stack size. The tab's items
/// are never changed: clicks make copies.
pub fn click_display(
    carried: &mut Option<ItemStack>,
    shown: Option<&ItemStack>,
    max: impl Fn(&ItemStack) -> u32,
    button: i32,
    input: crate::menu::Input,
) -> DisplayClick {
    use crate::menu::Input;
    let copy = |s: &ItemStack, count: u32| ItemStack { count, ..s.clone() };
    let quick_key = input == Input::QuickMove;
    match input {
        Input::Swap => {
            return match shown {
                Some(s) if button == 40 => DisplayClick::Offhand(copy(s, max(s))),
                Some(s) if (0..9).contains(&button) => {
                    DisplayClick::Hotbar(button as usize, copy(s, max(s)))
                }
                Some(_) | None => DisplayClick::Nothing,
            };
        }
        Input::Clone => {
            if carried.is_none()
                && let Some(s) = shown
            {
                *carried = Some(copy(s, max(s)));
            }
            return DisplayClick::Nothing;
        }
        Input::Throw => {
            return match shown {
                Some(s) => DisplayClick::Drop(copy(s, if button == 0 { 1 } else { max(s) })),
                None => DisplayClick::Nothing,
            };
        }
        _ => {}
    }
    match (carried.as_mut(), shown) {
        (Some(c), Some(s)) if c.item == s.item && c.extra == s.extra && c.damage == s.damage => {
            // isSameItemSameComponents: grow, fill or shrink what is carried.
            if button == 0 {
                if quick_key {
                    c.count = max(c);
                } else if c.count < max(c) {
                    c.count += 1;
                }
            } else {
                c.count -= 1;
            }
        }
        (None, Some(s)) => {
            *carried = Some(copy(s, if quick_key { max(s) } else { s.count }));
        }
        (Some(c), _) => {
            if button == 0 {
                *carried = None;
            } else {
                c.count -= 1;
            }
        }
        (None, None) => {}
    }
    if carried.as_ref().is_some_and(|c| c.count == 0) {
        *carried = None;
    }
    DisplayClick::Nothing
}
