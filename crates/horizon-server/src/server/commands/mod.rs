//! Running commands: `Commands.performPrefixedCommand` and the messages to a source, for the
//! commands ported so far (see [`tree`]), as Leaf 26.3 has them.
//!
//! This is a child module of the runtime (`#[path]` in `runtime.rs`) because commands act on the
//! whole server: a command's body is a `match` over its [`Key`] in `Host::run`, with `&mut Server`
//! (the tick thread owns it; nothing is shared or stored as a closure).
//!
//! Flow of a line (`performPrefixedCommand`): one leading `/` is dropped; the line is parsed
//! against the tree for the source's permission level (a command the source may not use does not
//! parse); `finishParsing` turns a parse error into Leaf's unknown-command message (Paper's
//! `UnknownCommandEvent` has no listeners; Leaf's `unknown-command` setting is "default"); a good
//! parse is queued in an [`ExecutionContext`] with the `maxCommandSequenceLength` and
//! `maxCommandForks` game rule defaults (65536 each: game rules are not ported) and run.
//!
//! The console's source is `MinecraftServer.createCommandSourceStack`: owner permissions, the
//! respawn level at the respawn position. Its feedback goes through `MinecraftServer.sendSystemMessage`
//! (`System chat: <text>` in the log) and is broadcast, where a command asks, to the operators
//! online (`broadcastToAdmins`).
//!
//! Selectors in player arguments: `@a`, `@p` and `@r` (bare) resolve against the online players;
//! `@e`/`@n`/`@s` give vanilla's "only players" error; selectors with `[options]` are refused
//! (`argument.entity.selector.not_allowed`) until entities exist.
//!
//! Stand-ins: no user cache file (`usercache.json`): a name resolves to an online player, else, as
//! `CachedUserNameToIdResolver` does without a Mojang lookup, to the offline profile in offline mode
//! and to "no player" in proxy online mode; Paper's command events (`ServerCommandEvent` and the
//! like) have no listeners; the permission of a command is the vanilla level only (no Bukkit
//! permission nodes).

mod admin;
mod lists;
pub mod locate;
mod source;
mod tick;
mod tree;
mod world;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

pub use source::{CommandSource, Origin};
pub use tree::{Cmd, Key, Suggest, build_tree};

use super::*;
use crate::azalea::chat::FormattedText;
use crate::azalea::chat::style::ChatFormatting;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::exec::{ExecSource, ExecutionContext, Runner};
use crate::command::network::{PlayerCommands, SuggestionProvider};
use crate::command::{CommandContext, CommandError, CommandTree};
use source::{failure_style, literal, translatable, with_formatting};

/// `GameRules.MAX_COMMAND_SEQUENCE_LENGTH` and `MAX_COMMAND_FORKS` defaults.
const MAX_COMMAND_SEQUENCE_LENGTH: usize = 65536;
const MAX_COMMAND_FORKS: usize = 65536;

