//! Plugins: Bukkit-style server plugins (manifest `side = "server"`,
//! `api = "0.4"`, capability `plugin`; docs/plugins.md).
//!
//! A plugin is a type implementing [`Plugin`], exported with
//! [`export_plugin!`](crate::export_plugin). The server enables it once the
//! world is up, after the plugins it depends on:
//!
//! ```ignore
//! use horizon_mod_sdk::plugin::*;
//!
//! struct Hello;
//! impl Plugin for Hello {
//!     fn enable() -> Result<Self, String> {
//!         listen(EventKind::PlayerJoin, Priority::Normal);
//!         Ok(Hello)
//!     }
//!     fn on_event(&mut self, _priority: Priority, event: &mut Event) {
//!         if let Event::PlayerJoin(e) = event {
//!             e.message = Some(format!("§e{} is here!", e.player.name));
//!         }
//!     }
//!     fn on_command(&mut self, sender: &Sender, _cmd: &str, _label: &str, args: &[String]) -> bool {
//!         sender.send(&format!("§aHello, {}!", args.first().map_or("world", |a| a.as_str())));
//!         true
//!     }
//! }
//! horizon_mod_sdk::export_plugin!(Hello);
//! ```
//!
//! Commands and permissions are declared in `horizon-mod.toml` as in a
//! `plugin.yml`; [`commands::register`] adds more at run time.

wit_bindgen::generate!({
    path: "wit",
    world: "horizon-plugin",
    pub_export_macro: true,
    export_macro_name: "__export_horizon_plugin",
    default_bindings_module: "horizon_mod_sdk::plugin",
    additional_derives: [PartialEq],
    with: {
        "horizon:mods/log@0.1.0": crate::log,
        "horizon:mods/storage@0.1.0": crate::storage,
    },
});

pub use events::*;
pub use exports::horizon::mods::plugin::Guest as __Guest;
pub use horizon::mods::{
    plugin_bossbars as bossbars, plugin_commands as commands, plugin_entities as entities,
    plugin_event_types as events, plugin_files as files, plugin_inventories as inventories,
    plugin_items as items, plugin_listeners as listeners, plugin_messaging as messaging,
    plugin_permissions as permissions, plugin_players as players, plugin_scheduler as scheduler,
    plugin_scoreboards as scoreboards, plugin_server as server, plugin_types as types,
    plugin_worlds as worlds,
};
pub use types::{
    Block, BlockPos, EntityRef, GameMode, Hand, ItemStack, Location, Player, Sender, Vec3,
};

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

/// The server's calls into a plugin. Only `enable` is required.
pub trait Plugin: Sized + 'static {
    /// The server enables the plugin: register listeners and commands, load
    /// data. An error keeps it off and is logged.
    fn enable() -> Result<Self, String>;
    /// The server stops or reloads the plugin: save what must be kept.
    fn disable(&mut self) {}
    /// An event the plugin listens to ([`listen`]): change it in place, set
    /// its `cancelled` to cancel it.
    fn on_event(&mut self, _priority: Priority, _event: &mut Event) {}
    /// A command the plugin owns: `command` is its name, `label` the alias
    /// typed. Returning false shows its usage.
    fn on_command(
        &mut self,
        _sender: &Sender,
        _command: &str,
        _label: &str,
        _args: &[String],
    ) -> bool {
        false
    }
    /// Completions for the last of `args`. An empty list offers the names
    /// of the players online.
    fn on_tab_complete(
        &mut self,
        _sender: &Sender,
        _command: &str,
        _args: &[String],
    ) -> Vec<String> {
        Vec::new()
    }
    /// A task scheduled with [`scheduler::run_later`] is due (tasks made
    /// with [`later`] and [`timer`] run their closure instead).
    fn on_task(&mut self, _task: u32) {}
}

