//! Port of `net.kyori.adventure.text.format.Style` (and `StyleImpl`, `Style.Builder`, `Style.Merge`).
//!
//! A `Style` is an immutable value; its setters return a new style. Overloads follow the arity rule:
//! the getter is `color()`, the setter `color_1(..)`.
use crate::prelude::*;
use crate::net::kyori::adventure::key::Key;
use crate::net::kyori::adventure::text::event::{ClickEvent, HoverEvent};
use crate::net::kyori::adventure::text::format::decoration_map::{DecorationMap, DECORATIONS};
use crate::net::kyori::adventure::text::format::{ShadowColor, StyleBuilderApplicable, TextColor, TextDecoration, TextDecorationState};

// ---- interface net.kyori.adventure.text.format.Style
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Style {
    pub(crate) font: Option<JRef<dyn Key>>,
    pub(crate) color: Option<TextColor>,
    pub(crate) shadow_color: Option<ShadowColor>,
    pub(crate) decorations: DecorationMap,
    pub(crate) click_event: Option<ClickEvent>,
    pub(crate) hover_event: Option<HoverEvent>,
    pub(crate) insertion: Option<String>,
}
pub const DEFAULT_FONT: &str = "minecraft:default";

/// `Style.empty()`.
pub static EMPTY: LazyLock<Style> = LazyLock::new(Style::empty);