/// The server's command state.
pub struct State {
    /// Taken out while a line runs (the commands borrow the server mutably), put back after.
    tree: Option<CommandTree<Key>>,
    /// `ServerLevel.noSave` of every level (`/save-off`): the periodic `level.dat` save is skipped.
    pub no_save: bool,
    /// Names the console resolved (a stand-in for `usercache.json`), by lowercase name.
    users: HashMap<String, crate::server::user_lists::NameAndId>,
    /// Every line of feedback the console printed, when a test asks for it.
    pub log: Option<Vec<String>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            tree: Some(build_tree()),
            no_save: false,
            users: HashMap::new(),
            log: None,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a source may use a node (`CommandNode.canUse`: the requirement).
fn can_use(source: &CommandSource, key: Key) -> bool {
    match key {
        Key::Permission(level) => source.has_level(level),
        _ => true,
    }
}

/// `StringUtil.filterText`: control characters and `§` are dropped.
fn filter_text(text: &str) -> String {
    text.chars()
        .filter(|&c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}')
        .collect()
}

/// Leaf's `getVanillaUnknownCommandMessage`: the error's message, then (with a position) a new line
/// with up to ten characters before it, the rest underlined red and `<--[HERE]`. Positions count
/// UTF-16 units, as Java's `substring` does.
pub fn unknown_command_message(error: &CommandError) -> FormattedText {
    let mut out = with_formatting(literal(""), &[ChatFormatting::Red]);
    out.append(error.message());
    if let Some(input) = error.input() {
        let units: Vec<u16> = input.encode_utf16().collect();
        let cursor = error.cursor_utf16().min(units.len());
        let slice = |from: usize, to: usize| String::from_utf16_lossy(&units[from..to]);
        let mut context = with_formatting(literal(""), &[ChatFormatting::Gray]);
        if cursor > 10 {
            context.append(literal("..."));
        }
        context.append(literal(filter_text(&slice(
            cursor.saturating_sub(10),
            cursor,
        ))));
        if cursor < units.len() {
            context.append(with_formatting(
                literal(filter_text(&slice(cursor, units.len()))),
                &[ChatFormatting::Red, ChatFormatting::Underline],
            ));
        }
        context.append(with_formatting(
            translatable("command.context.here", vec![]),
            &[ChatFormatting::Red, ChatFormatting::Italic],
        ));
        out.append(literal("\n"));
        out.append(context);
    }
    out
}

impl Server {
    /// `MinecraftServer.createCommandSourceStack`.
    pub fn console_source(&self) -> CommandSource {
        let level = self.world.respawn_level();
        let pos = self.world.levels[level].overrides.respawn.pos;
        CommandSource::console(
            level,
            [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)],
        )
    }

    /// A console line (`DedicatedServer.handleConsoleInputs`): `performPrefixedCommand` for the
    /// server's source.
    pub fn run_console_command(&mut self, line: &str) {
        let source = self.console_source();
        self.perform_prefixed_command(source, line);
    }

    /// `Commands.performPrefixedCommand`. The result is what the command reported to the source's
    /// callback (`success`, `result`); `None` when the line did not parse.
    pub fn perform_prefixed_command(
        &mut self,
        source: CommandSource,
        input: &str,
    ) -> Option<(bool, i32)> {
        let input = input.strip_prefix('/').unwrap_or(input);
        let tree = self.commands.tree.take().expect("the command tree");
        let mut outcome = None;
        {
            let parse = tree.parse(input, |key| can_use(&source, key));
            match parse.context_chain() {
                // `finishParsing`: the message of `UnknownCommandEvent`, sent without the failure style.
                Err(error) => self.send_failure(&source, unknown_command_message(&error), false),
                Ok(chain) => {
                    let mut context = ExecutionContext::new(
                        MAX_COMMAND_SEQUENCE_LENGTH.max(1),
                        MAX_COMMAND_FORKS,
                    );
                    let slot = context.new_slot();
                    context.queue_initial_command(
                        input,
                        &chain,
                        source.with_callback(slot.clone()),
                        crate::command::exec::ResultCallback::Empty,
                    );
                    context.run(&mut Host { server: self });
                    outcome = context.outcome(&slot);
                }
            }
        }
        self.commands.tree = Some(tree);
        outcome
    }

    /// `CommandSourceStack.sendSuccess`.
    pub fn send_success(
        &mut self,
        source: &CommandSource,
        message: FormattedText,
        broadcast: bool,
    ) {
        // `acceptsSuccess` is true for the console and for a player.
        let send = !source.silent;
        let inform = broadcast && !source.silent;
        if send {
            self.send_system_message(source, &message);
        }
        if inform {
            self.broadcast_to_admins(source, message);
        }
    }

