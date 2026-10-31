//! `/save-all [flush]`, `/save-off`, `/save-on`, `/seed`, `/stop`, `/list [uuids]`
//! (`SaveAllCommand`, `SaveOffCommand`, `SaveOnCommand`, `SeedCommand`, `StopCommand`,
//! `ListPlayersCommand`).

use super::source::{string_arg, text_arg, translatable, with_formatting};
use super::{CommandSource, Server};
use crate::azalea::chat::style::ChatFormatting;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::CommandError;

fn error(key: &'static str) -> CommandError {
    CommandError::new(key, Vec::new())
}

fn message(key: &str) -> crate::azalea::chat::FormattedText {
    translatable(key, Vec::new())
}

impl Server {
    /// `MinecraftServer.saveEverything(silent, flush, force)`: the players, then `level.dat` and
    /// every level's chunks. True when some level was saved (`saveAllChunks`' result).
    pub(super) fn save_everything(&mut self, silent: bool, flush: bool, _force: bool) -> bool {
        for joined in &self.players {
            self.save_player(&joined.player);
        }
        self.world.save_all_chunks(silent, flush);
        !self.world.levels.is_empty()
    }
}

/// `SaveAllCommand.saveAll`.
pub(super) fn save_all(
    server: &mut Server,
    source: &CommandSource,
    flush: bool,
) -> Result<i32, CommandError> {
    server.send_success(source, message("commands.save.saving"), false);
    if !server.save_everything(true, flush, true) {
        return Err(error("commands.save.failed"));
    }
    server.send_success(source, message("commands.save.success"), true);
    Ok(1)
}

/// `MinecraftServer.setAutoSave`: true when it changed anything. The flag is one for all levels (they
/// are only ever changed together here).
fn set_auto_save(server: &mut Server, enable: bool) -> bool {
    if server.commands.no_save == enable {
        server.commands.no_save = !enable;
        true
    } else {
        false
    }
}

pub(super) fn save_off(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    if !set_auto_save(server, false) {
        return Err(error("commands.save.alreadyOff"));
    }
    server.send_success(source, message("commands.save.disabled"), true);
    Ok(1)
}

pub(super) fn save_on(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    if !set_auto_save(server, true) {
        return Err(error("commands.save.alreadyOn"));
    }
    server.send_success(source, message("commands.save.enabled"), true);
    Ok(1)
}

/// `SeedCommand`: the level's seed, shown as a copyable `[seed]`; the result is `(int) seed`.
/// (Leaf's secure seed is off by default and not ported.)
pub(super) fn seed(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    let seed = server.world.levels[source.level].seed;
    // `ComponentUtils.copyOnClickText`: `[%s]` around the seed, green.
    let seed_text = with_formatting(
        translatable("chat.square_brackets", vec![string_arg(seed.to_string())]),
        &[ChatFormatting::Green],
    );
    server.send_success(
        source,
        translatable("commands.seed.success", vec![text_arg(seed_text)]),
        false,
    );
    Ok(seed as i32)
}

/// `StopCommand`.
pub(super) fn stop(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    server.send_success(source, message("commands.stop.stopping"), true);
    server.halt(0);
    Ok(1)
}

/// `ListPlayersCommand.format`: the players in join order, joined by ", ".
pub(super) fn list(
    server: &mut Server,
    source: &CommandSource,
    with_uuids: bool,
) -> Result<i32, CommandError> {
    let entries: Vec<String> = server
        .players
        .iter()
        .map(|j| {
            let profile = &j.player.profile;
            if with_uuids {
                translatable(
                    "commands.list.nameAndId",
                    vec![
                        string_arg(profile.name.clone()),
                        string_arg(profile.id.to_string()),
                    ],
                )
                .to_string()
            } else {
                profile.name.clone()
            }
        })
        .collect();
    let count = entries.len();
    let max_players = server.config.max_players;
    // The list is a literal "a, b, c" (`ComponentUtils.formatList` with the gray ", " separator).
    let list = with_formatting(super::source::literal(entries.join(", ")), &[]);
    server.send_success(
        source,
        translatable(
            "commands.list.players",
            vec![
                PrimitiveOrComponent::Integer(count as i32),
                PrimitiveOrComponent::Integer(max_players),
                text_arg(list),
            ],
        ),
        false,
    );
    Ok(count as i32)
}
