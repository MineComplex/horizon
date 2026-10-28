//! Commands (`CommandExecutor`, `TabCompleter`): a plugin's own `/name`.
use crate::server::{PlayerRef, Server};

/// `CommandSender`: the console or a player.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandSender {
    Console,
    Player(PlayerRef),
}
impl CommandSender {
    pub fn name(&self) -> &str {
        match self {
            Self::Console => "CONSOLE",
            Self::Player(p) => &p.name,
        }
    }
    pub fn player(&self) -> Option<&PlayerRef> {
        match self {
            Self::Console => None,
            Self::Player(p) => Some(p),
        }
    }
    /// `isOp`: the console always is.
    pub fn is_op(&self) -> bool {
        match self {
            Self::Console => true,
            Self::Player(p) => p.is_op(),
        }
    }
}

/// `onCommand`'s arguments: who, the label typed, the arguments, the server.
pub struct CommandContext<'a, 'b> {
    pub sender: CommandSender,
    pub label: String,
    pub args: Vec<String>,
    pub server: &'a mut Server<'b>,
}
impl CommandContext<'_, '_> {
    /// `sender.sendMessage`.
    pub fn reply(&mut self, text: &str) {
        match &self.sender {
            CommandSender::Console => self.server.log(text),
            CommandSender::Player(p) => {
                let p = p.clone();
                self.server.send_message(&p, text);
            }
        }
    }
}

pub(crate) type Executor = Box<dyn FnMut(&mut CommandContext<'_, '_>) -> bool + Send>;
pub(crate) type Completer = Box<dyn FnMut(&CommandSender, &[String]) -> Vec<String> + Send>;

/// A registered command (`PluginCommand`).
pub(crate) struct Registered {
    pub plugin: usize,
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    /// Needs an operator (`permission` with `default: op`) when `Some(level)`.
    pub op_level: Option<u8>,
    pub usage: Option<String>,
    pub executor: Executor,
    pub completer: Option<Completer>,
}
