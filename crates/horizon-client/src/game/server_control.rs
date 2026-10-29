//! What a client and the test tools ask a server on its control endpoint
//! (`horizon-server --control-socket PATH`), beside `server-status` and
//! `console` (dedicated/status.rs).
//!
//! A test drives a real client (`horizon-client --server ... --server-control
//! PATH`); what only the server knows (its mobs, its blocks, the world's clock,
//! its saves) the client asks here, as `as NAME WORDS...` for the player NAME:
//!
//! - `command TEXT`: run a command as the player, at the console's permission,
//!   in the player's level; answers with the feedback it got
//! - `give ITEM [COUNT]`, `time TICKS`, `mobs RADIUS`, `hurtlog`, `simulation`,
//!   `server-hold on|off`, `save`, `save-all`
//! - `setblock`, `fill`, `getblock`, `container`: the block commands
//!   (game/blockctl.rs) in the player's level
//! - `lan open [PORT] [commands] [force] | lan status | lan close`: the port the
//!   server listens on, and its players (what `horizon-ctl lan` reports)
//!
//! And for the tools: `world` (the level's name and the port), `exit-with PID`
//! (stop when that process ends: the client a test started the server for).
use horizon_client::Result;
use serde_json::Value;

/// The client's side: a test's client asks its server (`--server-control`).
impl crate::game::Sim<'_> {
    /// Ask the server this client plays on `as NAME words`: None when the
    /// client was not given the server's control endpoint.
    pub(super) fn ask_server(&self, words: &str) -> Option<Result<Value>> {
        let path = self.options.server_control.as_ref()?;
        let name = self.remote.as_ref().map(|r| r.profile_name.as_str());
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return Some(Err("not on the server yet".into()));
        };
        Some(ask(path, &format!("as {name} {words}")))
    }
    /// The world's name on the server this client plays on, once asked.
    pub(super) fn server_world_name(&self) -> Option<String> {
        if let Some(name) = self.server_world.get() {
            return Some(name.clone());
        }
        let path = self.options.server_control.as_ref()?;
        let name = ask(path, "world").ok()?["world"].as_str()?.to_owned();
        Some(self.server_world.get_or_init(|| name).clone())
    }
}

/// One request to a server's control endpoint; its `error` as an error.
fn ask(path: &std::path::Path, line: &str) -> Result<Value> {
    let reply = horizon_client::control::request(path, line)
        .map_err(|e| format!("the server's control endpoint {}: {e}", path.display()))?;
    let value: Value = serde_json::from_str(reply.trim())?;
    match value.get("error").and_then(Value::as_str) {
        Some(error) => Err(error.to_owned().into()),
        None => Ok(value),
    }
}
