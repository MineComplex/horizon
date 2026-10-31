//! `/whitelist`, `/op`, `/deop` (`WhitelistCommand`, `OpCommand`, `DeOpCommands`) over the user
//! lists, and the `game_profile` argument's resolution (`GameProfileArgument.Result.getNames`).

use super::source::{string_arg, text_arg, translatable};
use super::{CommandSource, Server};
use crate::command::{CommandContext, CommandError, ProfileTarget, SelectorKind};
use crate::network::Reason;
use crate::server::properties_file::PropertiesFile;
use crate::server::user_lists::{NameAndId, OpEntry, WhitelistEntry};

use super::tree::Key;

fn error(key: &'static str) -> CommandError {
    CommandError::new(key, Vec::new())
}

fn message(key: &str) -> crate::azalea::chat::FormattedText {
    translatable(key, Vec::new())
}

impl Server {
    /// `CachedUserNameToIdResolver.get(name)` as far as it goes without a Mojang lookup: the cache
    /// (names of earlier lookups), then the online players (whose profiles the real cache holds
    /// since they joined), then, in offline mode, the offline profile.
    fn resolve_name(&mut self, name: &str) -> Option<NameAndId> {
        let lower = name.to_lowercase();
        if let Some(user) = self.commands.users.get(&lower) {
            return Some(user.clone());
        }
        if let Some(j) = self
            .players
            .iter()
            .find(|j| j.player.profile.name.to_lowercase() == lower)
        {
            return Some((&j.player.profile).into());
        }
        // `lookupGameProfile`: a Mojang lookup in proxy online mode, which is not available here;
        // otherwise (and when the lookup finds nothing) `createUnknownProfile`.
        if self.config.proxy_online_mode {
            return None;
        }
        let user = NameAndId::offline(name);
        self.commands.users.insert(lower, user.clone());
        Some(user)
    }

    /// `GameProfileArgument.getGameProfiles`.
    fn profiles(
        &mut self,
        source: &CommandSource,
        target: &ProfileTarget,
    ) -> Result<Vec<NameAndId>, CommandError> {
        match target {
            ProfileTarget::Name(name) => self
                .resolve_name(name)
                .map(|user| vec![user])
                .ok_or_else(CommandError::player_unknown),
            ProfileTarget::Selector(kind) => {
                let players: Vec<(NameAndId, [f64; 3])> = self
                    .players
                    .iter()
                    .map(|j| ((&j.player.profile).into(), j.player.position))
                    .collect();
                let chosen: Vec<NameAndId> = match kind {
                    SelectorKind::AllPlayers => players.into_iter().map(|p| p.0).collect(),
                    SelectorKind::NearestPlayer => {
                        let at = source.position;
                        let distance = |p: &[f64; 3]| {
                            (0..3).map(|i| (p[i] - at[i]) * (p[i] - at[i])).sum::<f64>()
                        };
                        players
                            .into_iter()
                            .min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1)))
                            .map(|p| p.0)
                            .into_iter()
                            .collect()
                    }
                    SelectorKind::RandomPlayer => {
                        let n = players.len();
                        if n == 0 {
                            Vec::new()
                        } else {
                            vec![players[rand::random_range(0..n)].0.clone()]
                        }
                    }
                };
                if chosen.is_empty() {
                    Err(CommandError::no_players_found())
                } else {
                    Ok(chosen)
                }
            }
        }
    }

    /// `MinecraftServer.kickUnlistedPlayers`: with `enforce-whitelist` and the whitelist on, the
    /// players neither listed nor operators are kicked with Spigot's whitelist message.
    pub(super) fn kick_unlisted_players(&mut self) {
        if !(self.config.enforce_whitelist && self.config.white_list) {
            return;
        }
        let reason = Reason::Literal(self.config.whitelist_message.clone());
        for j in &mut self.players {
            let user: NameAndId = (&j.player.profile).into();
            if !self.user_lists.whitelist.contains(&user)
                && !self.user_lists.is_op(&user)
                && j.peer.closing.is_none()
            {
                j.peer.disconnect(reason.clone());
            }
        }
    }

    /// `DedicatedServer.setUsingWhitelist`: the setting, written back to `server.properties`
    /// (`DedicatedServerSettings.update`).
    fn set_using_whitelist(&mut self, using: bool) {
        self.config.white_list = using;
        let path = self.config.user_lists_dir.join("server.properties");
        match PropertiesFile::load(&path) {
            Ok(mut properties) => {
                properties.insert("white-list", using.to_string());
                if let Err(e) = properties.store(&path, "Minecraft server properties") {
                    tracing::warn!("Failed to save {}: {e}", path.display());
                }
            }
            Err(e) => tracing::warn!("Failed to save {}: {e}", path.display()),
        }
    }
}

