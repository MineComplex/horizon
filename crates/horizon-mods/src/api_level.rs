//! Mod API levels: which of the WIT contract's functions a Horizon has
//! (`docs/api/stability.md`). Every function the game offers mods has the
//! level it arrived at; a Horizon serves every level up to its [`LEVEL`].
//! A mod needs the level of the newest function it imports, so a mod that
//! calls a function this Horizon doesn't have is refused with "update
//! Horizon" instead of a linking error, and the registry lists each
//! version's level so the launcher can tell before it installs.

/// The newest level this Horizon serves.
pub const LEVEL: u32 = 8;

/// Every function the game offers mods, as `interface.function`, with its
/// level. Level 0 is the contract of Horizon 0.2.67 to 0.2.69. A new
/// function takes the next level and raises [`LEVEL`] (the `api_reference`
/// test fails until it is listed here).
pub const FUNCTIONS: [(&str, u32); 254] = [
    ("log.write", 0),
    ("game.get-player", 0),
    ("game.get-clock", 0),
    ("chat.show", 0),
    ("hud.set-panel", 0),
    ("hud.clear", 0),
    ("storage.get", 0),
    ("storage.set", 0),
    ("storage.delete", 0),
    ("command.run", 0),
    ("ui.open-screen", 0),
    ("ui.close-screen", 0),
    ("ui.set-settings", 0),
    ("input.claim-items", 0),
    ("input.register-key", 0),
    ("input.held", 0),
    ("input.poll", 0),
    ("input.claim-wheel", 0),
    ("input.mapping", 0),
    ("input.clicks-per-second", 0),
    ("render.set-hud", 0),
    ("render.gui-size", 0),
    ("render.shake", 0),
    ("render.set-corner", 0),
    ("render.clear-corner", 0),
    ("render.text-width", 0),
    ("render.frame-stats", 0),
    ("render.set-zoom", 0),
    ("render.set-minimap", 1),
    ("players.online", 1),
    ("players.tell", 1),
    ("players.broadcast", 1),
    ("game.raycast", 2),
    ("command.run-quiet", 3),
    ("world.get-block", 4),
    ("world.get-biome", 4),
    ("world.set-blocks", 4),
    ("recipes.items", 5),
    ("recipes.all", 5),
    ("recipes.known", 5),
    ("browser.set-items", 6),
    ("browser.poll", 6),
    ("browser.set-recipes", 6),
    ("recipes.place", 6),
    ("recipes.give", 6),
    ("render.set-far-terrain", 7),
    // Plugins (API 0.4, docs/plugins.md).
    ("plugin-listeners.listen", 8),
    ("plugin-listeners.unlisten", 8),
    ("plugin-commands.register", 8),
    ("plugin-commands.unregister", 8),
    ("plugin-server.info", 8),
    ("plugin-server.tps", 8),
    ("plugin-server.mspt", 8),
    ("plugin-server.now-millis", 8),
    ("plugin-server.random", 8),
    ("plugin-server.broadcast", 8),
    ("plugin-server.console", 8),
    ("plugin-server.dispatch-command", 8),
    ("plugin-server.shutdown", 8),
    ("plugin-server.set-motd", 8),
    ("plugin-server.set-max-players", 8),
    ("plugin-server.plugins", 8),
    ("plugin-server.plugin-enabled", 8),
    ("plugin-server.send-to-plugin", 8),
    ("plugin-server.find-player", 8),
    ("plugin-server.whitelist-enabled", 8),
    ("plugin-server.set-whitelist-enabled", 8),
    ("plugin-server.whitelist", 8),
    ("plugin-server.set-whitelisted", 8),
    ("plugin-server.operators", 8),
    ("plugin-server.banned-players", 8),
    ("plugin-server.ban", 8),
    ("plugin-server.pardon", 8),
    ("plugin-server.ban-ip", 8),
    ("plugin-server.pardon-ip", 8),
    ("plugin-players.online", 8),
    ("plugin-players.get", 8),
    ("plugin-players.by-name", 8),
    ("plugin-players.send-message", 8),
    ("plugin-players.send-action-bar", 8),
    ("plugin-players.send-title", 8),
    ("plugin-players.clear-title", 8),
    ("plugin-players.set-tab-list", 8),
    ("plugin-players.set-list-name", 8),
    ("plugin-players.set-display-name", 8),
    ("plugin-players.play-sound", 8),
    ("plugin-players.stop-sound", 8),
    ("plugin-players.spawn-particle", 8),
    ("plugin-players.send-block-change", 8),
    ("plugin-players.kick", 8),
    ("plugin-players.transfer", 8),
    ("plugin-players.send-resource-pack", 8),
    ("plugin-players.teleport", 8),
    ("plugin-players.set-game-mode", 8),
    ("plugin-players.set-health", 8),
    ("plugin-players.set-max-health", 8),
    ("plugin-players.set-food", 8),
    ("plugin-players.set-saturation", 8),
    ("plugin-players.set-level", 8),
    ("plugin-players.set-exp", 8),
    ("plugin-players.give-exp", 8),
    ("plugin-players.set-allow-flight", 8),
    ("plugin-players.set-flying", 8),
    ("plugin-players.set-walk-speed", 8),
    ("plugin-players.set-fly-speed", 8),
    ("plugin-players.set-op", 8),
    ("plugin-players.set-velocity", 8),
    ("plugin-players.damage", 8),
    ("plugin-players.kill", 8),
    ("plugin-players.set-fire-ticks", 8),
    ("plugin-players.effects", 8),
    ("plugin-players.add-effect", 8),
    ("plugin-players.remove-effect", 8),
    ("plugin-players.clear-effects", 8),
    ("plugin-players.hide-player", 8),
    ("plugin-players.show-player", 8),
    ("plugin-players.can-see", 8),
    ("plugin-players.perform-command", 8),
    ("plugin-players.chat", 8),
    ("plugin-players.set-respawn-location", 8),
    ("plugin-players.inventory", 8),
    ("plugin-players.get-slot", 8),
    ("plugin-players.set-slot", 8),
    ("plugin-players.give", 8),
    ("plugin-players.remove-item", 8),
    ("plugin-players.clear-inventory", 8),
    ("plugin-players.ender-chest", 8),
    ("plugin-players.set-ender-slot", 8),
    ("plugin-players.set-held-slot", 8),
    ("plugin-players.cursor", 8),
    ("plugin-players.set-cursor", 8),
    ("plugin-players.close-inventory", 8),
    ("plugin-players.update-inventory", 8),
    ("plugin-worlds.worlds", 8),
    ("plugin-worlds.get", 8),
    ("plugin-worlds.get-block", 8),
    ("plugin-worlds.get-biome", 8),
    ("plugin-worlds.highest-block-y", 8),
    ("plugin-worlds.get-block-data", 8),
    ("plugin-worlds.merge-block-data", 8),
    ("plugin-worlds.set-blocks", 8),
    ("plugin-worlds.is-chunk-loaded", 8),
    ("plugin-worlds.loaded-chunks", 8),
    ("plugin-worlds.set-chunk-forced", 8),
    ("plugin-worlds.set-time", 8),
    ("plugin-worlds.set-storm", 8),
    ("plugin-worlds.set-thundering", 8),
    ("plugin-worlds.set-spawn", 8),
    ("plugin-worlds.set-difficulty", 8),
    ("plugin-worlds.game-rule", 8),
    ("plugin-worlds.set-game-rule", 8),
    ("plugin-worlds.strike-lightning", 8),
    ("plugin-worlds.explode", 8),
    ("plugin-worlds.drop-item", 8),
    ("plugin-worlds.play-sound", 8),
    ("plugin-worlds.spawn-particle", 8),
    ("plugin-worlds.border", 8),
    ("plugin-worlds.set-border", 8),
    ("plugin-worlds.save", 8),
    ("plugin-entities.get", 8),
    ("plugin-entities.by-uuid", 8),
    ("plugin-entities.in-world", 8),
    ("plugin-entities.nearby", 8),
    ("plugin-entities.by-tag", 8),
    ("plugin-entities.spawn", 8),
    ("plugin-entities.teleport", 8),
    ("plugin-entities.remove", 8),
    ("plugin-entities.set-velocity", 8),
    ("plugin-entities.set-custom-name", 8),
    ("plugin-entities.set-glowing", 8),
    ("plugin-entities.set-invulnerable", 8),
    ("plugin-entities.set-silent", 8),
    ("plugin-entities.set-gravity", 8),
    ("plugin-entities.set-ai", 8),
    ("plugin-entities.damage", 8),
    ("plugin-entities.set-health", 8),
    ("plugin-entities.add-tag", 8),
    ("plugin-entities.remove-tag", 8),
    ("plugin-entities.add-passenger", 8),
    ("plugin-entities.eject", 8),
    ("plugin-entities.get-nbt", 8),
    ("plugin-entities.merge-nbt", 8),
    ("plugin-items.parse", 8),
    ("plugin-items.meta", 8),
    ("plugin-items.with-meta", 8),
    ("plugin-items.get-tag", 8),
    ("plugin-items.set-tag", 8),
    ("plugin-items.similar", 8),
    ("plugin-items.max-stack", 8),
    ("plugin-items.display-name", 8),
    ("plugin-items.registry", 8),
    ("plugin-inventories.create", 8),
    ("plugin-inventories.size", 8),
    ("plugin-inventories.get", 8),
    ("plugin-inventories.set", 8),
    ("plugin-inventories.contents", 8),
    ("plugin-inventories.set-contents", 8),
    ("plugin-inventories.add", 8),
    ("plugin-inventories.clear", 8),
    ("plugin-inventories.set-title", 8),
    ("plugin-inventories.open", 8),
    ("plugin-inventories.viewers", 8),
    ("plugin-inventories.open-inventory", 8),
    ("plugin-inventories.destroy", 8),
    ("plugin-scheduler.run-later", 8),
    ("plugin-scheduler.cancel", 8),
    ("plugin-scheduler.cancel-all", 8),
    ("plugin-scheduler.is-queued", 8),
    ("plugin-scheduler.current-tick", 8),
    ("plugin-permissions.has", 8),
    ("plugin-permissions.is-set", 8),
    ("plugin-permissions.set", 8),
    ("plugin-permissions.unset", 8),
    ("plugin-permissions.clear", 8),
    ("plugin-permissions.effective", 8),
    ("plugin-permissions.declare", 8),
    ("plugin-scoreboards.set-sidebar", 8),
    ("plugin-scoreboards.clear-sidebar", 8),
    ("plugin-scoreboards.set-name-tag", 8),
    ("plugin-scoreboards.clear-name-tag", 8),
    ("plugin-scoreboards.objectives", 8),
    ("plugin-scoreboards.add-objective", 8),
    ("plugin-scoreboards.remove-objective", 8),
    ("plugin-scoreboards.set-display-slot", 8),
    ("plugin-scoreboards.get-score", 8),
    ("plugin-scoreboards.set-score", 8),
    ("plugin-scoreboards.reset-score", 8),
    ("plugin-scoreboards.teams", 8),
    ("plugin-scoreboards.add-team", 8),
    ("plugin-scoreboards.remove-team", 8),
    ("plugin-scoreboards.join-team", 8),
    ("plugin-scoreboards.leave-team", 8),
    ("plugin-scoreboards.set-team-option", 8),
    ("plugin-bossbars.create", 8),
    ("plugin-bossbars.set-title", 8),
    ("plugin-bossbars.set-progress", 8),
    ("plugin-bossbars.set-color", 8),
    ("plugin-bossbars.set-style", 8),
    ("plugin-bossbars.set-flags", 8),
    ("plugin-bossbars.add-player", 8),
    ("plugin-bossbars.remove-player", 8),
    ("plugin-bossbars.players", 8),
    ("plugin-bossbars.remove", 8),
    ("plugin-messaging.register-incoming", 8),
    ("plugin-messaging.unregister-incoming", 8),
    ("plugin-messaging.send", 8),
    ("plugin-files.read", 8),
    ("plugin-files.write", 8),
    ("plugin-files.append", 8),
    ("plugin-files.delete", 8),
    ("plugin-files.exists", 8),
    ("plugin-files.list", 8),
    ("plugin-files.bundled", 8),
    ("plugin-files.save-default", 8),
];

