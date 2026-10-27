//! `horizon-mod.toml`, a mod's manifest, and the `.hmod` file around it.
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::Read,
    path::{Path, PathBuf},
};

/// The manifest's file name, beside `mod.wasm`.
pub const MANIFEST: &str = "horizon-mod.toml";
/// The component's file name in a `.hmod` or a mod folder.
pub const WASM: &str = "mod.wasm";
/// The API (WIT package `horizon:mods`) versions this build runs: a mod
/// built for any 0.1.x SDK.
pub const API: &str = "0.1";
/// The manifest `api` lines this build loads. 0.2 is 0.1 plus content:
/// `assets/` and `data/` in the `.hmod`, with blocks and items declared in
/// `data/<id>/horizon/` (docs/modpacks.md). Code still links API 0.1.
/// 0.3 is a server mod (experimental): the `horizon-server-mod` world.
/// 0.4 is a plugin: the `horizon-plugin` world (docs/plugins.md).
pub const APIS: [&str; 4] = [API, "0.2", SERVER_API, PLUGIN_API];
/// The line of server mods, experimental: `side = "server"`.
pub const SERVER_API: &str = "0.3";
/// The line of plugins, Bukkit-style server code: `side = "server"` and
/// capability `plugin` (docs/plugins.md).
pub const PLUGIN_API: &str = "0.4";
/// Namespaces no mod may claim: vanilla's, the convention tags' (mods add
/// to those, docs/mods.md) and the game's own.
const RESERVED_NAMESPACES: [&str; 3] = ["minecraft", "c", "horizon"];
/// A mod's data (capability `storage`), in `horizon-mods/` beside the mods
/// (`find` never mistakes it for a mod: it has no manifest).
pub const STORAGE_DIR: &str = ".horizon-mod-data";
/// The biggest `mod.wasm` a mod may ship.
const MAX_WASM: u64 = 32 << 20;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[a-z0-9_]`, 1 to 64 characters: the mod's folder, log and settings key.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// The API line the mod was built for: `0.1`, or `0.2` for content.
    pub api: String,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// Namespaces besides `id` that the mod's `assets/` and `data/` use, for
    /// a content mod whose files sit under other names than its id (Towns
    /// and Towers ships `kaisyn` and `towns_and_towers`). No two content mods
    /// that are on may share one (`content::clash`).
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// The experimental capabilities the mod opts into (`["ui"]`): each one
    /// it lists in `capabilities` must be named here too, since it may change
    /// in any release (docs/api/stability.md). One no longer experimental is
    /// accepted here, so the manifest keeps loading once it is promoted.
    #[serde(default)]
    pub experimental: Vec<Capability>,
    /// Plugins (API 0.4): the plugins this one needs, enabled before it; it
    /// stays off without them (plugin.yml's `depend`).
    #[serde(default)]
    pub depend: Vec<String>,
    /// Plugins enabled before this one when they are installed (`softdepend`).
    #[serde(default)]
    pub softdepend: Vec<String>,
    /// Plugins enabled after this one when they are installed (`loadbefore`).
    #[serde(default)]
    pub loadbefore: Vec<String>,
    /// Plugins: the commands it owns, by name (`commands`).
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, CommandSpec>,
    /// Plugins: the permission nodes it declares (`permissions`).
    #[serde(default)]
    pub permissions: std::collections::BTreeMap<String, PermissionSpec>,
    /// Plugins: shown in `/plugins` and its log lines instead of `name`.
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
}

/// A command a plugin's manifest declares (`[commands.<name>]`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CommandSpec {
    #[serde(default)]
    pub description: String,
    /// Shown when the plugin's handler returns false; `<command>` is the
    /// label the sender typed.
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// The node a sender needs to run it.
    #[serde(default)]
    pub permission: Option<String>,
    /// What a sender without the permission reads (text).
    #[serde(default)]
    pub permission_message: Option<String>,
}

/// A permission node a plugin's manifest declares (`[permissions."a.b"]`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PermissionSpec {
    #[serde(default)]
    pub description: String,
    /// Who has it when no plugin set it: `true`, `false`, `op` (the
    /// default) or `not-op`.
    #[serde(default)]
    pub default: PermissionDefault,
    /// Nodes it sets with it: `{ "a.b.c" = true }`.
    #[serde(default)]
    pub children: std::collections::BTreeMap<String, bool>,
}

/// Who has a permission node by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionDefault {
    True,
    False,
    #[default]
    Op,
    NotOp,
}
impl PermissionDefault {
    /// Bukkit's spellings: `true`, `false`, `op`, `not op`, `not-op`, `notop`
    /// (any case), or a TOML boolean.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
            "true" | "always" => Some(Self::True),
            "false" | "never" => Some(Self::False),
            "op" | "isop" | "operator" => Some(Self::Op),
            "not-op" | "notop" | "!op" | "non-op" => Some(Self::NotOp),
            _ => None,
        }
    }
    /// Whether a player who is (or isn't) an operator has it.
    pub fn grants(self, op: bool) -> bool {
        match self {
            Self::True => true,
            Self::False => false,
            Self::Op => op,
            Self::NotOp => !op,
        }
    }
}
impl<'de> Deserialize<'de> for PermissionDefault {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(Self::True),
            Raw::Bool(false) => Ok(Self::False),
            Raw::Text(t) => Self::parse(&t).ok_or_else(|| {
                serde::de::Error::custom(format!("default {t:?} is not true, false, op or not-op"))
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Runs in the player's game only (HUD, keys, chat commands).
    #[default]
    Client,
    /// Runs where the world is simulated (rules, blocks): `api = "0.3"`,
    /// or a plugin with `api = "0.4"`.
    Server,
    /// Both, and the server tells clients which version it expects.
    Both,
}

/// What a mod may do beyond writing its log. Each is a WIT interface the
/// component imports; the player sees them before turning the mod on.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    GameRead,
    Chat,
    Hud,
    Storage,
    /// Runs vanilla commands as the player, with the player's permissions.
    Command,
    /// New blocks and items from the mod's `data/` (API 0.2): no WIT
    /// interface, the game reads them before the world loads.
    Registries,
    /// A screen the mod owns and a settings page in Options > Mods, drawn
    /// with the menus' own widgets (interface `ui`, calls `ui-events`).
    Ui,
    /// Shapes on the HUD and the camera's shake, drawn by the game's own
    /// renderer (interface `render`).
    Render,
    /// The use button on items the mod claims, and its own keys
    /// (interface `input`).
    Input,
    /// The players on the server and messages to them (server mods).
    Players,
    /// Blocks in the server's loaded chunks, and changes to them (server
    /// mods; client mods later, where the game runs the world).
    World,
    /// The recipes and items the game knows, for recipe viewers
    /// (interface `recipes`).
    Recipes,
    /// A plugin's reach over the whole server (every `plugin-*` interface):
    /// players, worlds, entities, items, inventories, permissions, events.
    Plugin,
}
impl Capability {
    pub const ALL: [Capability; 13] = [
        Capability::GameRead,
        Capability::Chat,
        Capability::Hud,
        Capability::Storage,
        Capability::Command,
        Capability::Registries,
        Capability::Ui,
        Capability::Render,
        Capability::Input,
        Capability::Players,
        Capability::World,
        Capability::Recipes,
        Capability::Plugin,
    ];
    /// Whether a server mod may have it: the others act on a local player.
    pub fn on_server(self) -> bool {
        matches!(
            self,
            Capability::Storage | Capability::Players | Capability::World | Capability::Plugin
        )
    }
    /// The WIT interface it grants, in `horizon:mods`.
    pub fn interface(self) -> &'static str {
        match self {
            Capability::GameRead => "game",
            Capability::Chat => "chat",
            Capability::Hud => "hud",
            Capability::Storage => "storage",
            Capability::Command => "command",
            Capability::Registries => "registries",
            Capability::Ui => "ui",
            Capability::Render => "render",
            Capability::Input => "input",
            Capability::Players => "players",
            Capability::World => "world",
            Capability::Recipes => "recipes",
            Capability::Plugin => "plugin-server",
        }
    }
    /// May change in any release: a mod uses it only with
    /// `experimental = ["<name>"]` in its manifest.
    pub fn is_experimental(self) -> bool {
        matches!(
            self,
            Capability::Ui | Capability::Render | Capability::Input | Capability::Recipes
        )
    }
    /// What the player reads before turning the mod on: "May <this>".
    pub fn describe(self) -> &'static str {
        match self {
            Capability::GameRead => "see your position, health and the time",
            Capability::Chat => "show you chat messages",
            Capability::Hud => "draw text on your screen",
            Capability::Storage => "save its own settings to disk",
            Capability::Command => "run commands as you, with your permissions",
            Capability::Registries => "add blocks and items to your worlds",
            Capability::Ui => "show its own screens and a settings page",
            Capability::Render => "draw on your screen, shake and zoom the camera",
            Capability::Input => {
                "see the keys you hold and your clicks, take over the use button on its own items and the mouse wheel, and add keys"
            }
            Capability::Players => "see who is on the server and message them",
            Capability::World => "read and change the world's blocks",
            Capability::Recipes => {
                "read the game's recipes and items and which recipes you know, place recipes in your crafting grid and, in creative mode, give you items"
            }
            Capability::Plugin => {
                "run as a server plugin: see and change players, worlds, entities, items and permissions, and hear and cancel what happens on the server"
            }
        }
    }
    /// The capability a component import needs, `Ok(None)` for one every
    /// mod has; `Err` for an import Horizon does not offer.
    pub fn of_import(name: &str) -> Result<Option<Capability>, String> {
        let unknown = || format!("imports {name}, which Horizon does not offer");
        let rest = name.strip_prefix("horizon:mods/").ok_or_else(unknown)?;
        let (interface, version) = rest.split_once('@').ok_or_else(unknown)?;
        if !version.starts_with("0.1.") {
            return Err(format!("needs API {version}; this Horizon runs {API}"));
        }
        if interface == "log" {
            return Ok(None);
        }
        // A recipe viewer's panel is part of capability `ui`.
        if interface == "browser" {
            return Ok(Some(Capability::Ui));
        }
        // Every plugin interface is capability `plugin`.
        if interface == "plugin" || interface.starts_with("plugin-") {
            return Ok(Some(Capability::Plugin));
        }
        Capability::ALL
            .into_iter()
            .filter(|c| *c != Capability::Registries)
            .find(|c| c.interface() == interface)
            .map(Some)
            .ok_or_else(unknown)
    }
}
impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Capability::GameRead => "game-read",
            Capability::Chat => "chat",
            Capability::Hud => "hud",
            Capability::Storage => "storage",
            Capability::Command => "command",
            Capability::Registries => "registries",
            Capability::Ui => "ui",
            Capability::Render => "render",
            Capability::Input => "input",
            Capability::Players => "players",
            Capability::World => "world",
            Capability::Recipes => "recipes",
            Capability::Plugin => "plugin",
        };
        f.write_str(name)
    }
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| format!("{MANIFEST}: {e}"))?;
        let id_ok = (1..=64).contains(&m.id.len())
            && m.id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !id_ok {
            return Err(format!(
                "{MANIFEST}: id {:?} is not [a-z0-9_]{{1,64}}",
                m.id
            ));
        }
        if !APIS.contains(&m.api.as_str()) {
            return Err(format!(
                "built for API {}; this Horizon runs {}",
                m.api,
                APIS.join(" and ")
            ));
        }
        if m.capabilities.contains(&Capability::Registries) && m.api == API {
            return Err(format!(
                "{MANIFEST}: capability registries needs api = \"0.2\""
            ));
        }
        let server = m.side == Side::Server;
        let plugin = m.api == PLUGIN_API;
        if server != (m.api == SERVER_API || plugin) {
            return Err(format!(
                "{MANIFEST}: a server mod is side = \"server\" with api = \"{SERVER_API}\", and only it"
            ));
        }
        if plugin != m.capabilities.contains(&Capability::Plugin) {
            return Err(format!(
                "{MANIFEST}: a plugin is api = \"{PLUGIN_API}\" with capability plugin, and only it"
            ));
        }
        let plugin_fields = !m.depend.is_empty()
            || !m.softdepend.is_empty()
            || !m.loadbefore.is_empty()
            || !m.commands.is_empty()
            || !m.permissions.is_empty()
            || m.prefix.is_some();
        if plugin_fields && !plugin {
            return Err(format!(
                "{MANIFEST}: depend, softdepend, loadbefore, commands, permissions and prefix are for plugins (api = \"{PLUGIN_API}\")"
            ));
        }
        for (name, spec) in &m.commands {
            for c in std::iter::once(name).chain(&spec.aliases) {
                if !command_name_ok(c) {
                    return Err(format!("{MANIFEST}: command {c:?} is not a command name"));
                }
            }
        }
        for dep in m.depend.iter().chain(&m.softdepend).chain(&m.loadbefore) {
            if *dep == m.id {
                return Err(format!(
                    "{MANIFEST}: the plugin names itself in depend, softdepend or loadbefore"
                ));
            }
        }
        if let Some(c) = m
            .capabilities
            .iter()
            .find(|c| c.on_server() != server && **c != Capability::Storage)
        {
            let side = if server { "client" } else { "server" };
            return Err(format!("{MANIFEST}: capability {c} is for {side} mods"));
        }
        if let Some(c) = m
            .capabilities
            .iter()
            .find(|c| c.is_experimental() && !m.experimental.contains(c))
        {
            return Err(format!(
                "{MANIFEST}: capability {c} is experimental and may change in any release: \
                 add experimental = [\"{c}\"] to use it"
            ));
        }
        if !m.namespaces.is_empty() && !m.capabilities.contains(&Capability::Registries) {
            return Err(format!(
                "{MANIFEST}: namespaces are for content, which needs capability registries"
            ));
        }
        for (i, ns) in m.namespaces.iter().enumerate() {
            // Vanilla's namespace characters, at most as long as an id.
            let chars_ok = (1..=64).contains(&ns.len())
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
            if !chars_ok {
                return Err(format!(
                    "{MANIFEST}: namespace {ns:?} is not [a-z0-9_.-]{{1,64}}"
                ));
            }
            if RESERVED_NAMESPACES.contains(&ns.as_str()) {
                return Err(format!(
                    "{MANIFEST}: namespace {ns} is not a mod's to claim (only to add to its tags)"
                ));
            }
            if *ns == m.id || m.namespaces[..i].contains(ns) {
                return Err(format!("{MANIFEST}: namespace {ns} is listed twice"));
            }
        }
        Ok(m)
    }
    /// A plugin (API 0.4).
    pub fn is_plugin(&self) -> bool {
        self.api == PLUGIN_API
    }
    /// The name to show for the plugin: its `prefix`, else its `name`.
    pub fn display_name(&self) -> &str {
        self.prefix.as_deref().unwrap_or(&self.name)
    }
    /// The namespaces the mod's content owns: its id, then `namespaces`.
    pub fn owned_namespaces(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.namespaces.iter().map(String::as_str))
    }
    /// Checks that the component's imports need only declared capabilities.
    pub fn check_imports<'a>(
        &self,
        imports: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        for name in imports {
            if let Some(c) = Capability::of_import(name)?
                && !self.capabilities.contains(&c)
            {
                return Err(format!("uses capability {c}, missing from {MANIFEST}"));
            }
        }
        Ok(())
    }
}

/// A command name or alias: no spaces, slashes or colons, 1 to 64 characters.
pub fn command_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && c != '/' && c != ':')
}

/// A mod as found on disk: its manifest and component bytes.
pub struct Package {
    pub manifest: Manifest,
    /// None for a content-only mod (capability `registries`, no code).
    pub wasm: Option<Vec<u8>>,
}
impl Package {
    /// A `.hmod` (a zip holding `horizon-mod.toml`, `mod.wasm` and, for
    /// content, `assets/` and `data/`) or a folder holding the same.
    pub fn read(path: &Path) -> Result<Self, String> {
        let (toml, wasm) = if path.is_dir() {
            let toml = std::fs::read_to_string(path.join(MANIFEST));
            let wasm = path.join(WASM);
            let wasm = match wasm.exists() {
                true => Some(std::fs::read(wasm).map_err(|e| format!("{WASM}: {e}"))?),
                false => None,
            };
            (toml.map_err(|e| format!("{MANIFEST}: {e}"))?, wasm)
        } else {
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            let mut entry = |name: &str| -> Result<Option<Vec<u8>>, String> {
                let f = match zip.by_name(name) {
                    Ok(f) => f,
                    Err(zip::result::ZipError::FileNotFound) => return Ok(None),
                    Err(e) => return Err(format!("{name}: {e}")),
                };
                let mut out = Vec::new();
                f.take(MAX_WASM + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| e.to_string())?;
                Ok(Some(out))
            };
            let toml = entry(MANIFEST)?.ok_or_else(|| format!("{MANIFEST}: missing"))?;
            let toml = String::from_utf8(toml).map_err(|_| format!("{MANIFEST}: not UTF-8"))?;
            (toml, entry(WASM)?)
        };
        if wasm.as_ref().is_some_and(|w| w.len() as u64 > MAX_WASM) {
            return Err(format!("{WASM} is over {} MiB", MAX_WASM >> 20));
        }
        let manifest = Manifest::parse(&toml)?;
        if wasm.is_none() && !manifest.capabilities.contains(&Capability::Registries) {
            return Err(format!(
                "{WASM}: missing (only a content mod, with capability registries, may have no code)"
            ));
        }
        Ok(Package { manifest, wasm })
    }
    /// The `.hmod` bytes for `manifest_text` and a component.
    pub fn write(manifest_text: &str, wasm: &[u8]) -> Result<Vec<u8>, String> {
        Self::write_files(manifest_text, Some(wasm), &[])
    }
    /// The `.hmod` bytes for `manifest_text`, a component if the mod has
    /// code, and content `files` (`assets/...`, `data/...`) by path.
    pub fn write_files(
        manifest_text: &str,
        wasm: Option<&[u8]>,
        files: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, String> {
        use std::io::Write;
        let manifest = Manifest::parse(manifest_text)?;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let err = |e: zip::result::ZipError| e.to_string();
        let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
            zip.start_file(name, options).map_err(err)?;
            zip.write_all(bytes).map_err(|e| e.to_string())
        };
        put(MANIFEST, manifest_text.as_bytes())?;
        if let Some(wasm) = wasm {
            put(WASM, wasm)?;
        }
        for (name, bytes) in files {
            if manifest.is_plugin() && name.starts_with("defaults/") {
                crate::content::check_defaults_path(name)?;
            } else {
                crate::content::check_path(name)?;
            }
            put(name, bytes)?;
        }
        Ok(zip.finish().map_err(err)?.into_inner())
    }
}

/// The `.hmod` files and mod folders in `dir`, by name.
pub fn find(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "hmod") || p.join(MANIFEST).is_file())
        .collect();
    found.sort();
    found
}

/// Turns a `wasm32-unknown-unknown` cdylib built with the SDK into the
/// component the game loads.
#[cfg(feature = "pack")]
pub fn componentize(core: &[u8]) -> Result<Vec<u8>, String> {
    wit_component::ComponentEncoder::default()
        .module(core)
        .and_then(|e| e.validate(true).encode())
        .map_err(|e| format!("{e:#}"))
}

/// The names a component imports: `horizon:mods/game@0.1.0`, ...
#[cfg(feature = "pack")]
pub fn component_imports(component: &[u8]) -> Result<Vec<String>, String> {
    use wit_parser::{WorldItem, WorldKey};
    let wit_component::DecodedWasm::Component(resolve, world) =
        wit_component::decode(component).map_err(|e| format!("{e:#}"))?
    else {
        return Err("a WIT package, not a component".into());
    };
    let names = resolve.worlds[world]
        .imports
        .iter()
        .map(|(key, item)| match (key, item) {
            (WorldKey::Interface(id), _) | (_, WorldItem::Interface { id, .. }) => resolve
                .id_of(*id)
                .unwrap_or_else(|| resolve.name_world_key(key)),
            _ => resolve.name_world_key(key),
        });
    Ok(names.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
id = "coords_hud"
name = "Coordinates HUD"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "hud"]
"#;

    #[test]
    fn parses_and_checks_imports() {
        let m = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.side, Side::Client);
        assert_eq!(m.capabilities, [Capability::GameRead, Capability::Hud]);
        let ok = [
            "horizon:mods/log@0.1.0",
            "horizon:mods/game@0.1.0",
            "horizon:mods/hud@0.1.2",
        ];
        assert_eq!(m.check_imports(ok), Ok(()));
        let chat = m.check_imports(["horizon:mods/chat@0.1.0"]);
        assert_eq!(
            chat,
            Err("uses capability chat, missing from horizon-mod.toml".into())
        );
        assert!(m.check_imports(["wasi:filesystem/types@0.2.0"]).is_err());
        assert!(m.check_imports(["horizon:mods/game@0.2.0"]).is_err());
        let storage = m.check_imports(["horizon:mods/storage@0.1.0"]);
        assert_eq!(
            storage,
            Err("uses capability storage, missing from horizon-mod.toml".into())
        );
        assert_eq!(
            m.check_imports(["horizon:mods/command@0.1.0"]),
            Err("uses capability command, missing from horizon-mod.toml".into())
        );
        let with_storage = EXAMPLE.replace("\"hud\"", "\"hud\", \"storage\"");
        let m = Manifest::parse(&with_storage).unwrap();
        assert_eq!(m.check_imports(["horizon:mods/storage@0.1.0"]), Ok(()));
    }

    #[test]
    fn refuses_bad_ids_and_apis() {
        let bad_id = EXAMPLE.replace("coords_hud", "Coords HUD");
        assert!(Manifest::parse(&bad_id).unwrap_err().contains("is not"));
        let bad_api = EXAMPLE.replace("api = \"0.1\"", "api = \"0.9\"");
        assert!(Manifest::parse(&bad_api).unwrap_err().contains("API 0.9"));
        let unknown = format!("{EXAMPLE}network = true\n");
        assert!(Manifest::parse(&unknown).is_err());
    }

    #[test]
    fn server_mods_are_api_0_3_with_server_capabilities() {
        let server = |api: &str, side: &str, caps: &str| {
            Manifest::parse(&format!(
                "id = \"w\"\nname = \"W\"\nversion = \"1\"\napi = \"{api}\"\nside = \"{side}\"\ncapabilities = [{caps}]\n"
            ))
        };
        let m = server("0.3", "server", "\"players\", \"storage\"").unwrap();
        let imports = ["horizon:mods/players@0.1.0", "horizon:mods/storage@0.1.0"];
        assert_eq!(m.check_imports(imports), Ok(()));
        for (api, side) in [("0.3", "client"), ("0.3", "both"), ("0.1", "server")] {
            let e = server(api, side, "").unwrap_err();
            assert!(e.contains("a server mod is side"), "{e}");
        }
        let e = server("0.3", "server", "\"hud\"").unwrap_err();
        assert!(e.contains("capability hud is for client mods"), "{e}");
        let e = server("0.1", "client", "\"players\"").unwrap_err();
        assert!(e.contains("capability players is for server mods"), "{e}");
    }

    #[test]
    fn plugins_are_api_0_4_with_capability_plugin() {
        let text = r#"
id = "essentials"
name = "Essentials"
version = "1.0"
api = "0.4"
side = "server"
capabilities = ["plugin", "storage"]
depend = ["vault"]
softdepend = ["luckperms"]

[commands.spawn]
description = "Teleports you to spawn"
usage = "/<command> [player]"
aliases = ["s"]
permission = "essentials.spawn"
permission-message = "&cYou may not."

[permissions."essentials.spawn"]
default = true

[permissions."essentials.spawn.others"]
default = "op"
children = { "essentials.spawn" = true }
"#;
        let m = Manifest::parse(text).unwrap();
        assert!(m.is_plugin());
        assert_eq!(m.commands["spawn"].aliases, ["s"]);
        assert_eq!(
            m.permissions["essentials.spawn"].default,
            PermissionDefault::True
        );
        assert!(m.permissions["essentials.spawn.others"].children["essentials.spawn"]);
        let imports = [
            "horizon:mods/plugin-players@0.1.0",
            "horizon:mods/plugin-types@0.1.0",
            "horizon:mods/storage@0.1.0",
        ];
        assert_eq!(m.check_imports(imports), Ok(()));
        // Capability `plugin` is a plugin's, and a plugin has it.
        let no_cap = text.replace("\"plugin\", ", "");
        assert!(
            Manifest::parse(&no_cap)
                .unwrap_err()
                .contains("capability plugin")
        );
        let mod_with = "id = \"w\"\nname = \"W\"\nversion = \"1\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"plugin\"]\n";
        assert!(
            Manifest::parse(mod_with)
                .unwrap_err()
                .contains("capability plugin")
        );
        let mod_cmds = format!("{EXAMPLE}depend = [\"x\"]\n");
        assert!(
            Manifest::parse(&mod_cmds)
                .unwrap_err()
                .contains("are for plugins")
        );
        let bad_default = text.replace("default = true", "default = \"sometimes\"");
        assert!(Manifest::parse(&bad_default).is_err());
        assert_eq!(
            PermissionDefault::parse("not op"),
            Some(PermissionDefault::NotOp)
        );
        assert!(PermissionDefault::NotOp.grants(false));
        assert!(!PermissionDefault::Op.grants(false));
    }

    #[test]
    fn checks_extra_namespaces() {
        let content = "id = \"t_and_t\"\nname = \"T\"\nversion = \"1\"\napi = \"0.2\"\ncapabilities = [\"registries\"]\n";
        let with = |list: &str| Manifest::parse(&format!("{content}namespaces = [{list}]\n"));
        let m = with("\"kaisyn\", \"towns_and_towers\"").unwrap();
        let owned: Vec<&str> = m.owned_namespaces().collect();
        assert_eq!(owned, ["t_and_t", "kaisyn", "towns_and_towers"]);
        for reserved in ["minecraft", "c", "horizon"] {
            let e = with(&format!("\"{reserved}\"")).unwrap_err();
            assert!(e.contains("not a mod's to claim"), "{e}");
        }
        assert!(with("\"t_and_t\"").unwrap_err().contains("twice"));
        assert!(with("\"a\", \"a\"").unwrap_err().contains("twice"));
        assert!(with("\"Kaisyn\"").unwrap_err().contains("is not"));
        assert!(with("\"\"").unwrap_err().contains("is not"));
        let code = format!("{EXAMPLE}namespaces = [\"kaisyn\"]\n");
        assert!(Manifest::parse(&code).unwrap_err().contains("registries"));
    }

    #[test]
    fn hmod_round_trips() {
        let dir = std::env::temp_dir().join(format!("horizon-mods-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("coords_hud.hmod");
        std::fs::write(&path, Package::write(EXAMPLE, b"\0asm").unwrap()).unwrap();
        let p = Package::read(&path).unwrap();
        assert_eq!(p.manifest.id, "coords_hud");
        assert_eq!(p.wasm.as_deref(), Some(&b"\0asm"[..]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn experimental_capabilities_need_the_opt_in() {
        let base = "id = \"m\"\nname = \"M\"\nversion = \"1\"\napi = \"0.1\"\n";
        let err = Manifest::parse(&format!("{base}capabilities = [\"ui\"]\n")).unwrap_err();
        assert!(err.contains("experimental = [\"ui\"]"), "{err}");
        let m = Manifest::parse(&format!(
            "{base}capabilities = [\"ui\"]\nexperimental = [\"ui\"]\n"
        ))
        .unwrap();
        assert_eq!(m.experimental, [Capability::Ui]);
        // A capability that is (or became) stable is fine in the list.
        Manifest::parse(&format!(
            "{base}capabilities = [\"hud\"]\nexperimental = [\"hud\"]\n"
        ))
        .unwrap();
    }
}
