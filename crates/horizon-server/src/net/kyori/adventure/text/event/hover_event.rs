//! Port of `net.kyori.adventure.text.event.HoverEvent` (with `ShowItem`, `ShowEntity`, `Action`).
//!
//! `HoverEvent<V>` keeps Java's type parameter (`HoverEvent<HoverEventShowItem>`...). A style stores the
//! erased form, `HoverEvent` (= `HoverEvent<HoverEventValue>`): `erase()` converts, and
//! `value_as::<V>()` gets the typed value back.
use crate::prelude::*;
use crate::net::kyori::adventure::key::Key;
use crate::net::kyori::adventure::nbt::api::BinaryTagHolder;
use crate::net::kyori::adventure::text::Component;
use crate::net::kyori::adventure::text::event::{DataComponentValue, HoverEventSource};
use crate::net::kyori::adventure::text::format::{StyleBuilder, StyleBuilderApplicable};
use crate::net::kyori::adventure::util::Index;

/// The value of an erased hover event (Java's `HoverEvent<?>` value).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum HoverEventValue {
    Text(Component),
    Item(HoverEventShowItem),
    Entity(HoverEventShowEntity),
    Achievement(String),
}

/// The Java types a hover event's value can have (`Component`, `ShowItem`, `ShowEntity`, `String`).
pub trait HoverEventValueType: Clone + Send + Sync + 'static {
    fn into_value(self) -> HoverEventValue;
    fn from_value(value: HoverEventValue) -> Option<Self>;
}
impl HoverEventValueType for HoverEventValue {
    fn into_value(self) -> HoverEventValue {
        self
    }
    fn from_value(value: HoverEventValue) -> Option<Self> {
        Some(value)
    }
}
impl HoverEventValueType for Component {
    fn into_value(self) -> HoverEventValue {
        HoverEventValue::Text(self)
    }
    fn from_value(value: HoverEventValue) -> Option<Self> {
        if let HoverEventValue::Text(c) = value { Some(c) } else { None }
    }
}
impl HoverEventValueType for HoverEventShowItem {
    fn into_value(self) -> HoverEventValue {
        HoverEventValue::Item(self)
    }
    fn from_value(value: HoverEventValue) -> Option<Self> {
        if let HoverEventValue::Item(c) = value { Some(c) } else { None }
    }
}
impl HoverEventValueType for HoverEventShowEntity {
    fn into_value(self) -> HoverEventValue {
        HoverEventValue::Entity(self)
    }
    fn from_value(value: HoverEventValue) -> Option<Self> {
        if let HoverEventValue::Entity(c) = value { Some(c) } else { None }
    }
}
impl HoverEventValueType for String {
    fn into_value(self) -> HoverEventValue {
        HoverEventValue::Achievement(self)
    }
    fn from_value(value: HoverEventValue) -> Option<Self> {
        if let HoverEventValue::Achievement(c) = value { Some(c) } else { None }
    }
}

