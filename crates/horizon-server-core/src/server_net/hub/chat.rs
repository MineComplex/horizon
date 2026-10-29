//! Chat on the server side, as ServerGamePacketListenerImpl and PlayerList
//! handle it without signed chat (players sign in through MineComplex, not
//! with Mojang's profile keys): the acknowledged window, and messages relayed
//! with each connection's numbering.
use super::{HostEvent, Hub};
use horizon_protocol::{
    packets::play::{self, Clientbound as C},
    secure_chat::{self, Body, DecodeError, Signature, Unpacked},
    text::{literal, translatable},
};
use horizon_world::nbt::{JavaString, Tag};

/// Now, in epoch milliseconds (Instant.now()).
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// StringUtil.isAllowedChatCharacter.
fn allowed(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

/// A translatable message in red.
fn red(key: &str) -> Tag {
    let mut t = translatable(key, &[]);
    if let Tag::Compound(map) = &mut t {
        map.insert(
            JavaString::from("color"),
            Tag::String(JavaString::from("red")),
        );
    }
    t
}

/// PlayerChatMessage as the server relays it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub sender: u128,
    /// The link's index in the sender's chain (0 when unsigned).
    pub index: i32,
    pub signature: Option<Signature>,
    pub content: String,
    /// Epoch milliseconds.
    pub timestamp: i64,
    pub salt: i64,
    pub last_seen: Vec<Signature>,
    /// What clients show instead of the signed text (withUnsignedContent).
    pub unsigned_content: Option<Tag>,
}

impl ChatMessage {
    /// The message a chain let through: a signed one keeps its body;
    /// PlayerChatMessage.unsigned takes the time now, no salt, nothing seen.
    pub fn new(sender: u128, unpacked: Unpacked, body: &Body, now: i64) -> Self {
        let (index, signature, timestamp, salt, last_seen) = match unpacked {
            Unpacked::Unsigned => (0, None, now, 0, Vec::new()),
            Unpacked::Signed { index, signature } => (
                index,
                Some(signature),
                body.timestamp,
                body.salt,
                body.last_seen.to_vec(),
            ),
        };
        Self {
            sender,
            index,
            signature,
            content: body.content.to_owned(),
            timestamp,
            salt,
            last_seen,
            unsigned_content: None,
        }
    }
    /// hasExpiredServer.
    pub fn expired(&self, now: i64) -> bool {
        now > self.timestamp + secure_chat::EXPIRES_AFTER_SERVER
    }
    /// PlayerList.verifyChatTrusted: signed and not expired.
    pub fn trusted(&self, now: i64) -> bool {
        self.signature.is_some() && !self.expired(now)
    }
}

impl Hub {
    /// Component.getString in the server's language, for the log.
    pub(crate) fn english(&self, tag: &Tag) -> String {
        match &self.network.shared.config.log {
            Some(log) => horizon_protocol::text::plain(tag, &log.lang),
            None => horizon_protocol::text::plain(tag, &Default::default()),
        }
    }

    /// tryHandleChat: an illegal character disconnects; with chat hidden a
    /// message (not a command) is refused. Whether to go on.
    pub fn try_handle_chat(&mut self, i: usize, message: &str, command: bool) -> bool {
        let conn = self.players[i].conn;
        if !message.chars().all(allowed) {
            self.kick(
                conn,
                translatable("multiplayer.disconnect.illegal_characters", &[]),
            );
            return false;
        }
        // ChatVisiblity.HIDDEN.
        if !command && self.players[i].info.chat_visibility == 2 {
            self.players[i].send(&C::SystemChat(play::SystemChat {
                content: red("chat.disabled.options"),
                overlay: false,
            }));
            return false;
        }
        true
    }

    /// unpackAndApplyLastSeen: the acknowledged messages, or a disconnect.
    pub fn apply_last_seen(
        &mut self,
        i: usize,
        update: &play::LastSeenUpdate,
        events: &mut Vec<HostEvent>,
    ) -> Option<Vec<Signature>> {
        let g = &mut self.players[i];
        match g
            .last_seen
            .apply_update(update.offset, update.acknowledged, update.checksum)
        {
            Ok(seen) => Some(seen),
            // Vanilla logs the ValidationException's stack trace after the line.
            Err(_) => {
                events.push(HostEvent::Error(format!(
                    "Failed to validate message acknowledgements from {}",
                    g.name
                )));
                let conn = g.conn;
                self.kick(
                    conn,
                    translatable("multiplayer.disconnect.chat_validation_failed", &[]),
                );
                None
            }
        }
    }

