//! Control-socket block commands for scripted tests: `setblock`, `fill`,
//! `getblock` and `container`. The world is the server's: they run there
//! (`--server-control`); without it `setblock` and `fill` go out as the
//! player's commands, and `getblock` and `container` read the client's copy.
use horizon_client::Result;
use horizon_world::registry::BlockStateId;
use serde_json::{Value, json};

pub(super) const HELP: &[&str] = &[
    "setblock X Y Z BLOCK[props]    set a block with neighbour updates",
    "fill X1 Y1 Z1 X2 Y2 Z2 BLOCK   set a box of blocks",
    "getblock X Y Z                 block state at a position",
    "container X Y Z [ITEM COUNT]   list a container, or add items to it",
];

impl crate::game::Sim<'_> {
    fn state_text(&self, state: BlockStateId) -> String {
        let Some(s) = self.pack.assets.registry.state(state) else {
            return "?".into();
        };
        if s.properties.is_empty() {
            return s.name.clone();
        }
        let props: Vec<String> = s
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}[{}]", s.name, props.join(","))
    }
    /// Block commands; None when `words[0]` is not one of them.
    pub(super) fn block_command(&mut self, words: &[&str]) -> Result<Option<Value>> {
        let ours = matches!(
            words.first(),
            Some(&("setblock" | "fill" | "getblock" | "container"))
        );
        // A test's client asks its server (`--server-control`): the command
        // runs there, in the player's level, and the client hears of it like
        // any change.
        if ours && let Some(reply) = self.ask_server(&words.join(" ")) {
            return reply.map(Some);
        }
        // Another server's world changes only through its own commands, which
        // it checks as the player's; `getblock` and `container` read what this
        // client has of it.
        if matches!(words.first(), Some(&("setblock" | "fill"))) {
            self.send_chat(&format!("/{}", words.join(" ")));
            return Ok(Some(json!({ "ok": true, "sent": true })));
        }
        let int = |i: usize| -> Result<i32> {
            Ok(words
                .get(i)
                .ok_or_else(|| format!("missing argument {i}"))?
                .parse()?)
        };
        match words[0] {
            "getblock" => {
                let pos = [int(1)?, int(2)?, int(3)?];
                Ok(Some(
                    json!({ "block": self.state_text(self.world.block(pos[0], pos[1], pos[2])) }),
                ))
            }
            "container" => {
                let key = (int(1)?, int(2)?, int(3)?);
                if words.len() > 4 {
                    return Err("filling a container is the server's: --server-control".into());
                }
                let entity = self
                    .world
                    .block_entities
                    .get(&key)
                    .ok_or("no container there")?;
                let slots: Vec<Value> = entity
                    .clone()
                    .slots_mut()
                    .iter()
                    .map(|s| {
                        s.as_ref().map_or(
                            Value::Null,
                            |s| json!({ "item": &*s.item, "count": s.count }),
                        )
                    })
                    .collect();
                Ok(Some(json!({ "slots": slots })))
            }
            _ => Ok(None),
        }
    }
}