// ---- class net.kyori.adventure.text.event.HoverEvent<V>
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct HoverEvent<V = HoverEventValue> {
    action: HoverEventAction,
    value: V,
}
impl HoverEvent<Component> {
    /// `showText(ComponentLike)` / `showText(Component)`.
    pub fn show_text(text: impl Into<Component>) -> HoverEvent<Component> {
        HoverEvent { action: HoverEventAction::SHOW_TEXT, value: text.into() }
    }
}
impl HoverEvent<HoverEventShowItem> {
    /// `showItem(Key, int)` (`showItem(Keyed, int)`: pass `keyed.key()`).
    pub fn show_item_2(item: JRef<dyn Key>, count: i32) -> HoverEvent<HoverEventShowItem> {
        HoverEvent::show_item(HoverEventShowItem::show_item_2(item, count))
    }
    /// `showItem(Key, int, BinaryTagHolder)`.
    pub fn show_item_3(item: JRef<dyn Key>, count: i32, nbt: Option<BinaryTagHolder>) -> HoverEvent<HoverEventShowItem> {
        HoverEvent::show_item(HoverEventShowItem::show_item_3(item, count, nbt))
    }
    /// `showItem(Keyed, int, Map<Key, DataComponentValue>)`.
    pub fn show_item_3_2(item: JRef<dyn Key>, count: i32, data_components: HashMap<JRef<dyn Key>, DataComponentValue>) -> HoverEvent<HoverEventShowItem> {
        HoverEvent::show_item(HoverEventShowItem::show_item_3_2(item, count, data_components))
    }
    /// `showItem(ShowItem)`.
    pub fn show_item(item: HoverEventShowItem) -> HoverEvent<HoverEventShowItem> {
        HoverEvent { action: HoverEventAction::SHOW_ITEM, value: item }
    }
}
impl HoverEvent<HoverEventShowEntity> {
    /// `showEntity(Key, UUID)`.
    pub fn show_entity_2(type_: JRef<dyn Key>, id: Uuid) -> HoverEvent<HoverEventShowEntity> {
        HoverEvent::show_entity(HoverEventShowEntity::show_entity_2(type_, id))
    }
    /// `showEntity(Key, UUID, Component)`.
    pub fn show_entity_3(type_: JRef<dyn Key>, id: Uuid, name: Option<Component>) -> HoverEvent<HoverEventShowEntity> {
        HoverEvent::show_entity(HoverEventShowEntity::show_entity_3(type_, id, name))
    }
    /// `showEntity(ShowEntity)`.
    pub fn show_entity(entity: HoverEventShowEntity) -> HoverEvent<HoverEventShowEntity> {
        HoverEvent { action: HoverEventAction::SHOW_ENTITY, value: entity }
    }
}
impl HoverEvent<String> {
    pub fn show_achievement(value: &str) -> HoverEvent<String> {
        HoverEvent { action: HoverEventAction::SHOW_ACHIEVEMENT, value: value.to_string() }
    }
}
impl<V: HoverEventValueType> HoverEvent<V> {
    /// `hoverEvent(Action<V>, V)`.
    pub fn hover_event(action: HoverEventAction, value: V) -> HoverEvent<V> {
        HoverEvent { action, value }
    }
    pub fn action(&self) -> HoverEventAction {
        self.action
    }
    pub fn value(&self) -> V {
        self.value.clone()
    }
    /// `value(V)`.
    pub fn value_1(&self, value: V) -> HoverEvent<V> {
        HoverEvent { action: self.action, value }
    }
    /// The erased form (`HoverEvent<?>`).
    pub fn erase(&self) -> HoverEvent {
        HoverEvent { action: self.action, value: self.value.clone().into_value() }
    }
    /// `withRenderedValue(ComponentRenderer<C>, C)`: renders a text value, or a show-entity's name.
    pub fn with_rendered_value(&self, render: &dyn Fn(&Component) -> Component) -> HoverEvent<V> {
        let value = match self.value.clone().into_value() {
            HoverEventValue::Text(c) => HoverEventValue::Text(render(&c)),
            HoverEventValue::Entity(e) => match &e.name {
                Some(n) => HoverEventValue::Entity(e.name_1(Some(render(n)))),
                None => HoverEventValue::Entity(e),
            },
            other => other,
        };
        HoverEvent { action: self.action, value: V::from_value(value).expect("same value type") }
    }
}
impl HoverEvent {
    /// The value as `V` (`(V) event.value()` after checking `action()`).
    pub fn value_as<V: HoverEventValueType>(&self) -> Option<V> {
        V::from_value(self.value.clone())
    }
}
impl<V: HoverEventValueType> HoverEventSource<V> for HoverEvent<V> {
    fn as_hover_event_1(&self, op: &dyn Fn(V) -> V) -> HoverEvent<V> {
        HoverEvent { action: self.action, value: op(self.value.clone()) }
    }
}
impl<V: HoverEventValueType> StyleBuilderApplicable for HoverEvent<V> {
    fn style_apply(&self, style: &StyleBuilder) {
        style.hover_event(Some(self.erase()));
    }
}
impl From<HoverEvent<Component>> for HoverEvent {
    fn from(e: HoverEvent<Component>) -> Self {
        e.erase()
    }
}
impl From<HoverEvent<HoverEventShowItem>> for HoverEvent {
    fn from(e: HoverEvent<HoverEventShowItem>) -> Self {
        e.erase()
    }
}
impl From<HoverEvent<HoverEventShowEntity>> for HoverEvent {
    fn from(e: HoverEvent<HoverEventShowEntity>) -> Self {
        e.erase()
    }
}
impl From<HoverEvent<String>> for HoverEvent {
    fn from(e: HoverEvent<String>) -> Self {
        e.erase()
    }
}

