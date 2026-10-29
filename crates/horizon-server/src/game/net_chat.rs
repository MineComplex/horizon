//! A player's commands that carry chat: the unsigned and signed command
//! packets (ServerGamePacketListenerImpl.handleChatCommand and
//! handleSignedChatCommand), and the message commands whose text they sign
//! (SayCommand, EmoteCommands, MsgCommand), relayed by the hub as chat.
use super::server_net::Network;
use crate::hc::{
    command::{Arg, ArgType, NodeKind, ParseResults, vanilla},
    server_net::{
        data::component_nbt,
        hub::{HostEvent, chat::ChatMessage},
    },
    text::{self, Text},
};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The signing context of a command: each message argument as the chain
/// let it through, by argument name (CommandSigningContext.SignedArguments).
type Signed = Vec<(String, ChatMessage)>;

impl crate::game::Sim<'_> {
    /// CommandDispatcher.parse with the player's permission level.
    fn parse_as(&mut self, permission: u8, command: &str) -> ParseResults {
        let level = std::mem::replace(&mut self.world_data.registries.permission, permission);
        let parse = vanilla::game_dispatcher().parse(command, &self.world_data.registries);
        self.world_data.registries.permission = level;
        parse
    }
    /// SignableCommand.of: the command's message arguments, by name, with
    /// their text, not past a redirect to the root (`execute ... run`).
    pub(super) fn signable_arguments(
        &mut self,
        permission: u8,
        command: &str,
    ) -> Vec<(String, String)> {
        let parse = self.parse_as(permission, command);
        let nodes = &vanilla::game_dispatcher().nodes;
        let root = parse.context.root;
        let mut out = Vec::new();
        let mut context = Some(&parse.context);
        while let Some(c) = context {
            for &(node, (start, end)) in &c.nodes {
                if let NodeKind::Argument {
                    ty: ArgType::Message,
                    name,
                } = &nodes[node].kind
                    && let Some(text) = command.get(start..end)
                {
                    out.push((name.clone(), text.to_owned()));
                }
            }
            context = c.child.as_deref().filter(|child| child.root != root);
        }
        out
    }
    /// handleChatCommand (performUnsignedChatCommand): refused when the
    /// server enforces secure chat and the command has a message to sign.
    pub(super) fn player_chat_command(&mut self, net: &mut Network, conn: u32, command: &str) {
        let Some(i) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return;
        };
        if !net.hub.try_handle_chat(i, command, true) {
            return;
        }
        let permission = net.hub.players[i].permission;
        if net.hub.enforces_secure_chat()
            && !self.signable_arguments(permission, command).is_empty()
        {
            let name = net.hub.players[i].name.clone();
            self.net_log_event(HostEvent::Error(format!(
                "Received unsigned command packet from {name}, but the command requires signable arguments: {command}"
            )));
            let failure = Text::translate("chat.disabled.invalid_command_signature", &[])
                .with_color(text::RED);
            net.hub.players[i].send(&C::SystemChat(play::SystemChat {
                content: component_nbt(&failure.component()),
                overlay: false,
            }));
            return;
        }
        self.player_command(net, conn, command, None);
    }
    /// handleSignedChatCommand: the acknowledgements, then each message
    /// argument through the sender's chain, then the command with them.
    pub(super) fn player_signed_command(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: play::ChatCommandSigned,
    ) {
        let Some(i) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return;
        };
        let mut events = Vec::new();
        let mut signed = None;
        if let Some(last_seen) = net.hub.apply_last_seen(i, &p.last_seen, &mut events)
            && net.hub.try_handle_chat(i, &p.command, true)
        {
            let permission = net.hub.players[i].permission;
            let parsed = self.signable_arguments(permission, &p.command);
            match net
                .hub
                .signed_arguments(i, &p, &parsed, &last_seen, &mut events)
            {
                Ok(arguments) => signed = Some(arguments),
                Err(e) => net.hub.decode_failure(i, e, &mut events),
            }
        }
        for event in events {
            self.net_log_event(event);
        }
        if let Some(signed) = signed {
            self.player_command(net, conn, &p.command, Some(signed));
        }
    }
    /// SayCommand, EmoteCommands and MsgCommand for a player: chat to the
    /// players, signed when the player signed it (a disguised chat after an
    /// unsigned command packet). False when the command is another one, or
    /// one this can't run (selectors other than `@s` and `@a`), for the
    /// game's dispatcher to handle.
    pub(super) fn player_message_command(
        &mut self,
        net: &mut Network,
        conn: u32,
        command: &str,
        signed: Option<&Signed>,
    ) -> bool {
        let Some(i) = net.hub.players.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let parse = self.parse_as(net.hub.players[i].permission, command);
        let Ok((context, id)) = parse.executable() else {
            return false;
        };
        let argument = if id == "me" { "action" } else { "message" };
        let (Some(Arg::Str(text)), true) =
            (context.get(argument), matches!(id, "say" | "me" | "msg"))
        else {
            return false;
        };
        let message = signed.and_then(|s| {
            s.iter()
                .find(|(n, _)| n == argument)
                .map(|(_, m)| m.clone())
        });
        let mut events = Vec::new();
        if id == "msg" {
            let Some(Arg::Entity(selector)) = context.get("targets") else {
                return false;
            };
            let raw = context
                .args
                .iter()
                .rev()
                .find(|a| a.name == "targets")
                .and_then(|a| command.get(a.range.0..a.range.1));
            let players = &net.hub.players;
            let placed = |k: &usize| players[*k].placed;
            let targets: Vec<usize> = match (raw, &selector.player_name, selector.uuid) {
                (Some("@s"), ..) => vec![i],
                (Some("@a"), ..) => (0..players.len()).filter(placed).collect(),
                (_, Some(name), _) => (0..players.len())
                    .filter(|&k| placed(&k) && players[k].name.eq_ignore_ascii_case(name))
                    .collect(),
                (_, None, Some(uuid)) => (0..players.len())
                    .filter(|&k| placed(&k) && players[k].uuid == uuid)
                    .collect(),
                _ => return false,
            };
            if targets.is_empty() {
                let failure = Text::empty()
                    .append(Text::translate("argument.entity.notfound.player", &[]))
                    .with_color(text::RED);
                net.hub.players[i].send(&C::SystemChat(play::SystemChat {
                    content: component_nbt(&failure.component()),
                    overlay: false,
                }));
                return true;
            }
            net.hub.whisper(i, &targets, message, text);
        } else {
            let chat_type = if id == "say" {
                "say_command"
            } else {
                "emote_command"
            };
            net.hub
                .broadcast_command_chat(i, chat_type, message, text, &mut events);
        }
        for event in events {
            self.net_log_event(event);
        }
        true
    }
}