/// What the game's program carries so the launcher can read its level
/// without running it: `horizon-mod-api-level:0001;` (four digits). A
/// program from before the mark serves level 0.
pub const MARK: [u8; 27] = {
    let mut mark = *b"horizon-mod-api-level:0000;";
    let mut level = LEVEL;
    let mut i = 25;
    while level > 0 {
        mark[i] = b'0' + (level % 10) as u8;
        level /= 10;
        i -= 1;
    }
    mark
};

/// The level a program's bytes carry in their [`MARK`], if any.
pub fn read_mark(program: &[u8]) -> Option<u32> {
    let prefix = &MARK[..22];
    program
        .windows(MARK.len())
        .filter(|w| w.starts_with(prefix) && w[26] == b';')
        .find_map(|w| std::str::from_utf8(&w[22..26]).ok()?.parse().ok())
}

/// A function's level; None for one this Horizon doesn't have.
pub fn level_of(function: &str) -> Option<u32> {
    FUNCTIONS
        .iter()
        .find(|(name, _)| *name == function)
        .map(|&(_, level)| level)
}

/// The level a mod needs, from the functions it imports (as [`imports`]
/// names them): its newest function's. A function this Horizon doesn't
/// know comes from a newer one, so it needs more than [`LEVEL`]; that
/// function is returned with it, for the message.
pub fn needed<'a>(imports: impl IntoIterator<Item = &'a str>) -> (u32, Option<&'a str>) {
    let mut needed = (0, None);
    for name in imports {
        match level_of(name) {
            Some(level) if level > needed.0 => needed = (level, Some(name)),
            Some(_) => {}
            None => return (LEVEL + 1, Some(name)),
        }
    }
    needed
}