// ---- class net.kyori.adventure.text.event.HoverEvent.ShowItem
#[derive(Clone, Debug)]
pub struct HoverEventShowItem {
    item: JRef<dyn Key>,
    count: i32,
    nbt: Option<BinaryTagHolder>,
    data_components: IndexMap<JRef<dyn Key>, DataComponentValue>,
}
impl HoverEventShowItem {
    /// `ShowItem.showItem(Key, int)`.
    pub fn show_item_2(item: JRef<dyn Key>, count: i32) -> HoverEventShowItem {
        HoverEventShowItem::show_item_3(item, count, None)
    }
    pub fn of(item: JRef<dyn Key>, count: i32) -> HoverEventShowItem {
        HoverEventShowItem::show_item_2(item, count)
    }
    /// `ShowItem.showItem(Key, int, BinaryTagHolder)`.
    pub fn show_item_3(item: JRef<dyn Key>, count: i32, nbt: Option<BinaryTagHolder>) -> HoverEventShowItem {
        HoverEventShowItem { item, count, nbt, data_components: IndexMap::new() }
    }
    pub fn of_3(item: JRef<dyn Key>, count: i32, nbt: Option<BinaryTagHolder>) -> HoverEventShowItem {
        HoverEventShowItem::show_item_3(item, count, nbt)
    }
    /// `ShowItem.showItem(Keyed, int, Map<Key, ? extends DataComponentValue>)`.
    pub fn show_item_3_2(item: JRef<dyn Key>, count: i32, data_components: HashMap<JRef<dyn Key>, DataComponentValue>) -> HoverEventShowItem {
        HoverEventShowItem { item, count, nbt: None, data_components: data_components.into_iter().collect() }
    }
    pub fn item(&self) -> JRef<dyn Key> {
        self.item.clone()
    }
    pub fn item_1(&self, item: JRef<dyn Key>) -> HoverEventShowItem {
        if *item == *self.item {
            return self.clone();
        }
        HoverEventShowItem { item, ..self.clone() }
    }
    pub fn count(&self) -> i32 {
        self.count
    }
    pub fn count_1(&self, count: i32) -> HoverEventShowItem {
        if count == self.count {
            return self.clone();
        }
        HoverEventShowItem { count, ..self.clone() }
    }
    /// `nbt()` (deprecated: before data components).
    pub fn nbt(&self) -> Option<BinaryTagHolder> {
        self.nbt.clone()
    }
    pub fn nbt_1(&self, nbt: Option<BinaryTagHolder>) -> HoverEventShowItem {
        if nbt == self.nbt {
            return self.clone();
        }
        HoverEventShowItem { nbt, ..self.clone() }
    }
    pub fn data_components(&self) -> HashMap<JRef<dyn Key>, DataComponentValue> {
        self.data_components.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    pub fn data_components_1(&self, holder: HashMap<JRef<dyn Key>, DataComponentValue>) -> HoverEventShowItem {
        HoverEventShowItem { data_components: holder.into_iter().collect(), ..self.clone() }
    }
    /// `dataComponentsAs(Class<V>)`: each value converted to `V` by the converter registry.
    pub fn data_components_as<V: ?Sized + 'static>(&self) -> HashMap<JRef<dyn Key>, DataComponentValue> {
        self.data_components
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    crate::net::kyori::adventure::text::event::DataComponentValueConverterRegistry::convert::<V>(k.clone(), v.clone()),
                )
            })
            .collect()
    }
}
impl PartialEq for HoverEventShowItem {
    fn eq(&self, other: &Self) -> bool {
        *self.item == *other.item && self.count == other.count && self.nbt == other.nbt && self.data_components == other.data_components
    }
}
impl Eq for HoverEventShowItem {}
impl std::hash::Hash for HoverEventShowItem {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.item.hash(state);
        self.count.hash(state);
        self.nbt.hash(state);
        self.data_components.len().hash(state);
    }
}

