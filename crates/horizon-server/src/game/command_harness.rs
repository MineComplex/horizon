//! Headless command runner for `tools/test_command_exec_parity.py`: with
//! `HORIZON_COMMAND_SCRIPT` and `HORIZON_COMMAND_OUTPUT` set, `horizon-server`
//! runs the script once a player is on (a headless `horizon-client` with
//! `HORIZON_STATIONARY_CLIENT`), then stops. Each script line is a JSON
//! operation: `cmd` runs a
//! command as the player (Commands.performPrefixedCommand) and records every
//! system message it produced, as styled spans, with the results reported to
//! the player's `CommandResultCallback`; `tick` advances the game; `settle`
//! waits until the terrain and entities around the player are loaded;
//! `portal` sends the player through the nether portal it stands in.
use crate::hc::{Result, text::Text};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// A message as `[text, style]` runs with adjacent equal styles merged; the
/// style lists colour, bold, italic, underlined, strikethrough, obfuscated
/// and the suggested command.
pub(super) fn spans(text: &Text) -> Value {
    let mut out: Vec<(String, Value)> = Vec::new();
    for span in &text.spans {
        if span.text.is_empty() {
            continue;
        }
        let s = &span.style;
        let style = json!([
            s.color,
            s.bold == Some(true),
            s.italic == Some(true),
            s.underlined == Some(true),
            s.strikethrough == Some(true),
            s.obfuscated == Some(true),
        ]);
        match out.last_mut() {
            Some((t, last)) if *last == style => t.push_str(&span.text),
            _ => out.push((span.text.clone(), style)),
        }
    }
    Value::Array(out.into_iter().map(|(t, s)| json!([t, s])).collect())
}