/// Hear `kind` at `priority`, cancelled events included.
pub fn listen(kind: EventKind, priority: Priority) {
    listeners::listen(kind, priority, false);
}
/// Hear `kind` at `priority`, but not once a listener before cancelled it.
pub fn listen_uncancelled(kind: EventKind, priority: Priority) {
    listeners::listen(kind, priority, true);
}

// ---------------------------------------------------------------------------
// Events.

impl Event {
    /// The event's kind, as `listen` names it.
    pub fn kind(&self) -> EventKind {
        match self {
            Event::PlayerJoin(_) => EventKind::PlayerJoin,
            Event::PlayerQuit(_) => EventKind::PlayerQuit,
            Event::PlayerLogin(_) => EventKind::PlayerLogin,
            Event::PlayerChat(_) => EventKind::PlayerChat,
            Event::PlayerCommand(_) => EventKind::PlayerCommand,
            Event::PlayerMove(_) => EventKind::PlayerMove,
            Event::PlayerInteract(_) => EventKind::PlayerInteract,
            Event::PlayerInteractEntity(_) => EventKind::PlayerInteractEntity,
            Event::PlayerToggleSneak(_) => EventKind::PlayerToggleSneak,
            Event::PlayerToggleSprint(_) => EventKind::PlayerToggleSprint,
            Event::PlayerToggleFlight(_) => EventKind::PlayerToggleFlight,
            Event::PlayerItemHeld(_) => EventKind::PlayerItemHeld,
            Event::PlayerDropItem(_) => EventKind::PlayerDropItem,
            Event::PlayerSwapHands(_) => EventKind::PlayerSwapHands,
            Event::PlayerRespawn(_) => EventKind::PlayerRespawn,
            Event::PlayerDeath(_) => EventKind::PlayerDeath,
            Event::PlayerGameModeChange(_) => EventKind::PlayerGameModeChange,
            Event::PlayerChangedWorld(_) => EventKind::PlayerChangedWorld,
            Event::SignChange(_) => EventKind::SignChange,
            Event::PluginMessage(_) => EventKind::PluginMessage,
            Event::PlayerResourcePack(_) => EventKind::PlayerResourcePack,
            Event::BlockBreak(_) => EventKind::BlockBreak,
            Event::BlockPlace(_) => EventKind::BlockPlace,
            Event::BlockPistonExtend(_) => EventKind::BlockPistonExtend,
            Event::BlockPistonRetract(_) => EventKind::BlockPistonRetract,
            Event::BlockFromTo(_) => EventKind::BlockFromTo,
            Event::BlockGrow(_) => EventKind::BlockGrow,
            Event::BlockSpread(_) => EventKind::BlockSpread,
            Event::BlockForm(_) => EventKind::BlockForm,
            Event::BlockFade(_) => EventKind::BlockFade,
            Event::BlockFertilize(_) => EventKind::BlockFertilize,
            Event::EntityDamage(_) => EventKind::EntityDamage,
            Event::EntityDeath(_) => EventKind::EntityDeath,
            Event::EntitySpawn(_) => EventKind::EntitySpawn,
            Event::EntityExplode(_) => EventKind::EntityExplode,
            Event::InventoryClick(_) => EventKind::InventoryClick,
            Event::InventoryClose(_) => EventKind::InventoryClose,
            Event::ServerCommand(_) => EventKind::ServerCommand,
            Event::PluginState(_) => EventKind::PluginState,
            Event::PluginCall(_) => EventKind::PluginCall,
        }
    }
    /// Whether a listener cancelled it (a refused login counts); false for
    /// the kinds that can't be.
    pub fn is_cancelled(&self) -> bool {
        match self {
            Event::PlayerLogin(e) => !e.allowed,
            _ => self.cancelled_flag().is_some_and(|c| c),
        }
    }
    /// Cancels it, or takes the cancellation back (Bukkit's
    /// `setCancelled`); false when the kind can't be cancelled.
    pub fn set_cancelled(&mut self, cancelled: bool) -> bool {
        let flag = match self {
            Event::PlayerChat(e) => &mut e.cancelled,
            Event::PlayerCommand(e) => &mut e.cancelled,
            Event::PlayerMove(e) => &mut e.cancelled,
            Event::PlayerInteract(e) => &mut e.cancelled,
            Event::PlayerInteractEntity(e) => &mut e.cancelled,
            Event::PlayerToggleSneak(e) => &mut e.cancelled,
            Event::PlayerToggleSprint(e) => &mut e.cancelled,
            Event::PlayerToggleFlight(e) => &mut e.cancelled,
            Event::PlayerItemHeld(e) => &mut e.cancelled,
            Event::PlayerDropItem(e) => &mut e.cancelled,
            Event::PlayerSwapHands(e) => &mut e.cancelled,
            Event::PlayerGameModeChange(e) => &mut e.cancelled,
            Event::SignChange(e) => &mut e.cancelled,
            Event::BlockBreak(e) => &mut e.cancelled,
            Event::BlockPlace(e) => &mut e.cancelled,
            Event::BlockPistonExtend(e) | Event::BlockPistonRetract(e) => &mut e.cancelled,
            Event::BlockFromTo(e) => &mut e.cancelled,
            Event::BlockGrow(e) | Event::BlockForm(e) | Event::BlockFade(e) => &mut e.cancelled,
            Event::BlockSpread(e) => &mut e.cancelled,
            Event::BlockFertilize(e) => &mut e.cancelled,
            Event::EntityDamage(e) => &mut e.cancelled,
            Event::EntitySpawn(e) => &mut e.cancelled,
            Event::EntityExplode(e) => &mut e.cancelled,
            Event::InventoryClick(e) => &mut e.cancelled,
            Event::ServerCommand(e) => &mut e.cancelled,
            Event::PlayerLogin(e) => {
                e.allowed = !cancelled;
                return true;
            }
            _ => return false,
        };
        *flag = cancelled;
        true
    }
    fn cancelled_flag(&self) -> Option<bool> {
        Some(match self {
            Event::PlayerChat(e) => e.cancelled,
            Event::PlayerCommand(e) => e.cancelled,
            Event::PlayerMove(e) => e.cancelled,
            Event::PlayerInteract(e) => e.cancelled,
            Event::PlayerInteractEntity(e) => e.cancelled,
            Event::PlayerToggleSneak(e) => e.cancelled,
            Event::PlayerToggleSprint(e) => e.cancelled,
            Event::PlayerToggleFlight(e) => e.cancelled,
            Event::PlayerItemHeld(e) => e.cancelled,
            Event::PlayerDropItem(e) => e.cancelled,
            Event::PlayerSwapHands(e) => e.cancelled,
            Event::PlayerGameModeChange(e) => e.cancelled,
            Event::SignChange(e) => e.cancelled,
            Event::BlockBreak(e) => e.cancelled,
            Event::BlockPlace(e) => e.cancelled,
            Event::BlockPistonExtend(e) | Event::BlockPistonRetract(e) => e.cancelled,
            Event::BlockFromTo(e) => e.cancelled,
            Event::BlockGrow(e) | Event::BlockForm(e) | Event::BlockFade(e) => e.cancelled,
            Event::BlockSpread(e) => e.cancelled,
            Event::BlockFertilize(e) => e.cancelled,
            Event::EntityDamage(e) => e.cancelled,
            Event::EntitySpawn(e) => e.cancelled,
            Event::EntityExplode(e) => e.cancelled,
            Event::InventoryClick(e) => e.cancelled,
            Event::ServerCommand(e) => e.cancelled,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tasks with closures.

type TaskFn = Box<dyn FnMut(&mut dyn Any)>;
thread_local! {
    static TASKS: RefCell<HashMap<u32, (TaskFn, bool)>> = RefCell::new(HashMap::new());
}

/// Runs `f` with the plugin after `delay` ticks (20 a second; 0: at the
/// end of this tick). Returns the task's id for [`scheduler::cancel`].
pub fn later<P: Plugin>(delay: u64, mut f: impl FnMut(&mut P) + 'static) -> u32 {
    let id = scheduler::run_later(delay, None);
    let run: TaskFn = Box::new(move |p| {
        if let Some(p) = p.downcast_mut::<P>() {
            f(p)
        }
    });
    TASKS.with(|t| t.borrow_mut().insert(id, (run, false)));
    id
}
/// Runs `f` with the plugin after `delay` ticks, then every `period` ticks
/// until [`cancel`].
pub fn timer<P: Plugin>(delay: u64, period: u64, mut f: impl FnMut(&mut P) + 'static) -> u32 {
    let id = scheduler::run_later(delay, Some(period.max(1)));
    let run: TaskFn = Box::new(move |p| {
        if let Some(p) = p.downcast_mut::<P>() {
            f(p)
        }
    });
    TASKS.with(|t| t.borrow_mut().insert(id, (run, true)));
    id
}
/// Cancels a task made with [`later`], [`timer`] or [`scheduler::run_later`].
pub fn cancel(task: u32) -> bool {
    TASKS.with(|t| t.borrow_mut().remove(&task));
    scheduler::cancel(task)
}
/// Runs task `id`'s closure; false when it has none (the plugin's own
/// `on_task` takes it then). Used by [`export_plugin!`](crate::export_plugin).
#[doc(hidden)]
pub fn __run_task(id: u32, plugin: &mut dyn Any) -> bool {
    let taken = TASKS.with(|t| t.borrow_mut().remove(&id));
    let Some((mut f, repeats)) = taken else {
        return false;
    };
    f(plugin);
    if repeats && scheduler::is_queued(id) {
        TASKS.with(|t| t.borrow_mut().insert(id, (f, repeats)));
    }
    true
}

// ---------------------------------------------------------------------------
// Text.

/// `&a` codes as `§a` (Bukkit's `translateAlternateColorCodes('&', ...)`).
pub fn color(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&'
            && chars
                .peek()
                .is_some_and(|n| "0123456789abcdefklmnorABCDEFKLMNOR".contains(*n))
        {
            out.push('§');
            out.push(chars.next().expect("peeked").to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
/// Text without its `§` codes.
pub fn strip_color(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Senders and players.

impl Sender {
    /// The console, or the player's name.
    pub fn name(&self) -> &str {
        match self {
            Sender::Console => "CONSOLE",
            Sender::Player(p) => &p.name,
        }
    }
    pub fn player(&self) -> Option<&Player> {
        match self {
            Sender::Console => None,
            Sender::Player(p) => Some(p),
        }
    }
    pub fn is_console(&self) -> bool {
        matches!(self, Sender::Console)
    }
    /// A message to the sender (text).
    pub fn send(&self, message: &str) {
        match self {
            Sender::Console => server::console(&strip_color(message)),
            Sender::Player(p) => players::send_message(&p.id, message),
        }
    }
    /// The console has every permission.
    pub fn has_permission(&self, node: &str) -> bool {
        match self {
            Sender::Console => true,
            Sender::Player(p) => permissions::has(&p.id, node),
        }
    }
}

impl Player {
    /// Fresh state of the player, if still online.
    pub fn refresh(&self) -> Option<Player> {
        players::get(&self.id)
    }
    pub fn send(&self, message: &str) {
        players::send_message(&self.id, message);
    }
    pub fn action_bar(&self, message: &str) {
        players::send_action_bar(&self.id, message);
    }
    /// A title with vanilla's timings (10, 70, 20 ticks).
    pub fn title(&self, title: &str, subtitle: &str) {
        players::send_title(&self.id, title, subtitle, 10, 70, 20);
    }
    pub fn has_permission(&self, node: &str) -> bool {
        permissions::has(&self.id, node)
    }
    pub fn teleport(&self, to: &Location) -> bool {
        players::teleport(&self.id, to)
    }
    pub fn kick(&self, reason: &str) -> bool {
        players::kick(&self.id, reason)
    }
    pub fn give(&self, items: &[ItemStack]) -> Vec<ItemStack> {
        players::give(&self.id, items)
    }
    pub fn set_game_mode(&self, mode: GameMode) {
        players::set_game_mode(&self.id, mode);
    }
    pub fn play_sound(&self, sound: &str, volume: f32, pitch: f32) {
        players::play_sound(&self.id, sound, "master", volume, pitch, None);
    }
    pub fn perform_command(&self, line: &str) {
        players::perform_command(&self.id, line);
    }
    /// The block position the player stands in.
    pub fn block_pos(&self) -> BlockPos {
        self.location.block_pos()
    }
}

impl Location {
    pub fn new(world: &str, x: f64, y: f64, z: f64) -> Location {
        Location {
            world: world.to_owned(),
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
    pub fn with_rotation(mut self, yaw: f32, pitch: f32) -> Location {
        self.yaw = yaw;
        self.pitch = pitch;
        self
    }
    pub fn block_pos(&self) -> BlockPos {
        BlockPos {
            x: self.x.floor() as i32,
            y: self.y.floor() as i32,
            z: self.z.floor() as i32,
        }
    }
    /// The distance to `other` (infinite in another world).
    pub fn distance(&self, other: &Location) -> f64 {
        if self.world != other.world {
            return f64::INFINITY;
        }
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
    /// The centre of the block below the location's feet, on its top.
    pub fn centered(&self) -> Location {
        Location {
            x: self.x.floor() + 0.5,
            z: self.z.floor() + 0.5,
            ..self.clone()
        }
    }
}

impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos { x, y, z }
    }
    pub fn offset(&self, dx: i32, dy: i32, dz: i32) -> BlockPos {
        BlockPos::new(self.x + dx, self.y + dy, self.z + dz)
    }
    /// The block's centre in `world`.
    pub fn center(&self, world: &str) -> Location {
        Location::new(
            world,
            f64::from(self.x) + 0.5,
            f64::from(self.y),
            f64::from(self.z) + 0.5,
        )
    }
}

// ---------------------------------------------------------------------------
// Items.

impl ItemStack {
    /// `count` of `id` (`minecraft:` may be left out).
    pub fn of(id: &str, count: u32) -> ItemStack {
        let id = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        ItemStack {
            id,
            count,
            components: String::new(),
        }
    }
    /// The stack's common components.
    pub fn meta(&self) -> items::ItemMeta {
        items::meta(self)
    }
    /// The stack with its components changed by `f`.
    pub fn edit(self, f: impl FnOnce(&mut items::ItemMeta)) -> ItemStack {
        let mut meta = items::meta(&self);
        f(&mut meta);
        items::with_meta(&self, &meta)
    }
    /// The name shown in italics (text).
    pub fn named(self, name: &str) -> ItemStack {
        self.edit(|m| m.display_name = Some(name.to_owned()))
    }
    /// The name without italics, as GUI items usually have (text).
    pub fn item_named(self, name: &str) -> ItemStack {
        self.edit(|m| m.item_name = Some(name.to_owned()))
    }
    pub fn lore(self, lines: &[&str]) -> ItemStack {
        self.edit(|m| m.lore = lines.iter().map(|l| (*l).to_owned()).collect())
    }
    pub fn enchant(self, id: &str, level: u32) -> ItemStack {
        let id = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        self.edit(|m| {
            m.enchantments.retain(|e| e.id != id);
            m.enchantments.push(items::Enchantment { id, level });
        })
    }
    pub fn unbreakable(self) -> ItemStack {
        self.edit(|m| m.unbreakable = true)
    }
    pub fn glint(self, on: bool) -> ItemStack {
        self.edit(|m| m.glint = Some(on))
    }
    /// The plugin's own value under `key` (persistent data).
    pub fn tag(&self, key: &str) -> Option<String> {
        items::get_tag(self, key)
    }
    pub fn with_tag(self, key: &str, value: &str) -> ItemStack {
        items::set_tag(&self, key, Some(value))
    }
    pub fn similar(&self, other: &ItemStack) -> bool {
        items::similar(self, other)
    }
}

// ---------------------------------------------------------------------------
// Inventories.

/// An inventory the plugin owns, shown as a chest-like menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Gui(pub u32);

impl Gui {
    /// A chest of `rows` (1 to 6) rows titled `title` (text).
    pub fn chest(rows: u32, title: &str) -> Gui {
        Gui(inventories::create(
            inventories::InventoryKind::Chest,
            rows.clamp(1, 6),
            title,
        ))
    }
    pub fn hopper(title: &str) -> Gui {
        Gui(inventories::create(
            inventories::InventoryKind::Hopper,
            1,
            title,
        ))
    }
    pub fn size(self) -> u32 {
        inventories::size(self.0)
    }
    pub fn set(self, slot: u32, item: Option<&ItemStack>) {
        inventories::set(self.0, slot, item);
    }
    pub fn get(self, slot: u32) -> Option<ItemStack> {
        inventories::get(self.0, slot)
    }
    /// Fills every empty slot with `item` (a frame of glass panes).
    pub fn fill_empty(self, item: &ItemStack) {
        let mut contents = inventories::contents(self.0);
        for slot in contents.iter_mut().filter(|s| s.is_none()) {
            *slot = Some(item.clone());
        }
        inventories::set_contents(self.0, &contents);
    }
    pub fn open(self, player: &str) -> bool {
        inventories::open(player, self.0)
    }
    pub fn destroy(self) {
        inventories::destroy(self.0);
    }
    /// Whether an inventory event is about this inventory.
    pub fn is(self, view: &InventoryView) -> bool {
        view.plugin_inventory == Some(self.0)
    }
}

// ---------------------------------------------------------------------------
// Data files.

/// The plugin's data folder as text files.
pub mod data {
    use super::files;
    /// A file of the data folder as text.
    pub fn read(path: &str) -> Option<String> {
        files::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
    }
    pub fn write(path: &str, text: &str) -> Result<(), String> {
        files::write(path, text.as_bytes())
    }
    /// The config file: the one in the data folder, written from the
    /// package's `defaults/` the first time.
    pub fn config(path: &str) -> Option<String> {
        files::save_default(path);
        read(path)
    }
    /// `key: value` and `key = value` lines of a simple config, `#`
    /// comments and blank lines skipped, quotes around values dropped.
    pub fn simple_config(text: &str) -> std::collections::BTreeMap<String, String> {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let at = l.find([':', '='])?;
                let value = l[at + 1..].trim().trim_matches('"').trim_matches('\'');
                Some((l[..at].trim().to_owned(), value.to_owned()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_configs() {
        assert_eq!(color("&aHi &Lthere & co"), "§aHi §lthere & co");
        assert_eq!(strip_color("§aHi"), "Hi");
        let c = data::simple_config("# x\nspawn: \"world\"\nradius = 5\n\n");
        assert_eq!(c["spawn"], "world");
        assert_eq!(c["radius"], "5");
    }

    #[test]
    fn locations() {
        let a = Location::new("minecraft:overworld", 0.5, 64.0, 0.5);
        let b = Location::new("minecraft:overworld", 3.5, 68.0, 0.5);
        assert_eq!(a.distance(&b), 5.0);
        assert_eq!(b.block_pos(), BlockPos::new(3, 68, 0));
        assert!(
            a.distance(&Location::new("minecraft:the_nether", 0.0, 0.0, 0.0))
                .is_infinite()
        );
    }
}