    /// handleChatAck.
    pub(super) fn chat_ack(&mut self, i: usize, offset: i32, events: &mut Vec<HostEvent>) {
        let g = &mut self.players[i];
        if g.last_seen.apply_offset(offset).is_err() {
            events.push(HostEvent::Error(format!(
                "Failed to validate message acknowledgement offset from {}",
                g.name
            )));
            let conn = g.conn;
            self.kick(
                conn,
                translatable("multiplayer.disconnect.chat_validation_failed", &[]),
            );
        }
    }

    /// handleMessageDecodeFailure: logged, and the reason in red for the player.
    pub fn decode_failure(&mut self, i: usize, e: DecodeError, events: &mut Vec<HostEvent>) {
        let reason = red(e.key());
        events.push(HostEvent::Warn(format!(
            "Failed to update secure chat state for {}: '{}'",
            self.players[i].name,
            self.english(&reason)
        )));
        self.players[i].send(&C::SystemChat(play::SystemChat {
            content: reason,
            overlay: false,
        }));
    }

    /// handleChat: the acknowledgements, then the message through the
    /// sender's chain, then to everyone.
    pub(super) fn handle_chat(&mut self, i: usize, c: play::Chat, events: &mut Vec<HostEvent>) {
        let Some(last_seen) = self.apply_last_seen(i, &c.last_seen, events) else {
            return;
        };
        if !self.try_handle_chat(i, &c.message, false) {
            return;
        }
        let now = now_millis();
        let body = Body {
            content: &c.message,
            timestamp: c.timestamp,
            salt: c.salt,
            last_seen: &last_seen,
        };
        let g = &self.players[i];
        let (uuid, name) = (g.uuid, g.name.clone());
        // No chat is signed: players sign in through MineComplex, not with
        // Mojang's profile keys (PlayerChatMessage.unsigned).
        let message = ChatMessage::new(uuid, Unpacked::Unsigned, &body, now);
        if self.intercept_chat {
            events.push(HostEvent::ChatPending {
                conn: self.players[i].conn,
                message: Box::new(message),
            });
            return;
        }
        let bound = play::ChatTypeBound {
            chat_type: self.chat_type,
            name: self.player_display_name(uuid, &name),
            target: None,
        };
        let line = format!("<{name}> {}", message.content);
        self.broadcast_chat(&message, &bound, line, events);
        events.push(HostEvent::PlayerChat {
            sender: uuid,
            text: message.content,
        });
    }

    /// A held chat message ([`HostEvent::ChatPending`]) sent on as vanilla
    /// would have sent it, signed when it was.
    pub fn relay_chat(&mut self, message: ChatMessage) -> Vec<HostEvent> {
        let mut events = Vec::new();
        let Some(g) = self.players.iter().find(|g| g.uuid == message.sender) else {
            return events;
        };
        let (uuid, name) = (g.uuid, g.name.clone());
        let bound = play::ChatTypeBound {
            chat_type: self.chat_type,
            name: self.player_display_name(uuid, &name),
            target: None,
        };
        let line = format!("<{name}> {}", message.content);
        self.broadcast_chat(&message, &bound, line, &mut events);
        events.push(HostEvent::PlayerChat {
            sender: uuid,
            text: message.content,
        });
        events
    }

    /// PlayerList.broadcastChatMessage: logged, then sent to every player.
    pub fn broadcast_chat(
        &mut self,
        message: &ChatMessage,
        bound: &play::ChatTypeBound,
        line: String,
        events: &mut Vec<HostEvent>,
    ) {
        let secure = message.trusted(now_millis());
        events.push(HostEvent::Chat { line, secure });
        for k in 0..self.players.len() {
            if self.players[k].placed {
                self.send_chat(k, message, bound);
            }
        }
    }

    /// ServerPlayer.sendChatMessage (players who show chat) and
    /// sendPlayerChatMessage: numbered for the connection, the last seen
    /// packed against its cache; a signed message joins the cache and the
    /// messages the player must acknowledge.
    pub fn send_chat(&mut self, k: usize, message: &ChatMessage, bound: &play::ChatTypeBound) {
        let g = &mut self.players[k];
        if g.info.chat_visibility != 0 {
            return;
        }
        let last_seen = message
            .last_seen
            .iter()
            .map(|s| g.signatures.pack(s))
            .collect();
        g.send(&C::PlayerChat(play::PlayerChat {
            global_index: g.chat_index,
            sender: message.sender,
            index: message.index,
            signature: message.signature.map(|s| s.to_vec()),
            content: message.content.clone(),
            timestamp: message.timestamp,
            salt: message.salt,
            last_seen,
            unsigned_content: message.unsigned_content.clone(),
            filter: (0, Vec::new()),
            chat_type: bound.clone(),
        }));
        g.chat_index += 1;
        if let Some(signature) = &message.signature {
            g.signatures.push(&message.last_seen, Some(signature));
            g.last_seen.add_pending(signature);
            if g.last_seen.tracked_count() > 4096 {
                let conn = g.conn;
                self.kick(
                    conn,
                    translatable("multiplayer.disconnect.too_many_pending_chats", &[]),
                );
            }
        }
    }

