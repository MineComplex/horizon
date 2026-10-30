//! Port of `net.kyori.adventure.text.event.ClickEvent` (with `Payload`, `Action`).
use crate::prelude::*;
use crate::net::kyori::adventure::audience::Audience;
use crate::net::kyori::adventure::dialog::DialogLike;
use crate::net::kyori::adventure::key::Key;
use crate::net::kyori::adventure::nbt::api::BinaryTagHolder;
use crate::net::kyori::adventure::text::event::{ClickCallback, ClickCallbackOptions, ClickCallbackOptionsBuilder};
use crate::net::kyori::adventure::text::format::{StyleBuilder, StyleBuilderApplicable};
use crate::net::kyori::adventure::util::Index;

// ---- class net.kyori.adventure.text.event.ClickEvent
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ClickEvent {
    action: ClickEventAction,
    payload: ClickEventPayload,
}
impl ClickEvent {
    pub fn open_url(url: &str) -> ClickEvent {
        ClickEvent::click_event(ClickEventAction::OPEN_URL, url)
    }
    pub fn open_file(file: &str) -> ClickEvent {
        ClickEvent::click_event(ClickEventAction::OPEN_FILE, file)
    }
    pub fn run_command(command: &str) -> ClickEvent {
        ClickEvent::click_event(ClickEventAction::RUN_COMMAND, command)
    }
    pub fn suggest_command(command: &str) -> ClickEvent {
        ClickEvent::click_event(ClickEventAction::SUGGEST_COMMAND, command)
    }
    /// `changePage(String)`.
    pub fn change_page(page: &str) -> ClickEvent {
        let page: i32 = page.trim().parse().unwrap_or_else(|_| panic!("java.lang.NumberFormatException: For input string: \"{}\"", page));
        ClickEvent::change_page_1(page)
    }
    /// `changePage(int)`.
    pub fn change_page_1(page: i32) -> ClickEvent {
        ClickEvent::click_event_2(ClickEventAction::CHANGE_PAGE, ClickEventPayload::integer(page))
    }
    pub fn copy_to_clipboard(text: &str) -> ClickEvent {
        ClickEvent::click_event(ClickEventAction::COPY_TO_CLIPBOARD, text)
    }
    /// `callback(ClickCallback<Audience>)`.
    pub fn callback(function: ClickCallback<JRef<dyn Audience>>) -> ClickEvent {
        ClickEvent::callback_2(function, ClickCallbackOptions::builder().build())
    }
    /// `callback(ClickCallback<Audience>, ClickCallback.Options)`: made by the platform's provider.
    pub fn callback_2(function: ClickCallback<JRef<dyn Audience>>, options: ClickCallbackOptions) -> ClickEvent {
        crate::net::kyori::adventure::text::event::click_callback::provider().create(function, options)
    }
    /// `callback(ClickCallback<Audience>, Consumer<ClickCallback.Options.Builder>)`.
    pub fn callback_2_2(function: ClickCallback<JRef<dyn Audience>>, options_builder: impl FnOnce(&ClickCallbackOptionsBuilder)) -> ClickEvent {
        let b = ClickCallbackOptions::builder();
        options_builder(&b);
        ClickEvent::callback_2(function, b.build())
    }
    pub fn show_dialog(dialog: JRef<dyn DialogLike>) -> ClickEvent {
        ClickEvent::click_event_2(ClickEventAction::SHOW_DIALOG, ClickEventPayload::dialog(dialog))
    }
    /// `custom(Key, String)`.
    pub fn custom(key: JRef<dyn Key>, data: &str) -> ClickEvent {
        ClickEvent::click_event_2(ClickEventAction::CUSTOM, ClickEventPayload::custom(key, data))
    }
    /// `custom(Key, BinaryTagHolder)`.
    pub fn custom_2(key: JRef<dyn Key>, nbt: BinaryTagHolder) -> ClickEvent {
        ClickEvent::click_event_2(ClickEventAction::CUSTOM, ClickEventPayload::custom_2(key, nbt))
    }
    /// `clickEvent(Action, String)`.
    pub fn click_event(action: ClickEventAction, value: &str) -> ClickEvent {
        if action == ClickEventAction::CHANGE_PAGE {
            return ClickEvent::change_page(value);
        }
        if !action.supports(&ClickEventPayload::string("")) {
            panic!("java.lang.IllegalArgumentException: Action {} does not support string payloads", action);
        }
        ClickEvent { action, payload: ClickEventPayload::string(value) }
    }
    /// `clickEvent(Action, Payload)`.
    pub fn click_event_2(action: ClickEventAction, payload: ClickEventPayload) -> ClickEvent {
        if !action.supports(&payload) {
            panic!("java.lang.IllegalArgumentException: Action {} does not support payload {:?}", action, payload);
        }
        ClickEvent { action, payload }
    }
    pub fn action(&self) -> ClickEventAction {
        self.action
    }
    /// `value()` (deprecated): the payload as a string.
    pub fn value(&self) -> String {
        match &self.payload {
            ClickEventPayload::Text(v) => v.clone(),
            ClickEventPayload::Int(i) => i.to_string(),
            ClickEventPayload::Custom { key, nbt } => format!("{}{}", key.as_string(), nbt.string()),
            ClickEventPayload::Dialog(_) => panic!("java.lang.UnsupportedOperationException: dialog payloads have no string value"),
        }
    }
    pub fn payload(&self) -> ClickEventPayload {
        self.payload.clone()
    }
}
impl StyleBuilderApplicable for ClickEvent {
    fn style_apply(&self, style: &StyleBuilder) {
        style.click_event(Some(self.clone()));
    }
}