    /// `CommandSourceStack.sendFailure`.
    pub fn send_failure(
        &mut self,
        source: &CommandSource,
        message: FormattedText,
        with_style: bool,
    ) {
        if source.silent {
            return;
        }
        let message = if with_style {
            failure_style(message)
        } else {
            message
        };
        self.send_system_message(source, &message);
    }

    /// `CommandSource.sendSystemMessage`: the server logs it, a player gets a system chat packet.
    fn send_system_message(&mut self, source: &CommandSource, message: &FormattedText) {
        match &source.origin {
            Origin::Console => self.log_console(message),
            Origin::Player(id) => {
                if let Some(j) = self.players.iter().find(|j| j.player.profile.id == *id)
                    && let Ok(packet) = cb::system_chat(message, false)
                {
                    j.peer.conn.send(packet);
                }
            }
        }
    }

    /// `MinecraftServer.sendSystemMessage`.
    fn log_console(&mut self, message: &FormattedText) {
        let text = message.to_string();
        if let Some(log) = &mut self.commands.log {
            log.push(text.clone());
        }
        tracing::info!("System chat: {text}");
    }

    /// `CommandSourceStack.broadcastToAdmins`: operators online hear `chat.type.admin` (the
    /// `send_command_feedback` game rule is on; the log of other sources' commands needs players
    /// as sources, which come with `execute`).
    fn broadcast_to_admins(&mut self, source: &CommandSource, message: FormattedText) {
        let broadcast = with_formatting(
            translatable(
                "chat.type.admin",
                vec![
                    PrimitiveOrComponent::FormattedText(literal(source.name.clone())),
                    PrimitiveOrComponent::FormattedText(message),
                ],
            ),
            &[ChatFormatting::Gray, ChatFormatting::Italic],
        );
        let Ok(packet) = cb::system_chat(&broadcast, false) else {
            return;
        };
        for j in &self.players {
            let own = matches!(&source.origin, Origin::Player(id) if *id == j.player.profile.id);
            if !own && self.user_lists.is_op(&(&j.player.profile).into()) {
                j.peer.conn.send(packet.clone());
            }
        }
    }

    /// `Commands.sendCommands`: the commands this player may use, by its permission level.
    pub fn commands_packet(
        &self,
        profile: &GameProfile,
    ) -> crate::command::network::CommandsPacket {
        let level = self.permission_level(profile);
        let tree = self.commands.tree.as_ref().expect("the command tree");
        PlayerCommands::new(
            tree,
            |key| match key {
                Key::Permission(needed) => level >= needed,
                _ => true,
            },
            true,
        )
        .to_packet(
            |key| matches!(key, Key::Permission(needed) if needed > 0),
            |_| SuggestionProvider::AskServer,
        )
    }

    /// `PlayerList.sendPlayerPermissionLevel`: the level's entity event, then the commands again.
    fn send_permission_level(&self, id: Uuid) {
        let Some(j) = self.players.iter().find(|j| j.player.profile.id == id) else {
            return;
        };
        let level = self.permission_level(&j.player.profile);
        // `EntityEvent.PERMISSION_LEVEL_ALL` (24) .. `_OWNERS` (28).
        j.peer
            .conn
            .send(cb::entity_event(j.player.entity_id, 24 + level as i8));
        j.peer
            .conn
            .send(cb::commands(&self.commands_packet(&j.player.profile)));
    }
}

/// The server as the executor's runner.
struct Host<'a> {
    server: &'a mut Server,
}