fn targets(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<Vec<NameAndId>, CommandError> {
    match context.profile("targets") {
        Some(target) => server.profiles(source, &target.clone()),
        None => Ok(Vec::new()),
    }
}

pub(super) fn whitelist_on(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    if server.config.white_list {
        return Err(error("commands.whitelist.alreadyOn"));
    }
    server.set_using_whitelist(true);
    server.send_success(source, message("commands.whitelist.enabled"), true);
    server.kick_unlisted_players();
    Ok(1)
}

pub(super) fn whitelist_off(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    if !server.config.white_list {
        return Err(error("commands.whitelist.alreadyOff"));
    }
    server.set_using_whitelist(false);
    server.send_success(source, message("commands.whitelist.disabled"), true);
    Ok(1)
}

pub(super) fn whitelist_list(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    let names: Vec<String> = server
        .user_lists
        .whitelist
        .entries()
        .map(|e| e.user.name.clone())
        .collect();
    if names.is_empty() {
        server.send_success(source, message("commands.whitelist.none"), false);
    } else {
        server.send_success(
            source,
            translatable(
                "commands.whitelist.list",
                vec![
                    crate::azalea::chat::translatable_component::PrimitiveOrComponent::Integer(
                        names.len() as i32,
                    ),
                    string_arg(names.join(", ")),
                ],
            ),
            false,
        );
    }
    Ok(names.len() as i32)
}

/// `PlayerList.reloadWhiteList` is empty in 26.3: the command only reports, then kicks.
pub(super) fn whitelist_reload(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    server.send_success(source, message("commands.whitelist.reloaded"), true);
    server.kick_unlisted_players();
    Ok(1)
}

pub(super) fn whitelist_add(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let mut success = 0;
    for user in targets(server, source, context)? {
        if !server.user_lists.whitelist.contains(&user) {
            server
                .user_lists
                .whitelist
                .add(WhitelistEntry { user: user.clone() });
            server.send_success(
                source,
                translatable(
                    "commands.whitelist.add.success",
                    vec![text_arg(super::source::literal(user.name))],
                ),
                true,
            );
            success += 1;
        }
    }
    if success == 0 {
        return Err(error("commands.whitelist.add.failed"));
    }
    Ok(success)
}

pub(super) fn whitelist_remove(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let mut success = 0;
    for user in targets(server, source, context)? {
        if server.user_lists.whitelist.contains(&user) {
            server.user_lists.whitelist.remove(&user);
            server.send_success(
                source,
                translatable(
                    "commands.whitelist.remove.success",
                    vec![text_arg(super::source::literal(user.name))],
                ),
                true,
            );
            success += 1;
        }
    }
    if success == 0 {
        return Err(error("commands.whitelist.remove.failed"));
    }
    server.kick_unlisted_players();
    Ok(success)
}

/// `OpCommand.opPlayers`: `PlayerList.op` adds the entry with `op-permission-level` and the
/// player's current `bypassesPlayerLimit`, and tells the player of its new level.
pub(super) fn op(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let mut count = 0;
    for user in targets(server, source, context)? {
        if !server.user_lists.is_op(&user) {
            let entry = OpEntry {
                level: server.config.op_permission_level,
                bypasses_player_limit: server.user_lists.can_bypass_player_limit(&user),
                user: user.clone(),
            };
            server.user_lists.ops.add(entry);
            server.send_permission_level(user.id);
            count += 1;
            server.send_success(
                source,
                translatable("commands.op.success", vec![string_arg(user.name)]),
                true,
            );
        }
    }
    if count == 0 {
        return Err(error("commands.op.failed"));
    }
    Ok(count)
}

/// `DeOpCommands.deopPlayers`.
pub(super) fn deop(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let mut count = 0;
    for user in targets(server, source, context)? {
        if server.user_lists.is_op(&user) {
            if server.user_lists.ops.remove(&user) {
                server.send_permission_level(user.id);
            }
            count += 1;
            server.send_success(
                source,
                translatable("commands.deop.success", vec![string_arg(user.name)]),
                true,
            );
        }
    }
    if count == 0 {
        return Err(error("commands.deop.failed"));
    }
    server.kick_unlisted_players();
    Ok(count)
}