impl Default for Style {
    fn default() -> Self {
        Style::empty()
    }
}
impl Style {
    pub fn empty() -> Style {
        Style {
            font: None,
            color: None,
            shadow_color: None,
            decorations: DecorationMap::EMPTY,
            click_event: None,
            hover_event: None,
            insertion: None,
        }
    }
    /// `Style.style()`: a new builder.
    pub fn style() -> StyleBuilder {
        StyleBuilder(Arc::new(Mut::new(Style::empty())))
    }
    /// `Style.style(Consumer<Builder>)`.
    pub fn style_consumer(consumer: impl FnOnce(&StyleBuilder)) -> Style {
        let b = Style::style();
        consumer(&b);
        b.build()
    }
    /// `Style.style(TextColor)` / `Style.style(TextDecoration)` / `Style.style(StyleBuilderApplicable)`:
    /// the style the one applicable makes.
    pub fn style_1(applicable: impl StyleBuilderApplicable) -> Style {
        let b = Style::style();
        applicable.style_apply(&b);
        b.build()
    }
    /// `Style.style(TextColor, TextDecoration...)` / `Style.style(TextColor, Set<TextDecoration>)`.
    pub fn style_2(color: Option<TextColor>, decorations: impl IntoIterator<Item = TextDecoration>) -> Style {
        let builder = Style::style();
        builder.color(color);
        for decoration in decorations {
            builder.decoration(decoration, TextDecorationState::TRUE);
        }
        builder.build()
    }
    /// `Style.style(StyleBuilderApplicable...)` / `Style.style(Iterable<StyleBuilderApplicable>)`.
    pub fn style_applicables(applicables: &[&dyn StyleBuilderApplicable]) -> Style {
        if applicables.is_empty() {
            return Style::empty();
        }
        let builder = Style::style();
        for applicable in applicables {
            applicable.style_apply(&builder);
        }
        builder.build()
    }
    /// `edit(Consumer<Builder>)`: `edit(consumer, Merge.Strategy.ALWAYS)`.
    pub fn edit(&self, consumer: impl FnOnce(&StyleBuilder)) -> Style {
        self.edit_2(consumer, StyleMergeStrategy::ALWAYS)
    }
    /// `edit(Consumer<Builder>, Merge.Strategy)`.
    pub fn edit_2(&self, consumer: impl FnOnce(&StyleBuilder), strategy: StyleMergeStrategy) -> Style {
        Style::style_consumer(|style| {
            if strategy == StyleMergeStrategy::ALWAYS {
                style.merge_2(self, strategy);
            }
            consumer(style);
            if strategy == StyleMergeStrategy::IF_ABSENT_ON_TARGET {
                style.merge_2(self, strategy);
            }
        })
    }
    pub fn font(&self) -> Option<JRef<dyn Key>> {
        self.font.clone()
    }
    pub fn font_1(&self, font: Option<JRef<dyn Key>>) -> Style {
        if self.font == font {
            return self.clone();
        }
        Style { font, ..self.clone() }
    }
    pub fn color(&self) -> Option<TextColor> {
        self.color
    }
    pub fn color_1(&self, color: Option<TextColor>) -> Style {
        if self.color == color {
            return self.clone();
        }
        Style { color, ..self.clone() }
    }
    pub fn color_if_absent(&self, color: Option<TextColor>) -> Style {
        if self.color.is_none() {
            return self.color_1(color);
        }
        self.clone()
    }
    pub fn shadow_color(&self) -> Option<ShadowColor> {
        self.shadow_color
    }
    pub fn shadow_color_1(&self, argb: Option<ShadowColor>) -> Style {
        if self.shadow_color == argb {
            return self.clone();
        }
        Style { shadow_color: argb, ..self.clone() }
    }
    pub fn shadow_color_if_absent(&self, argb: Option<ShadowColor>) -> Style {
        if self.shadow_color.is_none() {
            return self.shadow_color_1(argb);
        }
        self.clone()
    }
    /// `hasDecoration(TextDecoration)`: `decoration(d) == State.TRUE`.
    pub fn has_decoration(&self, decoration: TextDecoration) -> bool {
        self.decoration(decoration) == TextDecorationState::TRUE
    }
    /// `decoration(TextDecoration)`: the state.
    pub fn decoration(&self, decoration: TextDecoration) -> TextDecorationState {
        self.decorations.get(decoration)
    }
    /// `decorate(TextDecoration)`: sets it to `TRUE`.
    pub fn decorate(&self, decoration: TextDecoration) -> Style {
        self.decoration_2(decoration, TextDecorationState::TRUE)
    }
    /// `decoration(TextDecoration, boolean)` / `decoration(TextDecoration, State)`.
    pub fn decoration_2(&self, decoration: TextDecoration, state: impl Into<TextDecorationState>) -> Style {
        let state = state.into();
        if self.decoration(decoration) == state {
            return self.clone();
        }
        Style { decorations: self.decorations.with(decoration, state), ..self.clone() }
    }
    pub fn decoration_if_absent(&self, decoration: TextDecoration, state: TextDecorationState) -> Style {
        if self.decorations.get(decoration) == TextDecorationState::NOT_SET {
            return Style { decorations: self.decorations.with(decoration, state), ..self.clone() };
        }
        self.clone()
    }
    pub fn decorations(&self) -> IndexMap<TextDecoration, TextDecorationState> {
        self.decorations.to_map()
    }
    /// `decorations(Map<TextDecoration, State>)`.
    pub fn decorations_1(&self, decorations: &IndexMap<TextDecoration, TextDecorationState>) -> Style {
        Style { decorations: DecorationMap::merge(decorations, &self.decorations), ..self.clone() }
    }
    /// `decorations(Set<TextDecoration>, boolean)` (StyleSetter default).
    pub fn decorations_2(&self, decorations: &[TextDecoration], flag: bool) -> Style {
        let state = TextDecorationState::by_boolean(flag);
        let map: IndexMap<TextDecoration, TextDecorationState> = decorations.iter().map(|d| (*d, state)).collect();
        self.decorations_1(&map)
    }
    pub fn click_event(&self) -> Option<ClickEvent> {
        self.click_event.clone()
    }
    pub fn click_event_1(&self, event: Option<ClickEvent>) -> Style {
        Style { click_event: event, ..self.clone() }
    }
    pub fn hover_event(&self) -> Option<HoverEvent> {
        self.hover_event.clone()
    }
    /// `hoverEvent(HoverEventSource<?>)`: pass the erased event (`source.as_hover_event().erase()`, or
    /// `.into()` from a typed `HoverEvent<V>`).
    pub fn hover_event_1(&self, source: Option<HoverEvent>) -> Style {
        Style { hover_event: source, ..self.clone() }
    }
    pub fn insertion(&self) -> Option<String> {
        self.insertion.clone()
    }
    pub fn insertion_1(&self, insertion: Option<String>) -> Style {
        if self.insertion == insertion {
            return self.clone();
        }
        Style { insertion, ..self.clone() }
    }
    /// `merge(Style)`: `merge(that, Merge.all())`.
    pub fn merge(&self, that: &Style) -> Style {
        self.merge_3(that, StyleMergeStrategy::ALWAYS, &StyleMerge::all())
    }
    /// `merge(Style, Merge.Strategy)`.
    pub fn merge_2(&self, that: &Style, strategy: StyleMergeStrategy) -> Style {
        self.merge_3(that, strategy, &StyleMerge::all())
    }
    /// `merge(Style, Merge...)` / `merge(Style, Set<Merge>)`.
    pub fn merge_2_merges(&self, that: &Style, merges: &[StyleMerge]) -> Style {
        self.merge_3(that, StyleMergeStrategy::ALWAYS, merges)
    }
    /// `merge(Style, Merge.Strategy, Set<Merge>)` (and the `Merge...` forms).
    pub fn merge_3(&self, that: &Style, strategy: StyleMergeStrategy, merges: &[StyleMerge]) -> Style {
        if nothing_to_merge(that, strategy, merges) {
            return self.clone();
        }
        if self.is_empty() && StyleMerge::has_all(merges) {
            return that.clone();
        }
        let builder = self.to_builder();
        builder.merge_3(that, strategy, merges);
        builder.build()
    }
    pub fn unmerge(&self, that: &Style) -> Style {
        if self.is_empty() {
            return self.clone();
        }
        let builder = self.to_builder();
        if self.font == that.font {
            builder.font(None);
        }
        if self.color == that.color {
            builder.color(None);
        }
        if self.shadow_color == that.shadow_color {
            builder.shadow_color(None);
        }
        for decoration in DECORATIONS {
            if self.decoration(decoration) == that.decoration(decoration) {
                builder.decoration(decoration, TextDecorationState::NOT_SET);
            }
        }
        if self.click_event == that.click_event {
            builder.click_event(None);
        }
        if self.hover_event == that.hover_event {
            builder.hover_event(None);
        }
        if self.insertion == that.insertion {
            builder.insertion(None);
        }
        builder.build()
    }
    pub fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.shadow_color.is_none()
            && self.decorations == DecorationMap::EMPTY
            && self.click_event.is_none()
            && self.hover_event.is_none()
            && self.insertion.is_none()
            && self.font.is_none()
    }
    pub fn to_builder(&self) -> StyleBuilder {
        StyleBuilder(Arc::new(Mut::new(self.clone())))
    }
}
fn nothing_to_merge(merge_from: &Style, strategy: StyleMergeStrategy, merges: &[StyleMerge]) -> bool {
    strategy == StyleMergeStrategy::NEVER || merge_from.is_empty() || merges.is_empty()
}
impl From<TextColor> for Style {
    fn from(c: TextColor) -> Self {
        Style::empty().color_1(Some(c))
    }
}
impl From<Option<TextColor>> for Style {
    fn from(c: Option<TextColor>) -> Self {
        Style::empty().color_1(c)
    }
}
impl From<TextDecoration> for Style {
    fn from(d: TextDecoration) -> Self {
        Style::style_1(d)
    }
}
impl StyleBuilderApplicable for Style {
    fn style_apply(&self, style: &StyleBuilder) {
        style.merge(self);
    }
}
impl std::fmt::Debug for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleImpl")
            .field("color", &self.color)
            .field("shadowColor", &self.shadow_color)
            .field("decorations", &self.decorations)
            .field("clickEvent", &self.click_event)
            .field("hoverEvent", &self.hover_event)
            .field("insertion", &self.insertion)
            .field("font", &self.font)
            .finish()
    }
}