impl<'p> Runner<'p, Key, CommandSource> for Host<'_> {
    fn run(
        &mut self,
        key: Key,
        context: &CommandContext<'p, Key>,
        source: &CommandSource,
    ) -> Result<i32, CommandError> {
        let Key::Command(command) = key else {
            return Ok(0);
        };
        let server = &mut *self.server;
        match command {
            Cmd::SaveAll => admin::save_all(server, source, false),
            Cmd::SaveAllFlush => admin::save_all(server, source, true),
            Cmd::SaveOff => admin::save_off(server, source),
            Cmd::SaveOn => admin::save_on(server, source),
            Cmd::Seed => admin::seed(server, source),
            Cmd::Stop => admin::stop(server, source),
            Cmd::List => admin::list(server, source, false),
            Cmd::ListUuids => admin::list(server, source, true),
            Cmd::LocateStructure => locate::structure(server, source, context),
            Cmd::LocateBiome => locate::biome(server, source, context),
            Cmd::LocatePoi => locate::poi(server, source, context),
            Cmd::WhitelistOn => lists::whitelist_on(server, source),
            Cmd::WhitelistOff => lists::whitelist_off(server, source),
            Cmd::WhitelistList => lists::whitelist_list(server, source),
            Cmd::WhitelistReload => lists::whitelist_reload(server, source),
            Cmd::WhitelistAdd => lists::whitelist_add(server, source, context),
            Cmd::WhitelistRemove => lists::whitelist_remove(server, source, context),
            Cmd::Op => lists::op(server, source, context),
            Cmd::Deop => lists::deop(server, source, context),
            Cmd::DifficultyQuery => world::difficulty_query(server, source),
            Cmd::DifficultySet(id) => world::difficulty_set(server, source, id),
            Cmd::TimeSetTicks => world::time_set(server, source, context),
            Cmd::TimeSetMarker => world::time_set_marker(server, source, context),
            Cmd::TimeAdd => world::time_add(server, source, context),
            Cmd::TimePause => world::time_pause(server, source, true),
            Cmd::TimeResume => world::time_pause(server, source, false),
            Cmd::TimeRate => world::time_rate(server, source, context),
            Cmd::TimeQueryTime => world::time_query(server, source),
            Cmd::TimeQueryGameTime => world::time_query_game_time(server, source),
            Cmd::WeatherClear => world::weather(server, source, world::Weather::Clear, -1),
            Cmd::WeatherRain => world::weather(server, source, world::Weather::Rain, -1),
            Cmd::WeatherThunder => world::weather(server, source, world::Weather::Thunder, -1),
            Cmd::WeatherClearFor => {
                world::weather(server, source, world::Weather::Clear, duration(context))
            }
            Cmd::WeatherRainFor => {
                world::weather(server, source, world::Weather::Rain, duration(context))
            }
            Cmd::WeatherThunderFor => {
                world::weather(server, source, world::Weather::Thunder, duration(context))
            }
            Cmd::TickQuery => tick::query(server, source),
            Cmd::TickRate => tick::rate(server, source, context),
            Cmd::TickStep => tick::step(server, source, 1),
            Cmd::TickStepTime => tick::step(server, source, time_argument(context)),
            Cmd::TickStepStop => tick::step_stop(server, source),
            Cmd::TickSprintTime => tick::sprint(server, source, time_argument(context)),
            Cmd::TickSprintStop => tick::sprint_stop(server, source),
            Cmd::TickFreeze => tick::freeze(server, source, true),
            Cmd::TickUnfreeze => tick::freeze(server, source, false),
        }
    }

    fn modify(
        &mut self,
        _key: Key,
        _context: &CommandContext<'p, Key>,
        source: &CommandSource,
    ) -> Result<Vec<CommandSource>, CommandError> {
        // No redirect of the tree has a modifier yet (`/execute` is not ported): the aliases only
        // redirect.
        Ok(vec![source.clone()])
    }

    /// `CommandSourceStack.handleError`: a forked chain stays quiet, else the raw message in red.
    fn handle_error(&mut self, source: &CommandSource, error: &CommandError, forked: bool) {
        if !forked {
            self.server.send_failure(source, error.message(), true);
        }
    }
}

fn time_argument(context: &CommandContext<'_, Key>) -> i32 {
    context.time("time").unwrap_or(0)
}

fn duration(context: &CommandContext<'_, Key>) -> i32 {
    context.time("duration").unwrap_or(0)
}