    /// sendChatMessage of a system message: a disguised chat, to players
    /// who show chat.
    fn send_disguised(&self, k: usize, message: Tag, chat_type: play::ChatTypeBound) {
        let g = &self.players[k];
        if g.info.chat_visibility == 0 {
            g.send(&C::DisguisedChat(play::DisguisedChat {
                message,
                chat_type,
            }));
        }
    }

    /// collectUnsignedArguments: each of the command's message arguments
    /// (`parsed`, by name) as an unsigned message, whatever signatures the
    /// client sent (no chat is signed here).
    pub fn signed_arguments(
        &mut self,
        i: usize,
        _p: &play::ChatCommandSigned,
        parsed: &[(String, String)],
        _last_seen: &[Signature],
        _events: &mut Vec<HostEvent>,
    ) -> Result<Vec<(String, ChatMessage)>, DecodeError> {
        let now = now_millis();
        let uuid = self.players[i].uuid;
        Ok(parsed
            .iter()
            .map(|(name, value)| {
                // SignedMessageBody.unsigned.
                let body = Body {
                    content: value,
                    timestamp: now,
                    salt: 0,
                    last_seen: &[],
                };
                (
                    name.clone(),
                    ChatMessage::new(uuid, Unpacked::Unsigned, &body, now),
                )
            })
            .collect())
    }

    /// A message command's chat (SayCommand, EmoteCommands): to everyone,
    /// bound to `chat_type` and the sender's name.
    pub fn broadcast_command_chat(
        &mut self,
        i: usize,
        chat_type: &str,
        message: Option<ChatMessage>,
        text: &str,
        events: &mut Vec<HostEvent>,
    ) {
        let (uuid, name) = (self.players[i].uuid, self.players[i].name.clone());
        let bound = play::ChatTypeBound {
            chat_type: self.chat_type_id(chat_type),
            name: self.player_display_name(uuid, &name),
            target: None,
        };
        let line = match chat_type {
            "say_command" => format!("[{name}] {text}"),
            _ => format!("* {name} {text}"),
        };
        match message {
            Some(message) => self.broadcast_chat(&message, &bound, line, events),
            // An unsigned command packet (no signing context):
            // PlayerChatMessage.system, a disguised chat.
            None => {
                events.push(HostEvent::Chat {
                    line,
                    secure: false,
                });
                for k in 0..self.players.len() {
                    if self.players[k].placed {
                        self.send_disguised(k, literal(text), bound.clone());
                    }
                }
            }
        }
    }

    /// MsgCommand.sendMessage to each target: the outgoing line for the
    /// sender, the incoming one for the target. Nothing is logged.
    pub fn whisper(
        &mut self,
        i: usize,
        targets: &[usize],
        message: Option<ChatMessage>,
        text: &str,
    ) {
        let (uuid, name) = (self.players[i].uuid, self.players[i].name.clone());
        let (outgoing, incoming) = (
            self.chat_type_id("msg_command_outgoing"),
            self.chat_type_id("msg_command_incoming"),
        );
        for &t in targets {
            let target = self.player_display_name(self.players[t].uuid, &self.players[t].name);
            let out = play::ChatTypeBound {
                chat_type: outgoing,
                name: self.player_display_name(uuid, &name),
                target: Some(target),
            };
            let inc = play::ChatTypeBound {
                chat_type: incoming,
                name: self.player_display_name(uuid, &name),
                target: None,
            };
            match &message {
                Some(m) => {
                    self.send_chat(i, m, &out);
                    self.send_chat(t, m, &inc);
                }
                None => {
                    self.send_disguised(i, literal(text), out);
                    self.send_disguised(t, literal(text), inc);
                }
            }
        }
    }

    /// A chat type's registry id.
    fn chat_type_id(&self, name: &str) -> i32 {
        self.sync
            .id("minecraft:chat_type", &format!("minecraft:{name}"))
            .unwrap_or(self.chat_type)
    }
}
