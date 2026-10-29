//! The multiplayer server list from the control socket (the Multiplayer screen's list).
use super::Game;
use horizon_client::Result;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::mpsc::Sender};

pub(super) const HELP: &[&str] =
    &["servers                        the multiplayer server list (servers.dat): name and address"];

fn saves() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join("saves"))
}

impl crate::game::Sim<'_> {
    /// Some(reply) for these commands (Some(None): the reply comes later), None for others.
    pub(super) fn saves_control(
        &mut self,
        words: &[&str],
        _line: &str,
        _reply: &Sender<String>,
    ) -> Result<Option<Option<Value>>> {
        let value = match words[0] {
            "servers" => {
                let file = horizon_client::multiplayer::servers_file(&saves()?);
                let servers: Vec<Value> = horizon_protocol::servers::load(&file)
                    .iter()
                    .map(|s| json!({ "name": s.name, "address": s.ip }))
                    .collect();
                json!({ "servers": servers })
            }
            _ => return Ok(None),
        };
        Ok(Some(Some(value)))
    }
}
