//! The player's advancements in the game: criteria triggered from game
//! events (`CriteriaTriggers`), awarding with rewards, the toast and the
//! chat announcement, and `players/advancements/<uuid>.json`.
use super::Game;
use horizon_client::{
    advancements::{self as adv, Frame, Progress, Tree},
    hud::HudBatch,
    items::ItemStack,
    text::Text,
    toasts::Slide,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Instant,
};

pub(super) struct Toast {
    icon: String,
    frame: Frame,
    title: Text,
    slide: Slide,
    /// `ToastInstance.firstSlotIndex`: the 32-pixel slot it stacks in once it
    /// leaves the queue.
    slot: Option<usize>,
    /// A `RecipeToast`: the crafting station and result of each display
    /// unlocked while it lasts (`recipeItems`); empty for an advancement.
    recipes: Vec<(String, ItemStack)>,
    /// `RecipeToast.changed` and `lastChanged`.
    changed: bool,
    last_changed: i64,
}
impl Toast {
    /// `RecipeToast.addOrUpdate` on a new toast.
    pub(super) fn recipe(items: Vec<(String, ItemStack)>) -> Self {
        Self {
            icon: String::new(),
            frame: Frame::Task,
            title: Text::literal(""),
            slide: Slide::default(),
            slot: None,
            recipes: items,
            changed: true,
            last_changed: 0,
        }
    }
    pub(super) fn is_recipe(&self) -> bool {
        !self.recipes.is_empty()
    }
    /// `RecipeToast.addItem`: more displays on a toast still queued or shown.
    pub(super) fn add_recipes(&mut self, items: Vec<(String, ItemStack)>) {
        self.recipes.extend(items);
        self.changed = true;
    }
    /// `Toast.update` with `fullyVisibleForMs`: whether it wants to be shown.
    fn wants_show(&mut self, fully_for: i64, multiplier: f64) -> bool {
        let time = 5000.0 * multiplier;
        if self.recipes.is_empty() {
            // AdvancementToast.
            return (fully_for as f64) < time;
        }
        if self.changed {
            self.last_changed = fully_for;
            self.changed = false;
        }
        ((fully_for - self.last_changed) as f64) < time
    }
    /// `RecipeToast.displayedRecipeIndex`: the displays take turns over the
    /// toast's display time.
    fn displayed_recipe(&self, fully_for: i64, multiplier: f64) -> usize {
        let n = self.recipes.len() as f64;
        (fully_for as f64 / (5000.0 * multiplier / n).max(1.0) % n) as usize
    }
}
#[derive(Default)]
pub(super) struct AdvState {
    pub(super) tree: Arc<Tree>,
    /// `ClientAdvancements`: a server's tree carries each advancement's
    /// place on its tab (`DisplayInfo` x, y), in tree order.
    pub(super) positions: Option<Vec<(f32, f32)>>,
    pub(super) progress: Progress,
    pub(super) toasts: VecDeque<Toast>,
    /// Awards skip the toast (`/advancement grant ... everything`).
    pub(super) quiet: bool,
}
impl Game<'_> {
    /// `ToastManager.SLOT_COUNT`.
    pub(super) const TOAST_SLOTS: usize = 5;
    /// `ToastManager.update` and `extractRenderState` for the `AdvancementToast`s and
    /// `RecipeToast`s: up to five stack in 32-pixel slots, the rest queue. `hidden` (F1)
    /// keeps them timing out without drawing them.
    pub(super) fn draw_toasts(&mut self, batch: &mut HudBatch, hidden: bool) {
        let multiplier = f64::from(self.settings.notification_display_time);
        let mut sound_played = false;
        let mut i = 0;
        while i < self.adv.toasts.len() {
            let t = &mut self.adv.toasts[i];
            if t.slot.is_none() {
                i += 1;
                continue;
            }
            let origin = *t.slide.origin.get_or_insert_with(Instant::now);
            let now = origin.elapsed().as_millis() as i64;
            let mut slide = std::mem::take(&mut t.slide);
            let (sound, finished) = slide.update(now, |fully| t.wants_show(fully, multiplier));
            t.slide = slide;
            if finished {
                self.adv.toasts.remove(i);
                continue;
            }
            // Only the first visibility change of an update plays its sound.
            if let Some(show) = sound
                && !std::mem::replace(&mut sound_played, true)
            {
                let event = if show { "ui.toast.in" } else { "ui.toast.out" };
                self.sounds
                    .play(&format!("minecraft:{event}"), None, 1.0, 1.0);
            }
            i += 1;
        }
        // The queued toasts take the free slots in order; a challenge's own sound
        // plays once as it leaves the queue.
        let mut used = self.toast_slots_used();
        let mut challenge_sound = false;
        for t in &mut self.adv.toasts {
            if t.slot.is_some() {
                continue;
            }
            let Some(slot) = (0..Self::TOAST_SLOTS).find(|s| used & (1 << s) == 0) else {
                break;
            };
            used |= 1 << slot;
            t.slot = Some(slot);
            challenge_sound |= !t.is_recipe() && t.frame == Frame::Challenge;
        }
        if challenge_sound {
            self.sounds
                .play("minecraft:ui.toast.challenge_complete", None, 1.0, 1.0);
        }
        if hidden {
            return;
        }
        for i in 0..self.adv.toasts.len() {
            if let Some(slot) = self.adv.toasts[i].slot {
                self.draw_toast(batch, i, 32.0 * slot as f32, multiplier);
            }
        }
    }
    /// The `AdvancementToast` or `RecipeToast` at `index` in its slot at `y`.
    fn draw_toast(&mut self, batch: &mut HudBatch, index: usize, y: f32, multiplier: f64) {
        let t = &self.adv.toasts[index];
        let x = batch.width - 160.0 * t.slide.portion;
        let fully_for = t.slide.fully_for;
        let (title, frame, icon) = (t.title.clone(), t.frame, t.icon.clone());
        let recipe = t
            .is_recipe()
            .then(|| t.recipes[t.displayed_recipe(fully_for, multiplier)].clone());
        if let Some((station, item)) = recipe {
            // RecipeToast: black description under a purple title, the
            // station at 0.6 scale in the corner over the cycling result.
            let atlas = &self.front.gui.atlas;
            batch.sprite(atlas, "gui/sprites/toast/recipe", x, y);
            let title = self.translate("recipe.toast.title", "New Recipe(s) Unlocked!");
            let description = self.translate("recipe.toast.description", "Check your recipe book");
            let atlas = &self.front.gui.atlas;
            batch.text(atlas, &title, x + 30.0, y + 7.0, 0xFF50_0050, false);
            batch.text(atlas, &description, x + 30.0, y + 18.0, 0xFF00_0000, false);
            let start = batch.vertices.len();
            self.draw_item_model(batch, &ItemStack::new(&station, 1), x + 3.0, y + 3.0);
            batch.scale_since(start, x, y, 0.6, 0.6);
            self.draw_item_model(batch, &item, x + 8.0, y + 8.0);
            return;
        }
        let atlas = &self.front.gui.atlas;
        batch.sprite(atlas, "gui/sprites/toast/advancement", x, y);
        let (key, color) = match frame {
            Frame::Task => ("advancements.toast.task", 0xFF00_FFFF),
            Frame::Goal => ("advancements.toast.goal", 0xFF00_FFFF),
            Frame::Challenge => ("advancements.toast.challenge", 0xFFFF_88FF),
        };
        batch.text_styled(
            atlas,
            &Text::translate(key, &[]),
            x + 30.0,
            y + 7.0,
            color,
            false,
        );
        batch.text_styled(atlas, &title, x + 30.0, y + 18.0, u32::MAX, false);
        self.draw_item(batch, &ItemStack::new(&icon, 1), x + 8.0, y + 8.0);
    }
}
impl crate::game::Sim<'_> {
    pub(super) fn load_advancements(&mut self) {}
    /// The slots held by shown advancement, recipe and pack toasts, as bits.
    pub(super) fn toast_slots_used(&self) -> u32 {
        let packs = self.server_packs.toast_slots();
        self.adv
            .toasts
            .iter()
            .filter_map(|t| t.slot)
            .chain(packs)
            .fold(0, |used, slot| used | 1 << slot)
    }
}
impl crate::game::Sim<'_> {
    /// `ClientAdvancements.update` (ClientboundUpdateAdvancementsPacket):
    /// the server's advancements replace or join the tree, each changed one
    /// takes the progress sent, and one just done shows its toast. `icons`
    /// are the added advancements' icon items by name.
    pub(super) fn remote_advancements(
        &mut self,
        p: horizon_protocol::packets::play::UpdateAdvancements,
        icons: Vec<String>,
    ) {
        use horizon_client::scoreboard::tag_to_json;
        let mut list: Vec<adv::Advancement> = Vec::new();
        let mut positions: Vec<(f32, f32)> = Vec::new();
        if !p.reset {
            let old = self.adv.positions.take().unwrap_or_default();
            for (i, a) in self.adv.tree.list.iter().enumerate() {
                if !p.removed.contains(&a.id) {
                    list.push(a.clone());
                    positions.push(old.get(i).copied().unwrap_or((0.0, 0.0)));
                }
            }
        } else {
            self.adv.progress = Progress::default();
        }
        for id in &p.removed {
            self.adv.progress.criteria.remove(id);
        }
        for (a, icon) in p.added.into_iter().zip(icons) {
            list.retain(|o| o.id != a.id);
            let display = a.display.map(|d| adv::Display {
                icon,
                title: tag_to_json(&d.title),
                description: tag_to_json(&d.description),
                frame: match d.frame {
                    1 => Frame::Challenge,
                    2 => Frame::Goal,
                    _ => Frame::Task,
                },
                background: d.background,
                show_toast: d.show_toast,
                announce: false,
                hidden: d.hidden,
            });
            let mut criteria: Vec<adv::Criterion> = Vec::new();
            for name in a.requirements.iter().flatten() {
                if !criteria.iter().any(|c| &c.name == name) {
                    criteria.push(adv::Criterion {
                        name: name.clone(),
                        trigger: String::new(),
                        conditions: Value::Null,
                    });
                }
            }
            list.push(adv::Advancement {
                id: a.id,
                parent: a.parent,
                display,
                criteria,
                requirements: a.requirements,
                rewards: Default::default(),
            });
            positions.push((a.x, a.y));
        }
        let mut tree = Tree::default();
        tree.list = list;
        for (i, a) in tree.list.iter().enumerate() {
            tree.by_id.insert(a.id.clone(), i);
        }
        self.world_data.registries.advancements = tree.list.iter().map(|a| a.id.clone()).collect();
        self.adv.tree = Arc::new(tree);
        self.adv.positions = Some(positions);
        let tree = self.adv.tree.clone();
        for (id, criteria) in p.progress {
            let Some(&i) = tree.by_id.get(&id) else {
                continue;
            };
            // The client only shows whether a criterion is obtained, not when.
            let obtained: BTreeMap<String, String> = criteria
                .into_iter()
                .filter(|(_, at)| at.is_some())
                .map(|(name, _)| (name, String::new()))
                .collect();
            self.adv.progress.criteria.insert(id, obtained);
            let a = &tree.list[i];
            if !p.reset
                && p.show
                && self.adv.progress.done(a)
                && let Some(d) = a.display.as_ref().filter(|d| d.show_toast)
            {
                self.adv.toasts.push_back(Toast {
                    icon: d.icon.clone(),
                    frame: d.frame,
                    title: text_of(&d.title),
                    slide: Slide::default(),
                    slot: None,
                    recipes: Vec::new(),
                    changed: false,
                    last_changed: 0,
                });
            }
        }
    }
}
/// An advancement's title or description: a whole component (ComponentSerialization.CODEC),
/// with its siblings and styles, as a plugin's `{"text":"","extra":[...]}` needs.
pub(super) fn text_of(v: &Value) -> Text {
    horizon_client::scoreboard::comp_text(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipes(n: usize) -> Toast {
        Toast::recipe(
            (0..n)
                .map(|i| {
                    let item = ItemStack::new(&format!("minecraft:item_{i}"), 1);
                    ("minecraft:crafting_table".to_owned(), item)
                })
                .collect(),
        )
    }
    /// Drives a toast frame by frame (every `step` ms) and returns the slide
    /// portions at the given times and when it finished.
    fn run(toast: &mut Toast, step: i64, until: i64, at: &[i64]) -> (Vec<f32>, Option<i64>) {
        let mut slide = Slide::default();
        let mut seen = Vec::new();
        for now in (0..=until).step_by(step as usize) {
            let (_, finished) = slide.update(now, |f| toast.wants_show(f, 1.0));
            if finished {
                return (seen, Some(now));
            }
            if at.contains(&now) {
                seen.push(slide.portion);
            }
        }
        (seen, None)
    }

    #[test]
    fn titles_keep_their_siblings_and_styles() {
        let title = text_of(&serde_json::json!({
            "text": "",
            "extra": [{"text": "Quest ", "color": "gold"}, "Board"],
        }));
        assert_eq!(title.plain(), "Quest Board");
        assert_eq!(title.spans[0].style.color, Some(horizon_client::text::GOLD));
        assert_eq!(title.spans[1].style.color, None);
        let vanilla = text_of(&serde_json::json!({"translate": "advancements.story.root.title"}));
        assert_eq!(
            vanilla,
            Text::translate("advancements.story.root.title", &[])
        );
        assert_eq!(
            vanilla.translation_key(),
            Some("advancements.story.root.title")
        );
    }

    #[test]
    fn toasts_slide_in_squared_and_out_after_five_seconds() {
        let mut toast = recipes(1);
        let (portions, finished) = run(&mut toast, 10, 10_000, &[0, 300, 600, 3000]);
        // (300 / 600)² = 0.25: ToastInstance.calculateVisiblePortion.
        assert_eq!(portions, [0.0, 0.25, 1.0, 1.0]);
        // Fully in at 600, shown 5000 ms, out over 600 more.
        assert!(
            finished.is_some_and(|t| (6200..=6220).contains(&t)),
            "{finished:?}"
        );
    }

    #[test]
    fn the_first_update_plays_the_show_sound_and_the_hide_sound_follows() {
        let mut toast = recipes(1);
        let mut slide = Slide::default();
        assert_eq!(slide.update(0, |f| toast.wants_show(f, 1.0)).0, Some(true));
        assert_eq!(slide.update(100, |f| toast.wants_show(f, 1.0)).0, None);
        assert_eq!(
            slide.update(5600, |f| toast.wants_show(f, 1.0)).0,
            Some(false)
        );
    }

    #[test]
    fn recipe_toasts_cycle_through_their_displays_over_the_display_time() {
        let toast = recipes(4);
        let shown: Vec<usize> = [0, 1249, 1250, 2500, 3750, 4999, 5000]
            .iter()
            .map(|&t| toast.displayed_recipe(t, 1.0))
            .collect();
        assert_eq!(shown, [0, 0, 1, 2, 3, 3, 0]);
        // notificationDisplayTime doubles each turn.
        assert_eq!(toast.displayed_recipe(2500, 2.0), 1);
    }

    #[test]
    fn unlocking_more_recipes_keeps_the_toast_up_five_more_seconds() {
        let mut toast = recipes(1);
        let mut slide = Slide::default();
        let mut finished = None;
        for now in (0..=20_000).step_by(10) {
            if now == 4000 {
                toast.add_recipes(recipes(1).recipes);
            }
            if slide.update(now, |f| toast.wants_show(f, 1.0)).1 {
                finished = Some(now);
                break;
            }
        }
        assert_eq!(toast.recipes.len(), 2);
        // lastChanged moved to 3400 ms fully shown: hidden at 8400 + 600.
        assert!(
            finished.is_some_and(|t| (9600..=9620).contains(&t)),
            "{finished:?}"
        );
    }
}