// ---- interface net.kyori.adventure.text.format.Style.Builder (StyleImpl.BuilderImpl)
/// A mutable builder handle: clones share state, every method returns the builder.
#[derive(Clone)]
pub struct StyleBuilder(Arc<Mut<Style>>);
impl StyleBuilder {
    pub fn font(&self, font: Option<JRef<dyn Key>>) -> Self {
        self.0.borrow_mut().font = font;
        self.clone()
    }
    pub fn color(&self, color: Option<TextColor>) -> Self {
        self.0.borrow_mut().color = color;
        self.clone()
    }
    pub fn color_if_absent(&self, color: Option<TextColor>) -> Self {
        if self.0.borrow().color.is_none() {
            self.0.borrow_mut().color = color;
        }
        self.clone()
    }
    pub fn shadow_color(&self, argb: Option<ShadowColor>) -> Self {
        self.0.borrow_mut().shadow_color = argb;
        self.clone()
    }
    pub fn shadow_color_if_absent(&self, argb: Option<ShadowColor>) -> Self {
        if self.0.borrow().shadow_color.is_none() {
            self.0.borrow_mut().shadow_color = argb;
        }
        self.clone()
    }
    /// `decorate(TextDecoration)`.
    pub fn decorate(&self, decoration: TextDecoration) -> Self {
        self.decoration(decoration, TextDecorationState::TRUE)
    }
    /// `decorate(TextDecoration...)`.
    pub fn decorate_1(&self, decorations: &[TextDecoration]) -> Self {
        for d in decorations {
            self.decorate(*d);
        }
        self.clone()
    }
    /// `decoration(TextDecoration, boolean)` / `decoration(TextDecoration, State)`.
    pub fn decoration(&self, decoration: TextDecoration, state: impl Into<TextDecorationState>) -> Self {
        let state = state.into();
        let d = self.0.borrow().decorations.with(decoration, state);
        self.0.borrow_mut().decorations = d;
        self.clone()
    }
    /// `decorations(Map<TextDecoration, State>)` (StyleSetter default).
    pub fn decorations(&self, decorations: &IndexMap<TextDecoration, TextDecorationState>) -> Self {
        for (d, s) in decorations {
            self.decoration(*d, *s);
        }
        self.clone()
    }
    pub fn decoration_if_absent(&self, decoration: TextDecoration, state: TextDecorationState) -> Self {
        let old_state = self.0.borrow().decorations.get(decoration);
        if old_state == TextDecorationState::NOT_SET {
            self.decoration(decoration, state);
        }
        self.clone()
    }
    pub fn click_event(&self, event: Option<ClickEvent>) -> Self {
        self.0.borrow_mut().click_event = event;
        self.clone()
    }
    /// `hoverEvent(HoverEventSource<?>)` (erased event).
    pub fn hover_event(&self, source: Option<HoverEvent>) -> Self {
        self.0.borrow_mut().hover_event = source;
        self.clone()
    }
    pub fn insertion(&self, insertion: Option<String>) -> Self {
        self.0.borrow_mut().insertion = insertion;
        self.clone()
    }
    /// `merge(Style)`.
    pub fn merge(&self, that: &Style) -> Self {
        self.merge_3(that, StyleMergeStrategy::ALWAYS, &StyleMerge::all())
    }
    /// `merge(Style, Merge.Strategy)`.
    pub fn merge_2(&self, that: &Style, strategy: StyleMergeStrategy) -> Self {
        self.merge_3(that, strategy, &StyleMerge::all())
    }
    /// `merge(Style, Merge...)`.
    pub fn merge_2_merges(&self, that: &Style, merges: &[StyleMerge]) -> Self {
        self.merge_3(that, StyleMergeStrategy::ALWAYS, merges)
    }
    /// `merge(Style, Merge.Strategy, Set<Merge>)`.
    pub fn merge_3(&self, that: &Style, strategy: StyleMergeStrategy, merges: &[StyleMerge]) -> Self {
        if nothing_to_merge(that, strategy, merges) {
            return self.clone();
        }
        let always = strategy == StyleMergeStrategy::ALWAYS;
        let if_absent = strategy == StyleMergeStrategy::IF_ABSENT_ON_TARGET;
        if merges.contains(&StyleMerge::COLOR) {
            if let Some(color) = that.color() {
                if always || (if_absent && self.0.borrow().color.is_none()) {
                    self.color(Some(color));
                }
            }
        }
        if merges.contains(&StyleMerge::SHADOW_COLOR) {
            if let Some(shadow_color) = that.shadow_color() {
                if always || (if_absent && self.0.borrow().shadow_color.is_none()) {
                    self.shadow_color(Some(shadow_color));
                }
            }
        }
        if merges.contains(&StyleMerge::DECORATIONS) {
            for decoration in DECORATIONS {
                let state = that.decoration(decoration);
                if state != TextDecorationState::NOT_SET {
                    if always {
                        self.decoration(decoration, state);
                    } else if if_absent {
                        self.decoration_if_absent(decoration, state);
                    }
                }
            }
        }
        if merges.contains(&StyleMerge::EVENTS) {
            if let Some(click_event) = that.click_event() {
                if always || (if_absent && self.0.borrow().click_event.is_none()) {
                    self.click_event(Some(click_event));
                }
            }
            if let Some(hover_event) = that.hover_event() {
                if always || (if_absent && self.0.borrow().hover_event.is_none()) {
                    self.hover_event(Some(hover_event));
                }
            }
        }
        if merges.contains(&StyleMerge::INSERTION) {
            if let Some(insertion) = that.insertion() {
                if always || (if_absent && self.0.borrow().insertion.is_none()) {
                    self.insertion(Some(insertion));
                }
            }
        }
        if merges.contains(&StyleMerge::FONT) {
            if let Some(font) = that.font() {
                if always || (if_absent && self.0.borrow().font.is_none()) {
                    self.font(Some(font));
                }
            }
        }
        self.clone()
    }
    /// `apply(StyleBuilderApplicable)`.
    pub fn apply(&self, applicable: &dyn StyleBuilderApplicable) -> Self {
        applicable.style_apply(self);
        self.clone()
    }
    pub fn build(&self) -> Style {
        self.0.get()
    }
}

