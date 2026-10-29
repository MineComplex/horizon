//! The merchant (villager trading) menu: `MerchantMenu` / `MerchantScreen`
//! layout (276x166, drawn 50 px left of the usual 176-wide origin), offer
//! buttons and payment slots over the server's offers (`ClientSideMerchant`).
use super::{
    Game,
    stations::{INPUT, Kind, RESULT, Station},
};
use horizon_client::{
    hud::{HudBatch, rgba},
    items::ItemStack,
    merchant::{self, MerchantOffer},
};

/// The merchant screen is 100 px wider than the 176-px menus it shares an origin with.
const SHIFT: f32 = -50.0;
const GREY: u32 = 0xFF40_4040;

pub(super) struct MerchantState {
    /// The merchant's side of the menu: offers, experience, level, the
    /// selection hint and active offer (`horizon_client::merchant::Session`).
    pub(super) session: Box<merchant::Session>,
    /// The offer list's scroll offset.
    pub(super) scroll: usize,
    /// The offer button that keeps focus after a click (drawn highlighted).
    pub(super) focused: Option<usize>,
    /// The merchant's custom name.
    pub(super) name: Option<String>,
}
impl MerchantState {
    /// `Merchant.showProgressBar`: villagers only.
    pub(super) fn progress(&self) -> bool {
        self.session.show_progress()
    }
}
/// Player inventory slot positions in `MerchantMenu` (x from 108).
pub(super) fn inventory_slot(i: usize) -> (f32, f32) {
    match i {
        0..9 => (108.0 + SHIFT + i as f32 * 18.0, 142.0),
        _ => (
            108.0 + SHIFT + ((i - 9) % 9) as f32 * 18.0,
            84.0 + ((i - 9) / 9) as f32 * 18.0,
        ),
    }
}

