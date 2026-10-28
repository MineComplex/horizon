//! horizon-server's plugin API, shaped like Bukkit's (Paper's `paper-api`).
//!
//! A plugin is a Rust type implementing [`Plugin`], compiled into
//! `horizon-server` (as `horizon-proxy` takes its plugins: Rust has no stable
//! ABI to load them from `plugins/` at run time). In
//! [`Plugin::on_enable`] it registers listeners and commands:
//!
//! ```
//! use horizon_server_api::{Plugin, PluginDescription, Plugins, event::*};
//!
//! struct Greeter;
//! impl Plugin for Greeter {
//!     fn description(&self) -> PluginDescription {
//!         PluginDescription::new("Greeter", "1.0")
//!     }
//!     fn on_enable(&mut self, plugins: &mut Plugins) {
//!         // @EventHandler(priority = EventPriority.HIGH)
//!         plugins.listen(EventPriority::High, false, |e: &mut PlayerJoinEvent, server| {
//!             e.join_message = Some(format!("§aWelcome, {}!", e.player.name));
//!             server.send_message(&e.player, "§7Have fun.");
//!         });
//!         plugins.command("hello", "Says hello", None, |ctx| {
//!             let name = ctx.sender.name().to_owned();
//!             ctx.reply(&format!("Hello, {name}!"));
//!             true
//!         });
//!     }
//! }
//! ```
//!
//! **What runs when.** Everything runs on the server thread, in the tick, as
//! Bukkit's synchronous events do. Handlers and commands read the server
//! through [`Server`]; what they change (messages, blocks, teleports, kicks,
//! commands they dispatch) is applied by the server right after the event or
//! the command returns, before anything else happens in the tick. An event's
//! own fields (`cancelled`, the join message, a chat message...) decide what
//! the event itself does.
//!
//! **Text** takes Bukkit's legacy colour codes: `§a`, `§l`, `§r`... (`&` is
//! not translated: use [`colorize`] for that, like `ChatColor.translateAlternateColorCodes`).
#![forbid(unsafe_code)]

pub mod command;
pub mod event;
mod host;
pub mod server;

pub use command::{CommandContext, CommandSender};
pub use event::{Event, EventPriority};
pub use host::{PluginHost, Plugins};
pub use server::{Action, BlockPos, ItemStack, Location, PlayerRef, Server, World};

/// What a plugin is: `plugin.yml`'s name, version, description and authors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginDescription {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    /// Plugins this one needs enabled first (`depend`).
    pub depend: Vec<String>,
}
impl PluginDescription {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            ..Self::default()
        }
    }
    pub fn description(mut self, text: &str) -> Self {
        self.description = text.to_owned();
        self
    }
    pub fn author(mut self, name: &str) -> Self {
        self.authors.push(name.to_owned());
        self
    }
    pub fn depend(mut self, plugin: &str) -> Self {
        self.depend.push(plugin.to_owned());
        self
    }
}

/// `JavaPlugin`: a plugin's life on the server.
pub trait Plugin: Send + 'static {
    fn description(&self) -> PluginDescription;
    /// `onLoad`: before the worlds load.
    fn on_load(&mut self) {}
    /// `onEnable`: the worlds are loaded; register listeners, commands and tasks.
    fn on_enable(&mut self, _plugins: &mut Plugins) {}
    /// `onDisable`: the server is stopping.
    fn on_disable(&mut self) {}
}

/// `ChatColor.translateAlternateColorCodes('&', text)`: `&a` and the like
/// become `§a`.
pub fn colorize(text: &str) -> String {
    const CODES: &str = "0123456789AaBbCcDdEeFfKkLlMmNnOoRrXx";
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&'
            && let Some(&next) = chars.peek()
            && CODES.contains(next)
        {
            out.push('§');
            out.push(next.to_ascii_lowercase());
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// `ChatColor.stripColor`: the text without its `§` codes.
pub fn strip_colors(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes_translate_and_strip() {
        assert_eq!(colorize("&aHi &Lthere & co"), "§aHi §lthere & co");
        assert_eq!(strip_colors("§aHi §lthere"), "Hi there");
    }
}
