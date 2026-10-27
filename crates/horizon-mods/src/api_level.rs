//! Mod API levels: which of the WIT contract's functions a Horizon has
//! (`docs/api/stability.md`). Every function the game offers mods has the
//! level it arrived at; a Horizon serves every level up to its [`LEVEL`].
//! A mod needs the level of the newest function it imports, so a mod that
//! calls a function this Horizon doesn't have is refused with "update
//! Horizon" instead of a linking error, and the registry lists each
//! version's level so the launcher can tell before it installs.

/// The newest level this Horizon serves.
pub const LEVEL: u32 = 7;

/// Every function the game offers mods, as `interface.function`, with its
/// level. Level 0 is the contract of Horizon 0.2.67 to 0.2.69. A new
/// function takes the next level and raises [`LEVEL`] (the `api_reference`
/// test fails until it is listed here).
pub const FUNCTIONS: [(&str, u32); 253] = [
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
    // Plugins (API 0.4, docs/plugins.md).
    ("plugin-listeners.listen", 7),
    ("plugin-listeners.unlisten", 7),
    ("plugin-commands.register", 7),
    ("plugin-commands.unregister", 7),
    ("plugin-server.info", 7),
    ("plugin-server.tps", 7),
    ("plugin-server.mspt", 7),
    ("plugin-server.now-millis", 7),
    ("plugin-server.random", 7),
    ("plugin-server.broadcast", 7),
    ("plugin-server.console", 7),
    ("plugin-server.dispatch-command", 7),
    ("plugin-server.shutdown", 7),
    ("plugin-server.set-motd", 7),
    ("plugin-server.set-max-players", 7),
    ("plugin-server.plugins", 7),
    ("plugin-server.plugin-enabled", 7),
    ("plugin-server.send-to-plugin", 7),
    ("plugin-server.find-player", 7),
    ("plugin-server.whitelist-enabled", 7),
    ("plugin-server.set-whitelist-enabled", 7),
    ("plugin-server.whitelist", 7),
    ("plugin-server.set-whitelisted", 7),
    ("plugin-server.operators", 7),
    ("plugin-server.banned-players", 7),
    ("plugin-server.ban", 7),
    ("plugin-server.pardon", 7),
    ("plugin-server.ban-ip", 7),
    ("plugin-server.pardon-ip", 7),
    ("plugin-players.online", 7),
    ("plugin-players.get", 7),
    ("plugin-players.by-name", 7),
    ("plugin-players.send-message", 7),
    ("plugin-players.send-action-bar", 7),
    ("plugin-players.send-title", 7),
    ("plugin-players.clear-title", 7),
    ("plugin-players.set-tab-list", 7),
    ("plugin-players.set-list-name", 7),
    ("plugin-players.set-display-name", 7),
    ("plugin-players.play-sound", 7),
    ("plugin-players.stop-sound", 7),
    ("plugin-players.spawn-particle", 7),
    ("plugin-players.send-block-change", 7),
    ("plugin-players.kick", 7),
    ("plugin-players.transfer", 7),
    ("plugin-players.send-resource-pack", 7),
    ("plugin-players.teleport", 7),
    ("plugin-players.set-game-mode", 7),
    ("plugin-players.set-health", 7),
    ("plugin-players.set-max-health", 7),
    ("plugin-players.set-food", 7),
    ("plugin-players.set-saturation", 7),
    ("plugin-players.set-level", 7),
    ("plugin-players.set-exp", 7),
    ("plugin-players.give-exp", 7),
    ("plugin-players.set-allow-flight", 7),
    ("plugin-players.set-flying", 7),
    ("plugin-players.set-walk-speed", 7),
    ("plugin-players.set-fly-speed", 7),
    ("plugin-players.set-op", 7),
    ("plugin-players.set-velocity", 7),
    ("plugin-players.damage", 7),
    ("plugin-players.kill", 7),
    ("plugin-players.set-fire-ticks", 7),
    ("plugin-players.effects", 7),
    ("plugin-players.add-effect", 7),
    ("plugin-players.remove-effect", 7),
    ("plugin-players.clear-effects", 7),
    ("plugin-players.hide-player", 7),
    ("plugin-players.show-player", 7),
    ("plugin-players.can-see", 7),
    ("plugin-players.perform-command", 7),
    ("plugin-players.chat", 7),
    ("plugin-players.set-respawn-location", 7),
    ("plugin-players.inventory", 7),
    ("plugin-players.get-slot", 7),
    ("plugin-players.set-slot", 7),
    ("plugin-players.give", 7),
    ("plugin-players.remove-item", 7),
    ("plugin-players.clear-inventory", 7),
    ("plugin-players.ender-chest", 7),
    ("plugin-players.set-ender-slot", 7),
    ("plugin-players.set-held-slot", 7),
    ("plugin-players.cursor", 7),
    ("plugin-players.set-cursor", 7),
    ("plugin-players.close-inventory", 7),
    ("plugin-players.update-inventory", 7),
    ("plugin-worlds.worlds", 7),
    ("plugin-worlds.get", 7),
    ("plugin-worlds.get-block", 7),
    ("plugin-worlds.get-biome", 7),
    ("plugin-worlds.highest-block-y", 7),
    ("plugin-worlds.get-block-data", 7),
    ("plugin-worlds.merge-block-data", 7),
    ("plugin-worlds.set-blocks", 7),
    ("plugin-worlds.is-chunk-loaded", 7),
    ("plugin-worlds.loaded-chunks", 7),
    ("plugin-worlds.set-chunk-forced", 7),
    ("plugin-worlds.set-time", 7),
    ("plugin-worlds.set-storm", 7),
    ("plugin-worlds.set-thundering", 7),
    ("plugin-worlds.set-spawn", 7),
    ("plugin-worlds.set-difficulty", 7),
    ("plugin-worlds.game-rule", 7),
    ("plugin-worlds.set-game-rule", 7),
    ("plugin-worlds.strike-lightning", 7),
    ("plugin-worlds.explode", 7),
    ("plugin-worlds.drop-item", 7),
    ("plugin-worlds.play-sound", 7),
    ("plugin-worlds.spawn-particle", 7),
    ("plugin-worlds.border", 7),
    ("plugin-worlds.set-border", 7),
    ("plugin-worlds.save", 7),
    ("plugin-entities.get", 7),
    ("plugin-entities.by-uuid", 7),
    ("plugin-entities.in-world", 7),
    ("plugin-entities.nearby", 7),
    ("plugin-entities.by-tag", 7),
    ("plugin-entities.spawn", 7),
    ("plugin-entities.teleport", 7),
    ("plugin-entities.remove", 7),
    ("plugin-entities.set-velocity", 7),
    ("plugin-entities.set-custom-name", 7),
    ("plugin-entities.set-glowing", 7),
    ("plugin-entities.set-invulnerable", 7),
    ("plugin-entities.set-silent", 7),
    ("plugin-entities.set-gravity", 7),
    ("plugin-entities.set-ai", 7),
    ("plugin-entities.damage", 7),
    ("plugin-entities.set-health", 7),
    ("plugin-entities.add-tag", 7),
    ("plugin-entities.remove-tag", 7),
    ("plugin-entities.add-passenger", 7),
    ("plugin-entities.eject", 7),
    ("plugin-entities.get-nbt", 7),
    ("plugin-entities.merge-nbt", 7),
    ("plugin-items.parse", 7),
    ("plugin-items.meta", 7),
    ("plugin-items.with-meta", 7),
    ("plugin-items.get-tag", 7),
    ("plugin-items.set-tag", 7),
    ("plugin-items.similar", 7),
    ("plugin-items.max-stack", 7),
    ("plugin-items.display-name", 7),
    ("plugin-items.registry", 7),
    ("plugin-inventories.create", 7),
    ("plugin-inventories.size", 7),
    ("plugin-inventories.get", 7),
    ("plugin-inventories.set", 7),
    ("plugin-inventories.contents", 7),
    ("plugin-inventories.set-contents", 7),
    ("plugin-inventories.add", 7),
    ("plugin-inventories.clear", 7),
    ("plugin-inventories.set-title", 7),
    ("plugin-inventories.open", 7),
    ("plugin-inventories.viewers", 7),
    ("plugin-inventories.open-inventory", 7),
    ("plugin-inventories.destroy", 7),
    ("plugin-scheduler.run-later", 7),
    ("plugin-scheduler.cancel", 7),
    ("plugin-scheduler.cancel-all", 7),
    ("plugin-scheduler.is-queued", 7),
    ("plugin-scheduler.current-tick", 7),
    ("plugin-permissions.has", 7),
    ("plugin-permissions.is-set", 7),
    ("plugin-permissions.set", 7),
    ("plugin-permissions.unset", 7),
    ("plugin-permissions.clear", 7),
    ("plugin-permissions.effective", 7),
    ("plugin-permissions.declare", 7),
    ("plugin-scoreboards.set-sidebar", 7),
    ("plugin-scoreboards.clear-sidebar", 7),
    ("plugin-scoreboards.set-name-tag", 7),
    ("plugin-scoreboards.clear-name-tag", 7),
    ("plugin-scoreboards.objectives", 7),
    ("plugin-scoreboards.add-objective", 7),
    ("plugin-scoreboards.remove-objective", 7),
    ("plugin-scoreboards.set-display-slot", 7),
    ("plugin-scoreboards.get-score", 7),
    ("plugin-scoreboards.set-score", 7),
    ("plugin-scoreboards.reset-score", 7),
    ("plugin-scoreboards.teams", 7),
    ("plugin-scoreboards.add-team", 7),
    ("plugin-scoreboards.remove-team", 7),
    ("plugin-scoreboards.join-team", 7),
    ("plugin-scoreboards.leave-team", 7),
    ("plugin-scoreboards.set-team-option", 7),
    ("plugin-bossbars.create", 7),
    ("plugin-bossbars.set-title", 7),
    ("plugin-bossbars.set-progress", 7),
    ("plugin-bossbars.set-color", 7),
    ("plugin-bossbars.set-style", 7),
    ("plugin-bossbars.set-flags", 7),
    ("plugin-bossbars.add-player", 7),
    ("plugin-bossbars.remove-player", 7),
    ("plugin-bossbars.players", 7),
    ("plugin-bossbars.remove", 7),
    ("plugin-messaging.register-incoming", 7),
    ("plugin-messaging.unregister-incoming", 7),
    ("plugin-messaging.send", 7),
    ("plugin-files.read", 7),
    ("plugin-files.write", 7),
    ("plugin-files.append", 7),
    ("plugin-files.delete", 7),
    ("plugin-files.exists", 7),
    ("plugin-files.list", 7),
    ("plugin-files.bundled", 7),
    ("plugin-files.save-default", 7),
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