// ---- interface net.kyori.adventure.text.event.ClickEvent.Payload (sealed: Text, Int, Dialog, Custom)
#[derive(Clone)]
pub enum ClickEventPayload {
    Text(String),
    Int(i32),
    Dialog(JRef<dyn DialogLike>),
    Custom { key: JRef<dyn Key>, nbt: BinaryTagHolder },
}
impl ClickEventPayload {
    /// `Payload.string(String)`.
    pub fn string(value: &str) -> ClickEventPayload {
        ClickEventPayload::Text(value.to_string())
    }
    pub fn integer(integer: i32) -> ClickEventPayload {
        ClickEventPayload::Int(integer)
    }
    pub fn dialog(dialog: JRef<dyn DialogLike>) -> ClickEventPayload {
        ClickEventPayload::Dialog(dialog)
    }
    /// `Payload.custom(Key, String)`: the string as an SNBT string tag.
    pub fn custom(key: JRef<dyn Key>, data: &str) -> ClickEventPayload {
        ClickEventPayload::Custom { key, nbt: BinaryTagHolder::binary_tag_holder(&snbt_quote(data)) }
    }
    pub fn custom_2(key: JRef<dyn Key>, nbt: BinaryTagHolder) -> ClickEventPayload {
        ClickEventPayload::Custom { key, nbt }
    }
    /// `((Payload.Text) p).value()`.
    pub fn value(&self) -> String {
        match self {
            ClickEventPayload::Text(v) => v.clone(),
            _ => panic!("java.lang.ClassCastException: payload is not ClickEvent.Payload.Text"),
        }
    }
    /// `((Payload.Int) p).integer()`.
    pub fn integer_value(&self) -> i32 {
        match self {
            ClickEventPayload::Int(v) => *v,
            _ => panic!("java.lang.ClassCastException: payload is not ClickEvent.Payload.Int"),
        }
    }
    /// `((Payload.Dialog) p).dialog()`.
    pub fn dialog_value(&self) -> JRef<dyn DialogLike> {
        match self {
            ClickEventPayload::Dialog(d) => d.clone(),
            _ => panic!("java.lang.ClassCastException: payload is not ClickEvent.Payload.Dialog"),
        }
    }
    /// `((Payload.Custom) p).key()`.
    pub fn key(&self) -> JRef<dyn Key> {
        match self {
            ClickEventPayload::Custom { key, .. } => key.clone(),
            _ => panic!("java.lang.ClassCastException: payload is not ClickEvent.Payload.Custom"),
        }
    }
    /// `((Payload.Custom) p).nbt()`.
    pub fn nbt(&self) -> BinaryTagHolder {
        match self {
            ClickEventPayload::Custom { nbt, .. } => nbt.clone(),
            _ => panic!("java.lang.ClassCastException: payload is not ClickEvent.Payload.Custom"),
        }
    }
    pub fn is_text(&self) -> bool {
        matches!(self, ClickEventPayload::Text(_))
    }
    pub fn is_int(&self) -> bool {
        matches!(self, ClickEventPayload::Int(_))
    }
    pub fn is_dialog(&self) -> bool {
        matches!(self, ClickEventPayload::Dialog(_))
    }
    pub fn is_custom(&self) -> bool {
        matches!(self, ClickEventPayload::Custom { .. })
    }
}
/// A string as an SNBT string literal (what `StringBinaryTag.stringBinaryTag(data)` prints).
fn snbt_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}
impl PartialEq for ClickEventPayload {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ClickEventPayload::Text(a), ClickEventPayload::Text(b)) => a == b,
            (ClickEventPayload::Int(a), ClickEventPayload::Int(b)) => a == b,
            (ClickEventPayload::Dialog(a), ClickEventPayload::Dialog(b)) => same(a, b),
            (ClickEventPayload::Custom { key: ka, nbt: na }, ClickEventPayload::Custom { key: kb, nbt: nb }) => ka == kb && na == nb,
            _ => false,
        }
    }
}
impl Eq for ClickEventPayload {}
impl std::hash::Hash for ClickEventPayload {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            ClickEventPayload::Text(a) => a.hash(state),
            ClickEventPayload::Int(a) => a.hash(state),
            ClickEventPayload::Dialog(a) => (Arc::as_ptr(a) as *const () as usize).hash(state),
            ClickEventPayload::Custom { key, nbt } => {
                key.hash(state);
                nbt.hash(state)
            }
        }
    }
}
impl std::fmt::Debug for ClickEventPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClickEventPayload::Text(a) => write!(f, "Text({:?})", a),
            ClickEventPayload::Int(a) => write!(f, "Int({})", a),
            ClickEventPayload::Dialog(_) => write!(f, "Dialog"),
            ClickEventPayload::Custom { key, nbt } => write!(f, "Custom({}, {:?})", key.as_string(), nbt.string()),
        }
    }
}

