//! `Server` as plugins see it: what they read, and the changes they ask for.
use std::fmt;

/// A level: `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`.
pub type World = String;

/// A block position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// `Location`: a level, a position and a facing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Location {
    pub world: World,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}
impl Location {
    pub fn new(world: &str, x: f64, y: f64, z: f64) -> Self {
        Self {
            world: world.to_owned(),
            x,
            y,
            z,
            ..Self::default()
        }
    }
    /// The block this location is in.
    pub fn block(&self) -> BlockPos {
        BlockPos::new(
            self.x.floor() as i32,
            self.y.floor() as i32,
            self.z.floor() as i32,
        )
    }
}

/// An online player, as an event or a read names them.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerRef {
    pub name: String,
    pub uuid: u128,
    pub location: Location,
    pub health: f32,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_mode: u8,
    /// The command permission level: 0 a player, 2 to 4 an operator.
    pub op_level: u8,
}
impl PlayerRef {
    /// The UUID as `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
    pub fn uuid_string(&self) -> String {
        uuid_string(self.uuid)
    }
    pub fn is_op(&self) -> bool {
        self.op_level >= 2
    }
}
impl fmt::Display for PlayerRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// A UUID's text form.
pub fn uuid_string(uuid: u128) -> String {
    let hex = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// An item stack: `minecraft:diamond` and a count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub item: String,
    pub count: u32,
}
impl ItemStack {
    pub fn new(item: &str, count: u32) -> Self {
        let item = if item.contains(':') {
            item.to_owned()
        } else {
            format!("minecraft:{item}")
        };
        Self { item, count }
    }
}

/// A change a plugin asked for, applied by the server once the handler or
/// command returns.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `Player.sendMessage`.
    Message { to: u128, text: String },
    /// `Bukkit.broadcastMessage`: every player and the console.
    Broadcast(String),
    /// A line in the server's log (`Plugin.getLogger().info`).
    Log { plugin: String, line: String },
    /// `Player.kick`.
    Kick { player: u128, reason: String },
    /// `Bukkit.dispatchCommand`: as the console, or as a player.
    Dispatch {
        player: Option<u128>,
        command: String,
    },
    /// `Block.setType` (a block state such as `minecraft:stone` or
    /// `minecraft:oak_stairs[facing=east]`).
    SetBlock {
        world: World,
        pos: BlockPos,
        state: String,
    },
    /// `Player.teleport`.
    Teleport { player: u128, to: Location },
    /// `PlayerInventory.addItem`.
    Give { player: u128, stack: ItemStack },
    /// `Player.setGameMode` (0 survival, 1 creative, 2 adventure, 3 spectator).
    GameMode { player: u128, mode: u8 },
}

/// What the server lets plugins read; the game implements it.
pub trait View {
    fn players(&self) -> Vec<PlayerRef>;
    /// The block state at a position, when its chunk is loaded.
    fn block(&self, world: &str, pos: BlockPos) -> Option<String>;
    /// The server's ticks so far.
    fn tick(&self) -> u64;
    /// `level-name`.
    fn world_name(&self) -> String;
    /// `Bukkit.getMaxPlayers`.
    fn max_players(&self) -> usize;
    /// The day time of the Overworld, 0..24000.
    fn time(&self) -> i64;
}

/// `Server` (`Bukkit`) as a handler or a command has it.
pub struct Server<'a> {
    view: &'a dyn View,
    plugin: &'a str,
    actions: &'a mut Vec<Action>,
}
impl<'a> Server<'a> {
    pub fn new(view: &'a dyn View, plugin: &'a str, actions: &'a mut Vec<Action>) -> Self {
        Self {
            view,
            plugin,
            actions,
        }
    }
    /// `getOnlinePlayers`.
    pub fn online_players(&self) -> Vec<PlayerRef> {
        self.view.players()
    }
    /// `getPlayerExact` (case-insensitive, as Paper's).
    pub fn player(&self, name: &str) -> Option<PlayerRef> {
        self.view
            .players()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
    pub fn player_by_uuid(&self, uuid: u128) -> Option<PlayerRef> {
        self.view.players().into_iter().find(|p| p.uuid == uuid)
    }
    /// `World.getBlockAt(...).getBlockData()`: None while the chunk isn't loaded.
    pub fn block(&self, world: &str, pos: BlockPos) -> Option<String> {
        self.view.block(world, pos)
    }
    pub fn tick(&self) -> u64 {
        self.view.tick()
    }
    pub fn world_name(&self) -> String {
        self.view.world_name()
    }
    pub fn max_players(&self) -> usize {
        self.view.max_players()
    }
    pub fn time(&self) -> i64 {
        self.view.time()
    }

    pub fn send_message(&mut self, player: &PlayerRef, text: &str) {
        self.actions.push(Action::Message {
            to: player.uuid,
            text: text.to_owned(),
        });
    }
    pub fn broadcast(&mut self, text: &str) {
        self.actions.push(Action::Broadcast(text.to_owned()));
    }
    /// The plugin's logger: `[Name] line` in the server's log.
    pub fn log(&mut self, line: &str) {
        self.actions.push(Action::Log {
            plugin: self.plugin.to_owned(),
            line: line.to_owned(),
        });
    }
    pub fn kick(&mut self, player: &PlayerRef, reason: &str) {
        self.actions.push(Action::Kick {
            player: player.uuid,
            reason: reason.to_owned(),
        });
    }
    /// As the console (`Bukkit.dispatchCommand(Bukkit.getConsoleSender(), ...)`).
    pub fn dispatch_command(&mut self, command: &str) {
        self.actions.push(Action::Dispatch {
            player: None,
            command: command.trim_start_matches('/').to_owned(),
        });
    }
    /// As this player, with their permission (`Player.performCommand`).
    pub fn perform_command(&mut self, player: &PlayerRef, command: &str) {
        self.actions.push(Action::Dispatch {
            player: Some(player.uuid),
            command: command.trim_start_matches('/').to_owned(),
        });
    }
    pub fn set_block(&mut self, world: &str, pos: BlockPos, state: &str) {
        let state = if state.contains(':') {
            state.to_owned()
        } else {
            format!("minecraft:{state}")
        };
        self.actions.push(Action::SetBlock {
            world: world.to_owned(),
            pos,
            state,
        });
    }
    pub fn teleport(&mut self, player: &PlayerRef, to: Location) {
        self.actions.push(Action::Teleport {
            player: player.uuid,
            to,
        });
    }
    pub fn give(&mut self, player: &PlayerRef, stack: ItemStack) {
        self.actions.push(Action::Give {
            player: player.uuid,
            stack,
        });
    }
    pub fn set_game_mode(&mut self, player: &PlayerRef, mode: u8) {
        self.actions.push(Action::GameMode {
            player: player.uuid,
            mode: mode.min(3),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_print_as_minecraft_does() {
        assert_eq!(
            uuid_string(0x069a79f444e94726a5befca90e38aaf5),
            "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
        assert_eq!(ItemStack::new("diamond", 2).item, "minecraft:diamond");
    }
}