// ---- enum net.kyori.adventure.text.format.Style.Merge
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum StyleMerge {
    COLOR,
    SHADOW_COLOR,
    DECORATIONS,
    EVENTS,
    INSERTION,
    FONT,
}
impl StyleMerge {
    pub const VALUES: [StyleMerge; 6] =
        [StyleMerge::COLOR, StyleMerge::SHADOW_COLOR, StyleMerge::DECORATIONS, StyleMerge::EVENTS, StyleMerge::INSERTION, StyleMerge::FONT];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    /// `Merge.all()`.
    pub fn all() -> Vec<StyleMerge> {
        StyleMerge::VALUES.to_vec()
    }
    /// `Merge.colorAndDecorations()`.
    pub fn color_and_decorations() -> Vec<StyleMerge> {
        vec![StyleMerge::COLOR, StyleMerge::DECORATIONS]
    }
    /// `Merge.merges(Merge...)` / `Merge.of(Merge...)`.
    pub fn merges(merges: &[StyleMerge]) -> Vec<StyleMerge> {
        let mut v: Vec<StyleMerge> = merges.to_vec();
        v.sort();
        v.dedup();
        v
    }
    pub fn has_all(merges: &[StyleMerge]) -> bool {
        StyleMerge::VALUES.iter().all(|m| merges.contains(m))
    }
}

// ---- enum net.kyori.adventure.text.format.Style.Merge.Strategy
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum StyleMergeStrategy {
    ALWAYS,
    NEVER,
    IF_ABSENT_ON_TARGET,
}