// ---- class net.kyori.adventure.text.event.HoverEvent.ShowEntity
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct HoverEventShowEntity {
    type_: JRef<dyn Key>,
    id: Uuid,
    name: Option<Component>,
}
impl HoverEventShowEntity {
    /// `ShowEntity.showEntity(Key, UUID)`.
    pub fn show_entity_2(type_: JRef<dyn Key>, id: Uuid) -> HoverEventShowEntity {
        HoverEventShowEntity::show_entity_3(type_, id, None)
    }
    pub fn of_2(type_: JRef<dyn Key>, id: Uuid) -> HoverEventShowEntity {
        HoverEventShowEntity::show_entity_2(type_, id)
    }
    /// `ShowEntity.showEntity(Key, UUID, Component)`.
    pub fn show_entity_3(type_: JRef<dyn Key>, id: Uuid, name: Option<Component>) -> HoverEventShowEntity {
        HoverEventShowEntity { type_, id, name }
    }
    pub fn of_3(type_: JRef<dyn Key>, id: Uuid, name: Option<Component>) -> HoverEventShowEntity {
        HoverEventShowEntity::show_entity_3(type_, id, name)
    }
    pub fn type_(&self) -> JRef<dyn Key> {
        self.type_.clone()
    }
    pub fn type_1(&self, type_: JRef<dyn Key>) -> HoverEventShowEntity {
        if *type_ == *self.type_ {
            return self.clone();
        }
        HoverEventShowEntity { type_, ..self.clone() }
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn id_1(&self, id: Uuid) -> HoverEventShowEntity {
        if id == self.id {
            return self.clone();
        }
        HoverEventShowEntity { id, ..self.clone() }
    }
    pub fn name(&self) -> Option<Component> {
        self.name.clone()
    }
    pub fn name_1(&self, name: Option<Component>) -> HoverEventShowEntity {
        if name == self.name {
            return self.clone();
        }
        HoverEventShowEntity { name, ..self.clone() }
    }
}

// ---- class net.kyori.adventure.text.event.HoverEvent.Action<V>
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HoverEventAction {
    SHOW_TEXT,
    SHOW_ITEM,
    SHOW_ENTITY,
    SHOW_ACHIEVEMENT,
}
impl HoverEventAction {
    pub const VALUES: [HoverEventAction; 4] =
        [HoverEventAction::SHOW_TEXT, HoverEventAction::SHOW_ITEM, HoverEventAction::SHOW_ENTITY, HoverEventAction::SHOW_ACHIEVEMENT];
    /// The action's name (`toString()`, `NAMES` key).
    pub fn to_string(self) -> &'static str {
        match self {
            HoverEventAction::SHOW_TEXT => "show_text",
            HoverEventAction::SHOW_ITEM => "show_item",
            HoverEventAction::SHOW_ENTITY => "show_entity",
            HoverEventAction::SHOW_ACHIEVEMENT => "show_achievement",
        }
    }
    /// `readable()`: false only for the removed `show_achievement`.
    pub fn readable(self) -> bool {
        self != HoverEventAction::SHOW_ACHIEVEMENT
    }
}
impl std::fmt::Display for HoverEventAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_string())
    }
}
/// `HoverEvent.Action.NAMES`.
pub static ACTION_NAMES: LazyLock<Index<String, HoverEventAction>> =
    LazyLock::new(|| Index::create(HoverEventAction::VALUES.to_vec(), |a: &HoverEventAction| a.to_string().to_string()));
