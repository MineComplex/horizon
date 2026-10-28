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