// ---- enum net.kyori.adventure.text.event.ClickEvent.Action
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ClickEventAction {
    OPEN_URL,
    OPEN_FILE,
    RUN_COMMAND,
    SUGGEST_COMMAND,
    CHANGE_PAGE,
    COPY_TO_CLIPBOARD,
    SHOW_DIALOG,
    CUSTOM,
}
impl ClickEventAction {
    pub const VALUES: [ClickEventAction; 8] = [
        ClickEventAction::OPEN_URL,
        ClickEventAction::OPEN_FILE,
        ClickEventAction::RUN_COMMAND,
        ClickEventAction::SUGGEST_COMMAND,
        ClickEventAction::CHANGE_PAGE,
        ClickEventAction::COPY_TO_CLIPBOARD,
        ClickEventAction::SHOW_DIALOG,
        ClickEventAction::CUSTOM,
    ];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            ClickEventAction::OPEN_URL => "OPEN_URL",
            ClickEventAction::OPEN_FILE => "OPEN_FILE",
            ClickEventAction::RUN_COMMAND => "RUN_COMMAND",
            ClickEventAction::SUGGEST_COMMAND => "SUGGEST_COMMAND",
            ClickEventAction::CHANGE_PAGE => "CHANGE_PAGE",
            ClickEventAction::COPY_TO_CLIPBOARD => "COPY_TO_CLIPBOARD",
            ClickEventAction::SHOW_DIALOG => "SHOW_DIALOG",
            ClickEventAction::CUSTOM => "CUSTOM",
        }
    }
    pub fn value_of(name: &str) -> Option<ClickEventAction> {
        Self::VALUES.into_iter().find(|v| v.name() == name)
    }
    /// The action's lower-case name (`toString()`, `NAMES` key).
    pub fn to_string(self) -> &'static str {
        match self {
            ClickEventAction::OPEN_URL => "open_url",
            ClickEventAction::OPEN_FILE => "open_file",
            ClickEventAction::RUN_COMMAND => "run_command",
            ClickEventAction::SUGGEST_COMMAND => "suggest_command",
            ClickEventAction::CHANGE_PAGE => "change_page",
            ClickEventAction::COPY_TO_CLIPBOARD => "copy_to_clipboard",
            ClickEventAction::SHOW_DIALOG => "show_dialog",
            ClickEventAction::CUSTOM => "custom",
        }
    }
    /// `readable()`: whether the client may send it (all but `OPEN_FILE` and `SHOW_DIALOG`).
    pub fn readable(self) -> bool {
        !matches!(self, ClickEventAction::OPEN_FILE | ClickEventAction::SHOW_DIALOG)
    }
    pub fn supports(self, payload: &ClickEventPayload) -> bool {
        match self {
            ClickEventAction::CHANGE_PAGE => payload.is_int(),
            ClickEventAction::SHOW_DIALOG => payload.is_dialog(),
            ClickEventAction::CUSTOM => payload.is_custom(),
            _ => payload.is_text(),
        }
    }
}
impl std::fmt::Display for ClickEventAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_string())
    }
}
/// `ClickEvent.Action.NAMES`.
pub static ACTION_NAMES: LazyLock<Index<String, ClickEventAction>> =
    LazyLock::new(|| Index::create(ClickEventAction::VALUES.to_vec(), |a: &ClickEventAction| a.to_string().to_string()));