/// Why this Horizon can't run a mod that needs `level`, or None when it can.
pub fn too_new(level: u32, function: Option<&str>) -> Option<String> {
    (level > LEVEL).then(|| {
        let uses = function.map_or(String::new(), |f| format!(" (it uses {f})"));
        format!("made for a newer Horizon{uses}: update Horizon to run it")
    })
}

/// The functions a mod's component imports from Horizon (`horizon:mods/*`),
/// as `interface.function`, in the component's order.
#[cfg(any(feature = "runtime", feature = "index"))]
pub fn imports(wasm: &[u8]) -> Result<Vec<String>, String> {
    use wasmparser::{
        Parser, Payload, Validator, WasmFeatures, component_types::ComponentEntityType,
    };
    let types = Validator::new_with_features(WasmFeatures::all())
        .validate_all(wasm)
        .map_err(|e| format!("not a component: {e}"))?;
    let types = types.as_ref();
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        let Payload::ComponentImportSection(section) = payload.map_err(|e| e.to_string())? else {
            continue;
        };
        for import in section {
            let name = import.map_err(|e| e.to_string())?.name.name;
            // `horizon:mods/render@0.1.0` → `render`.
            let Some(interface) = name.strip_prefix("horizon:mods/") else {
                continue;
            };
            let interface = interface.split('@').next().unwrap_or(interface);
            let Some(item) = types.component_item_for_import(name) else {
                continue;
            };
            if let ComponentEntityType::Instance(id) = item.ty {
                for (function, export) in &types[id].exports {
                    if matches!(export.ty, ComponentEntityType::Func(_)) {
                        out.push(format!("{interface}.{function}"));
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_listed_once_and_up_to_this_horizons() {
        for (i, (name, level)) in FUNCTIONS.iter().enumerate() {
            assert!(*level <= LEVEL, "{name}");
            assert!(
                FUNCTIONS[..i].iter().all(|(other, _)| other != name),
                "{name} twice"
            );
        }
        assert!(FUNCTIONS.iter().any(|&(_, level)| level == LEVEL));
    }

    #[test]
    fn the_mark_carries_the_level() {
        assert_eq!(read_mark(&MARK), Some(LEVEL));
        let mut program = b"\0\x7fELF horizon-mod-api-level: horizon-mod-api-level:".to_vec();
        program.extend_from_slice(&MARK);
        program.extend_from_slice(b"\0rest");
        assert_eq!(read_mark(&program), Some(LEVEL));
        assert_eq!(read_mark(b"horizon-mod-api-level:12;"), None);
        assert_eq!(read_mark(b"no mark"), None);
    }

    #[test]
    fn a_mod_needs_its_newest_function() {
        assert_eq!(needed(["log.write", "hud.set-panel"]), (0, None));
        assert_eq!(
            needed(["log.write", "render.set-minimap", "hud.clear"]),
            (1, Some("render.set-minimap"))
        );
        assert_eq!(
            needed(["render.set-minimap", "render.set-sky"]),
            (LEVEL + 1, Some("render.set-sky"))
        );
        assert_eq!(too_new(LEVEL, None), None);
        assert_eq!(
            too_new(LEVEL + 1, Some("render.set-sky")).as_deref(),
            Some("made for a newer Horizon (it uses render.set-sky): update Horizon to run it")
        );
    }
}