impl crate::game::Sim<'_> {
    pub(crate) fn run_command_harness(&mut self, script: &Path, output: &Path) -> Result<()> {
        let deadline = Instant::now()
            + Duration::from_secs(
                std::env::var("HORIZON_HARNESS_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(600),
            );
        // Join/configuration uses the normal hub, but never advances the world.
        while !self
            .network
            .as_ref()
            .is_some_and(|h| h.hub.players.iter().any(|g| g.placed && g.loaded))
        {
            self.tick_server(true)?;
            if Instant::now() > deadline {
                return Err("command harness: owner did not connect".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.harness_owner(|g| g.harness_settle(deadline))?;
        let mut out = String::new();
        for line in std::fs::read_to_string(script)?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let op: Value = serde_json::from_str(line)?;
            match op["op"].as_str().unwrap_or("") {
                "cmd" => {
                    let command = op["cmd"].as_str().unwrap_or("");
                    self.chat_capture = Some(Vec::new());
                    self.cmds.results = Some(Vec::new());
                    self.harness_owner(|g| {
                        g.as_console(|g| {
                            g.run_command(command.strip_prefix('/').unwrap_or(command))
                        });
                        Ok(())
                    })?;
                    let messages = self.chat_capture.take().unwrap_or_default();
                    let results = self.cmds.results.take().unwrap_or_default();
                    if op["record"].as_bool() != Some(false) {
                        let record = json!({
                            "cmd": command,
                            "messages": messages.iter().map(spans).collect::<Vec<_>>(),
                            "results": results.iter().map(|(s, v)| json!([s, v])).collect::<Vec<_>>(),
                        });
                        out.push_str(&record.to_string());
                        out.push('\n');
                    }
                }
                "tick" => {
                    self.chat_capture = Some(Vec::new());
                    for _ in 0..op["n"].as_u64().unwrap_or(1) {
                        self.tick_server(false)?;
                    }
                    let messages = self.chat_capture.take().unwrap_or_default();
                    if op["record"].as_bool() == Some(true) {
                        let record = json!({
                            "cmd": "#tick",
                            "messages": messages.iter().map(spans).collect::<Vec<_>>(),
                            "results": [],
                        });
                        out.push_str(&record.to_string());
                        out.push('\n');
                    }
                }
                "settle" => {
                    // Finish portal arrivals using connection-only passes too.
                    while !self.server_trips.is_empty() {
                        self.tick_server(true)?;
                        if Instant::now() > deadline {
                            return Err("command harness: portal did not arrive".into());
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    self.harness_owner(|g| g.harness_settle(deadline))?;
                }
                "portal" => self.harness_portal()?,
                other => return Err(format!("command harness: unknown op {other}").into()),
            }
        }
        std::fs::write(output, out)?;
        Ok(())
    }
}

impl crate::game::Sim<'_> {
    /// The owner's authoritative body and level, using the same swaps as a
    /// server command. Always put them back, also when a script operation fails.
    pub(crate) fn harness_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let mut net = self.network.take().ok_or("command harness: no hub")?;
        // The script's player: the first on the server.
        let owner = net.hub.players.first().map_or(0, |g| g.uuid);
        let player = net.hub.players.iter().find(|g| g.uuid == owner && g.placed);
        let Some(player) = player else {
            self.network = Some(net);
            return Err("command harness: owner is not connected".into());
        };
        let (conn, dim, back) = (
            player.conn,
            super::Dim::from_id(&player.dimension),
            self.dim,
        );
        if let Err(error) = self.enter_level(Some(&mut net), dim) {
            self.network = Some(net);
            return Err(error);
        }
        let swap = self
            .enter_player(&mut net, conn)
            .expect("the owner is not already acting");
        self.network = Some(net);
        let result = f(self);
        let mut net = self
            .network
            .take()
            .expect("the operation puts the hub back");
        self.leave_player(&mut net, swap);
        net.hub.sync_all_effects();
        self.run_level_teleports(&mut net);
        let restored = self.enter_level(Some(&mut net), back);
        self.network = Some(net);
        restored?;
        result
    }
}

impl crate::game::Sim<'_> {
    /// The player's portal process finished in the nether portal it stands
    /// in (the oracle's player is not ticked, so neither is the wait).
    pub(crate) fn harness_portal(&mut self) -> Result<()> {
        let (conn, plan) = self.harness_owner(|g| {
            let p = g
                .player
                .as_ref()
                .ok_or("command harness: portal without a player")?;
            let entry = horizon_core::position::BlockPos::containing(p.position);
            let block = g.world.block(entry.x, entry.y, entry.z);
            if &*g.res.facts().get(block).name != "minecraft:nether_portal" {
                return Err("command harness: the player is not in a nether portal".into());
            }
            let plan = g.portal_plan(
                entry,
                p.position,
                p.dimensions.width,
                p.dimensions.height,
                true,
            );
            Ok((
                g.net_acting.ok_or("command harness: no acting owner")?,
                plan,
            ))
        })?;
        // Start travel after restoring the owner's body: change_level updates
        // its hub record, which leave_player would otherwise overwrite.
        let owner = self
            .network
            .as_ref()
            .and_then(|h| h.hub.players.first())
            .map_or(0, |g| g.uuid);
        self.in_player_level(owner, |g| {
            let mut net = g.network.take().ok_or("command harness: no hub")?;
            g.start_server_trip(&mut net, conn, plan.dim, Some(plan));
            g.network = Some(net);
            Ok(())
        })
    }
}

impl crate::game::Sim<'_> {
    /// Load terrain until the player stands in lit chunks.
    pub(crate) fn harness_settle(&mut self, deadline: Instant) -> Result<()> {
        loop {
            self.stream_world();
            if let Some(p) = &self.player {
                let (cx, cz) = (
                    (p.position.x.floor() as i32) >> 4,
                    (p.position.z.floor() as i32) >> 4,
                );
                // Finish requested terrain before inspecting it, so worker
                // timing cannot change which command positions are loaded.
                if !self.streaming.busy()
                    && (-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz)))
                {
                    // The oracle's settle also waits for the entities saved
                    // in those chunks (areEntitiesActuallyLoadedAndTicking):
                    // after a dimension change they come back from the save.
                    self.spawn_loaded_entities();
                    return Ok(());
                }
            }
            if std::env::var_os("HORIZON_HARNESS_DEBUG").is_some() {
                eprintln!(
                    "settle: player {:?} center {:?} chunks {} pending {}",
                    self.player.as_ref().map(|p| p.position),
                    self.streaming.center,
                    self.world.chunks.len(),
                    self.streaming.pending()
                );
            }
            if Instant::now() > deadline {
                return Err("command harness: terrain did not load".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