impl Game<'_> {
    /// `MerchantScreen` rendering.
    pub(super) fn draw_merchant(&mut self, batch: &mut HudBatch) -> bool {
        let (ox, oy) = self.inventory_origin(batch);
        let local = (self.front.cursor.0 - ox, self.front.cursor.1 - oy);
        let Some(m) = self.station.as_ref().and_then(|s| s.merchant.as_ref()) else {
            return false;
        };
        let (progress, custom) = (m.progress(), m.name.clone());
        let (profession, level, xp, scroll, shop, future, focused) = (
            m.session
                .profession
                .trim_start_matches("minecraft:")
                .to_owned(),
            m.session.level,
            m.session.xp,
            m.scroll,
            m.session.selection_hint,
            m.session.future_xp,
            m.focused,
        );
        let offers = m.session.offers.clone();
        let costs: Vec<ItemStack> = offers.iter().map(|o| o.cost_a(self.res.items())).collect();
        let atlas = &self.front.gui.atlas;
        batch.fill(
            atlas,
            0.0,
            0.0,
            batch.width,
            batch.height,
            rgba(16, 16, 16, 160),
        );
        if let Some(mut r) = atlas.sprites.get("gui/container/villager").copied() {
            let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
            r.u1 = r.u0 + 276.0 * du;
            r.v1 = r.v0 + 166.0 * dv;
            batch.rect_uv(ox + SHIFT, oy, 276.0, 166.0, r, u32::MAX);
        }
        let e = &self.res.stations().enchantments;
        let tr = |k: &str, f: &str| e.translate(k).unwrap_or(f).to_owned();
        let name = custom.unwrap_or_else(|| {
            if progress {
                tr(
                    &format!("entity.minecraft.villager.{profession}"),
                    &profession,
                )
            } else {
                tr("entity.minecraft.wandering_trader", "Wandering Trader")
            }
        });
        let title = if progress {
            tr("merchant.title", "%s - %s")
                .replacen("%s", &name, 1)
                .replacen("%s", &tr(&format!("merchant.level.{level}"), ""), 1)
        } else {
            name
        };
        let tw = HudBatch::text_width(atlas, &title);
        batch.text(
            atlas,
            &title,
            ox + SHIFT + (49.0 + 138.0 - (tw / 2.0).floor()),
            oy + 6.0,
            GREY,
            false,
        );
        let trades = tr("merchant.trades", "Trades");
        let tw = HudBatch::text_width(atlas, &trades);
        batch.text(
            atlas,
            &trades,
            ox + SHIFT + 5.0 - (tw / 2.0).floor() + 48.0,
            oy + 6.0,
            GREY,
            false,
        );
        batch.text(
            atlas,
            &tr("container.inventory", "Inventory"),
            ox + SHIFT + 107.0,
            oy + 72.0,
            GREY,
            false,
        );
        if offers.get(shop).is_some_and(MerchantOffer::out_of_stock) {
            batch.sprite(
                atlas,
                "gui/sprites/container/villager/out_of_stock",
                ox + SHIFT + 182.0,
                oy + 35.0,
            );
        }
        // Experience bar (levels 1-4).
        if progress && level < 5 {
            batch.sprite(
                atlas,
                "gui/sprites/container/villager/experience_bar_background",
                ox + SHIFT + 136.0,
                oy + 16.0,
            );
            let min = merchant::min_xp(level);
            if xp >= min && merchant::can_level_up(level) {
                let mult = 102.0 / (merchant::max_xp(level) - min) as f32;
                let w = ((mult * (xp - min) as f32).floor()).min(102.0);
                batch.sprite_part(
                    atlas,
                    "gui/sprites/container/villager/experience_bar_current",
                    ox + SHIFT + 136.0,
                    oy + 16.0,
                    w / 102.0,
                );
                if future > 0 {
                    let fw = (future as f32 * mult).floor().min(102.0 - w);
                    if let Some(mut r) = atlas
                        .sprites
                        .get("gui/sprites/container/villager/experience_bar_result")
                        .copied()
                    {
                        let du = (r.u1 - r.u0) / r.w as f32;
                        r.u0 += w * du;
                        r.u1 = r.u0 + fw * du;
                        batch.rect_uv(ox + SHIFT + 136.0 + w, oy + 16.0, fw, 5.0, r, u32::MAX);
                    }
                }
            }
        }
        // Scroller.
        let steps = offers.len() as i32 + 1 - 7;
        if steps > 1 {
            let left = 139 - (27 + (steps - 1) * 139 / steps);
            let step = 1 + left / steps + 139 / steps;
            let mut y = (scroll as i32 * step).min(113);
            if scroll as i32 == steps - 1 {
                y = 113;
            }
            batch.sprite(
                atlas,
                "gui/sprites/container/villager/scroller",
                ox + SHIFT + 94.0,
                oy + 18.0 + y as f32,
            );
        } else {
            batch.sprite(
                atlas,
                "gui/sprites/container/villager/scroller_disabled",
                ox + SHIFT + 94.0,
                oy + 18.0,
            );
        }
        // Offer buttons.
        let mut tooltip: Option<ItemStack> = None;
        for (row, (index, offer)) in offers.iter().enumerate().skip(scroll).take(7).enumerate() {
            let (bx, by) = (ox + SHIFT + 5.0, oy + 18.0 + 20.0 * row as f32);
            let hover = local.0 >= 5.0 + SHIFT
                && local.0 < 93.0 + SHIFT
                && local.1 >= 18.0 + 20.0 * row as f32
                && local.1 < 38.0 + 20.0 * row as f32;
            let sprite = if hover || focused == Some(row) {
                "gui/sprites/widget/button_highlighted"
            } else {
                "gui/sprites/widget/button"
            };
            batch.sprite_scaled(&self.front.gui.atlas, sprite, bx, by, 88.0, 20.0, u32::MAX);
            let y = oy + 16.0 + 1.0 + 20.0 * row as f32 + 2.0;
            let base = offer.base_cost_a.stack.clone();
            let cost_a = costs[index].clone();
            let x = ox + SHIFT + 10.0;
            if base.count == cost_a.count {
                self.draw_item(batch, &cost_a, x, y);
            } else {
                // Discounted: the base price struck through, then the new price.
                self.draw_item(
                    batch,
                    &ItemStack {
                        count: 1,
                        ..cost_a.clone()
                    },
                    x,
                    y,
                );
                let atlas = &self.front.gui.atlas;
                let t = base.count.to_string();
                batch.text(
                    atlas,
                    &t,
                    x + 17.0 - HudBatch::text_width(atlas, &t),
                    y + 9.0,
                    u32::MAX,
                    true,
                );
                let t = cost_a.count.to_string();
                batch.text(
                    atlas,
                    &t,
                    x + 14.0 + 17.0 - HudBatch::text_width(atlas, &t),
                    y + 9.0,
                    u32::MAX,
                    true,
                );
                batch.sprite(
                    atlas,
                    "gui/sprites/container/villager/discount_strikethrough",
                    x + 7.0,
                    y + 12.0,
                );
            }
            if let Some(b) = offer.cost_b() {
                self.draw_item(batch, &b, ox + SHIFT + 40.0, y);
            }
            let arrow = if offer.out_of_stock() {
                "trade_arrow_out_of_stock"
            } else {
                "trade_arrow"
            };
            batch.sprite(
                &self.front.gui.atlas,
                &format!("gui/sprites/container/villager/{arrow}"),
                ox + SHIFT + 60.0,
                y + 3.0,
            );
            self.draw_item(batch, &offer.result, ox + SHIFT + 73.0, y);
            if hover && self.carried.is_none() {
                let rel = local.0 - (5.0 + SHIFT);
                tooltip = if rel < 20.0 {
                    Some(cost_a)
                } else if rel > 30.0 && rel < 50.0 {
                    offer.cost_b()
                } else if rel > 65.0 {
                    Some(offer.result.clone())
                } else {
                    None
                };
            }
        }
        // Slots, hover highlight, carried stack, tooltip.
        let slots: Vec<usize> = (0..36).chain([INPUT, INPUT + 1, RESULT]).collect();
        let drag = self.drag_view();
        for i in slots {
            let (sx, sy) = self.slot_position(i);
            let stack = self.slot_mut(i).clone();
            self.draw_slot_item(batch, drag.as_ref(), i, &stack, ox + sx, oy + sy);
        }
        if let Some(slot) = self.slot_at(batch, self.front.cursor.0, self.front.cursor.1) {
            let (sx, sy) = self.slot_position(slot);
            batch.fill(
                &self.front.gui.atlas,
                ox + sx,
                oy + sy,
                16.0,
                16.0,
                rgba(255, 255, 255, 128),
            );
            if self.carried.is_none() && tooltip.is_none() {
                tooltip = self.slot_mut(slot).clone();
            }
        }
        // An out-of-stock trade: "the villager needs to restock" over the
        // result (isHovering(186, 35, 22, 21), canRestock).
        let deprecated = progress
            && offers.get(shop).is_some_and(MerchantOffer::out_of_stock)
            && local.0 >= 186.0 + SHIFT - 1.0
            && local.0 < 186.0 + SHIFT + 22.0 + 1.0
            && local.1 >= 35.0 - 1.0
            && local.1 < 35.0 + 21.0 + 1.0;
        if self.carried.is_some() {
            self.draw_carried(batch);
        } else if deprecated {
            let text = self
                .res
                .stations()
                .enchantments
                .translate("merchant.deprecated")
                .unwrap_or("Villagers restock up to two times per day.")
                .to_owned();
            let lines: Vec<(String, u32)> =
                text.split('\n').map(|l| (l.to_owned(), u32::MAX)).collect();
            self.draw_tooltip_lines(batch, &lines, self.front.cursor.0, self.front.cursor.1);
        } else if let Some(stack) = tooltip {
            self.draw_stack_tooltip(batch, &stack, self.front.cursor.0, self.front.cursor.1);
        }
        true
    }
}
impl crate::game::Sim<'_> {
    /// A server's trading window (no mob here: the server keeps it).
    pub(super) fn open_remote_merchant(
        &mut self,
        session: merchant::Session,
        name: Option<String>,
    ) {
        self.open_merchant_state(MerchantState {
            session: Box::new(session),
            scroll: 0,
            focused: None,
            name,
        });
    }
    fn open_merchant_state(&mut self, state: MerchantState) {
        let mut s = Station::new_kind(Kind::Merchant, [0, 0, 0]);
        s.merchant = Some(Box::new(state));
        self.station = Some(s);
        self.screen = super::Screen::Inventory;
        self.capture(false);
        self.right = false;
    }
    /// After a menu click: the session back to the station. A client's
    /// merchant (`ClientSideMerchant`) hears of no trade: the orbs, the
    /// level-up and the yes/no voice are the server's.
    pub(super) fn merchant_sync_out(&mut self, mut session: Box<merchant::Session>) {
        session.events.clear();
        if let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) {
            m.session = session;
        }
    }
    /// `MerchantMenu.tryMoveItems` for the selected offer (the select trade packet).
    fn try_move_items(&mut self, index: usize) {
        // ServerboundSelectTradePacket; the server moves the items.
        if let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) {
            m.session.selection_hint = index;
        }
        self.send(horizon_protocol::packets::play::Serverbound::SelectTrade(
            horizon_protocol::packets::play::SelectTrade { item: index as i32 },
        ));
    }
    /// Clicks on the offer list; true when one was hit.
    pub(super) fn merchant_click(&mut self, local: (f32, f32)) -> bool {
        let Some(m) = self.station.as_ref().and_then(|s| s.merchant.as_ref()) else {
            return false;
        };
        for i in 0..7 {
            let (x, y) = (5.0 + SHIFT, 18.0 + 20.0 * i as f32);
            if local.0 >= x
                && local.0 < x + 88.0
                && local.1 >= y
                && local.1 < y + 20.0
                && i + m.scroll < m.session.offers.len()
            {
                let index = i + m.scroll;
                if let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) {
                    m.focused = Some(i);
                }
                self.sounds
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
                self.try_move_items(index);
                return true;
            }
        }
        false
    }
    /// The offer buttons a controller can focus (GUI origin relative).
    pub(super) fn merchant_targets(&self) -> Vec<[f32; 4]> {
        let Some(m) = self.station.as_ref().and_then(|s| s.merchant.as_ref()) else {
            return Vec::new();
        };
        (0..7)
            .filter(|i| i + m.scroll < m.session.offers.len())
            .map(|i| [5.0 + SHIFT, 18.0 + 20.0 * i as f32, 88.0, 20.0])
            .collect()
    }
    pub(super) fn merchant_scroll(&mut self, d: f32) {
        let Some(m) = self.station.as_mut().and_then(|s| s.merchant.as_mut()) else {
            return;
        };
        if m.session.offers.len() > 7 {
            let max = m.session.offers.len() - 7;
            m.scroll = (m.scroll as i32 - d.signum() as i32).clamp(0, max as i32) as usize;
        }
    }
}
