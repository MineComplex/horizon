//! Command blocks: `CommandBlock`'s placement (`setPlacedBy`), its scheduled
//! tick (`execute`, `executeChain`), `BaseCommandBlock.performCommand`
//! through the command system with the block's output, and
//! `CommandBlockEditScreen` with what `handleSetCommandBlock` does with it.
use super::{commands::execute::Source, server_net::Network};
use crate::hc::{
    command_block::{self, CommandBlock, Mode},
    containers::BlockEntity,
    text::{self, Text},
    world::World,
};
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{self};
use horizon_world::{
    chunk::BlockState,
    level::flags,
    registry::{BlockRegistry, BlockStateId},
};

type Pos = (i32, i32, i32);

/// What `ServerboundSetCommandBlockPacket` carries.
struct CommandBlockEdit {
    pos: Pos,
    command: String,
    track_output: bool,
    mode: Mode,
    conditional: bool,
    auto: bool,
}

fn state_of(registry: &BlockRegistry, state: BlockStateId) -> Option<&BlockState> {
    registry.state(state)
}
fn facing_step(state: &BlockState) -> [i32; 3] {
    match state.properties.get("facing").map(String::as_str) {
        Some("down") => [0, -1, 0],
        Some("up") => [0, 1, 0],
        Some("south") => [0, 0, 1],
        Some("west") => [-1, 0, 0],
        Some("east") => [1, 0, 0],
        _ => [0, 0, -1],
    }
}
/// `Direction.toYRot`: south 0, west 90, north 180, east (and up/down) 270.
fn facing_yaw(state: &BlockState) -> f32 {
    match state.properties.get("facing").map(String::as_str) {
        Some("south") => 0.0,
        Some("west") => 90.0,
        Some("north") => 180.0,
        _ => 270.0,
    }
}
fn conditional(state: &BlockState) -> bool {
    state
        .properties
        .get("conditional")
        .is_some_and(|c| c == "true")
}
fn offset(pos: Pos, step: [i32; 3]) -> Pos {
    (pos.0 + step[0], pos.1 + step[1], pos.2 + step[2])
}
fn command_block_mut(world: &mut World, pos: Pos) -> Option<&mut CommandBlock> {
    match world.block_entities.get_mut(&pos)? {
        BlockEntity::CommandBlock(c) => Some(c),
        _ => None,
    }
}

/// `CommandBlockEntity.markConditionMet`: a conditional block needs the
/// command block behind it to have succeeded.
pub(super) fn mark_condition_met(world: &mut World, registry: &BlockRegistry, pos: Pos) -> bool {
    let Some(state) = state_of(registry, world.block(pos.0, pos.1, pos.2)) else {
        return false;
    };
    let mut met = true;
    if conditional(state) {
        let back = facing_step(state).map(|v| -v);
        let behind = offset(pos, back);
        let behind_block = state_of(registry, world.block(behind.0, behind.1, behind.2))
            .is_some_and(|s| Mode::of_block(&s.name).is_some());
        met = behind_block
            && matches!(world.block_entities.get(&behind),
                Some(BlockEntity::CommandBlock(c)) if c.success_count > 0);
    }
    if let Some(c) = command_block_mut(world, pos) {
        c.condition_met = met;
    }
    met
}

/// `CommandBlockEntity.setPowered` from `CommandBlock.setPoweredAndUpdate`
/// (the level's neighbour hook): the previous flag and `isAutomatic`; a
/// rising signal that will tick the block checks its condition.
pub(super) fn set_powered(
    world: &mut World,
    registry: &BlockRegistry,
    pos: Pos,
    powered: bool,
) -> Option<(bool, bool)> {
    let mode = state_of(registry, world.block(pos.0, pos.1, pos.2))
        .and_then(|s| Mode::of_block(&s.name))?;
    let c = command_block_mut(world, pos)?;
    let (was, auto) = (c.powered, c.auto);
    c.powered = powered;
    if command_block::power_schedules(mode, was, powered, auto) {
        mark_condition_met(world, registry, pos);
    }
    Some((was, auto))
}

/// `getAnalogOutputSignal`: the success count.
pub(super) fn analog_signal(world: &World, pos: Pos) -> Option<i32> {
    match world.block_entities.get(&pos)? {
        BlockEntity::CommandBlock(c) => Some(c.success_count),
        _ => None,
    }
}

impl crate::game::Server<'_> {
    /// `Player.canUseGameMasterBlocks`: creative with game master commands.
    pub(super) fn can_use_game_master_blocks(&self) -> bool {
        self.creative && self.world_data.registries.permission >= 2
    }
    fn command_block(&self, pos: Pos) -> Option<&CommandBlock> {
        match self.world.block_entities.get(&pos)? {
            BlockEntity::CommandBlock(c) => Some(c),
            _ => None,
        }
    }
    fn block_state_at(&self, pos: Pos) -> Option<&'static BlockState> {
        let registry = self.pack.assets.registry;
        state_of(registry, self.world.block(pos.0, pos.1, pos.2))
    }
    fn schedule_command_block(&mut self, pos: Pos) {
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        self.with_level(|b, level, ticks| b.schedule_tick(level, ticks, at, 1));
    }
    /// `CommandBlockEntity.scheduleTick`.
    fn schedule_command_block_tick(&mut self, pos: Pos) {
        mark_condition_met(&mut self.world, self.pack.assets.registry, pos);
        self.schedule_command_block(pos);
    }
    fn update_output_signal(&mut self, pos: Pos) {
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        self.with_level(|b, level, ticks| b.update_neighbour_for_output_signal(level, ticks, at));
    }
    /// `CommandBlock.setPlacedBy`: unless the item carried block entity data
    /// (`defaults` false), output tracking follows `send_command_feedback` and
    /// the block's own automatic flag; then the block takes the signal around it.
    pub(super) fn command_block_placed(&mut self, pos: Pos, defaults: bool) {
        let feedback = self.world_data.rules.bool("send_command_feedback");
        let Some(name) = self.block_state_at(pos).map(|s| s.name.as_str()) else {
            return;
        };
        let auto = command_block::automatic_block(name);
        if defaults && let Some(c) = command_block_mut(&mut self.world, pos) {
            c.track_output = feedback;
            c.auto = auto;
        }
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        let signal = self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
        let registry = self.pack.assets.registry;
        if let Some((was, auto)) = set_powered(&mut self.world, registry, pos, signal)
            && signal
            && !was
            && !auto
            && !command_block::automatic_block(name)
        {
            self.schedule_command_block(pos);
        }
    }
    /// `CommandBlock.tick`.
    pub(super) fn command_block_tick(&mut self, pos: BlockPos) {
        let key = (pos.x, pos.y, pos.z);
        let Some(state) = self.block_state_at(key) else {
            return;
        };
        let Some(mode) = Mode::of_block(&state.name) else {
            return;
        };
        let Some(was_met) = self.command_block(key).map(|c| c.condition_met) else {
            return;
        };
        if mode == Mode::Auto {
            mark_condition_met(&mut self.world, self.pack.assets.registry, key);
        }
        match command_block::tick_action(mode, was_met, conditional(state)) {
            Some(true) => self.command_block_execute(key, state),
            Some(false) => {
                if let Some(c) = command_block_mut(&mut self.world, key) {
                    c.success_count = 0;
                }
            }
            None => {}
        }
        if let Some(c) = self.command_block(key)
            && command_block::repeats(mode, c.powered, c.auto)
        {
            self.schedule_command_block(key);
        }
        self.update_output_signal(key);
    }
    /// `CommandBlock.execute`: the command (or no successes without one),
    /// then the chain in front.
    fn command_block_execute(&mut self, pos: Pos, state: &BlockState) {
        let has_command = self
            .command_block(pos)
            .is_some_and(|c| !c.command.is_empty());
        if has_command {
            self.perform_command_block(pos);
        } else if let Some(c) = command_block_mut(&mut self.world, pos) {
            c.success_count = 0;
        }
        self.execute_chain(pos, facing_step(state));
    }
    /// `CommandBlock.executeChain`: the chain command blocks in front, each
    /// facing on to the next, up to `max_command_sequence_length`.
    fn execute_chain(&mut self, start: Pos, mut step: [i32; 3]) {
        let registry = self.pack.assets.registry;
        let limit = self.world_data.rules.int("max_command_sequence_length");
        let mut remaining = limit;
        let mut pos = start;
        while remaining > 0 {
            remaining -= 1;
            pos = offset(pos, step);
            let Some(state) = self.block_state_at(pos) else {
                break;
            };
            if Mode::of_block(&state.name) != Some(Mode::Sequence) {
                break;
            }
            let Some(c) = self.command_block(pos) else {
                break;
            };
            if c.powered || c.auto {
                if mark_condition_met(&mut self.world, registry, pos) {
                    if !self.perform_command_block(pos) {
                        break;
                    }
                    self.update_output_signal(pos);
                } else if conditional(state)
                    && let Some(c) = command_block_mut(&mut self.world, pos)
                {
                    c.success_count = 0;
                }
            }
            step = facing_step(state);
        }
        if remaining <= 0 {
            log::warn!(
                "Command Block chain tried to execute more than {} steps!",
                limit.max(0)
            );
        }
    }
    /// `BaseCommandBlock.performCommand`: runs the command as the block, at
    /// game master level, counting the successful results. False when it
    /// already ran this game tick.
    fn perform_command_block(&mut self, pos: Pos) -> bool {
        let game_time = self.world.game_time;
        let Some(c) = command_block_mut(&mut self.world, pos) else {
            return false;
        };
        if c.last_execution == game_time {
            return false;
        }
        if c.command.eq_ignore_ascii_case("Searge") {
            c.last_output = Some(Text::literal("#itzlipofutzli"));
            c.success_count = 1;
            return true;
        }
        c.success_count = 0;
        let command = c.command.clone();
        if self.world_data.rules.bool("command_blocks_work") && !command.is_empty() {
            if let Some(c) = command_block_mut(&mut self.world, pos) {
                c.last_output = None;
            }
            let Some(state) = self.block_state_at(pos) else {
                return false;
            };
            let source = Source {
                entity: None,
                pos: [
                    f64::from(pos.0) + 0.5,
                    f64::from(pos.1) + 0.5,
                    f64::from(pos.2) + 0.5,
                ],
                rot: [facing_yaw(state), 0.0],
                eyes: false,
                silent: false,
                store: None,
                dimension: self.dim,
                level: None,
                block: Some(pos),
            };
            // performPrefixedCommand: a leading slash is allowed.
            let line = command.strip_prefix('/').unwrap_or(&command).to_owned();
            let outer = self.cmds.results.replace(Vec::new());
            let level = std::mem::replace(&mut self.world_data.registries.permission, 2);
            self.with_source(source, |g| g.run_command(&line));
            self.world_data.registries.permission = level;
            let results = std::mem::replace(&mut self.cmds.results, outer).unwrap_or_default();
            let successes = results.iter().filter(|(success, _)| *success).count();
            if let Some(c) = command_block_mut(&mut self.world, pos) {
                c.success_count = successes as i32;
            }
        }
        if let Some(c) = command_block_mut(&mut self.world, pos) {
            c.last_execution = if c.update_last_execution {
                game_time
            } else {
                -1
            };
        }
        true
    }
    /// `CloseableCommandBlockSource.sendSystemMessage`: the block's last
    /// output, stamped with the time. Nothing when it doesn't track output
    /// (`CommandSource.NULL`).
    pub(super) fn command_block_output(&mut self, pos: Pos, message: Text) {
        if let Some(c) = command_block_mut(&mut self.world, pos)
            && c.track_output
        {
            let time = chrono::Local::now().format("[%H:%M:%S] ").to_string();
            c.last_output = Some(Text::literal(time).append(message));
        }
    }
    /// `CommandSourceStack.sendSuccess` from a command block: its output when
    /// `send_command_feedback`, and `broadcastToAdmins` when
    /// `command_block_output`.
    pub(super) fn command_block_success(&mut self, pos: Pos, message: Text) {
        let Some(name) = self
            .command_block(pos)
            .filter(|c| c.track_output)
            .map(CommandBlock::name)
        else {
            return;
        };
        let feedback = self.world_data.rules.bool("send_command_feedback");
        if feedback {
            self.command_block_output(pos, message.clone());
        }
        if feedback && self.world_data.rules.bool("command_block_output") {
            let broadcast = Text::translate("chat.type.admin", &[name, message])
                .with_color(text::GRAY)
                .italic();
            self.system_message(broadcast);
        }
    }
    /// `ServerGamePacketListenerImpl.handleSetCommandBlock` for a player: the
    /// block is edited only by a creative player with game master commands.
    pub(super) fn net_set_command_block(
        &mut self,
        net: &mut Network,
        conn: u32,
        p: play::SetCommandBlock,
    ) {
        let Some(g) = net.hub.player(conn) else {
            return;
        };
        let allowed = g.creative() && g.permission >= 2;
        let edit = CommandBlockEdit {
            pos: (p.pos[0], p.pos[1], p.pos[2]),
            command: p.command,
            track_output: p.flags & 1 != 0,
            mode: Mode::from_ordinal(p.mode),
            conditional: p.flags & 2 != 0,
            auto: p.flags & 4 != 0,
        };
        self.as_player(net, conn, |game| {
            if allowed {
                game.apply_command_block(&edit);
                // BaseCommandBlock.onUpdated: the clients get the new command.
                game.mark_block_entity_updated(edit.pos);
            } else {
                game.system_message(Text::translate("advMode.notAllowed", &[]));
            }
        });
    }
    /// What `handleSetCommandBlock` does once the player may edit the block.
    fn apply_command_block(&mut self, s: &CommandBlockEdit) {
        let pos = s.pos;
        let registry = self.pack.assets.registry;
        let Some(current) = self.block_state_at(pos) else {
            return;
        };
        let Some(old_mode) = Mode::of_block(&current.name) else {
            return;
        };
        if self.command_block(pos).is_none() {
            return;
        }
        // The mode is the block, the conditional flag its state; the block
        // entity stays.
        let mut wanted = current.clone();
        wanted.name = s.mode.block().to_owned();
        wanted
            .properties
            .insert("conditional".into(), s.conditional.to_string());
        if let Ok(id) = registry.resolve(&wanted)
            && wanted != *current
        {
            let entity = self.world.block_entities.take(&pos);
            let at = BlockPos::new(pos.0, pos.1, pos.2);
            self.with_level(|b, level, ticks| {
                b.set_block(level, ticks, at, id, flags::UPDATE_CLIENTS)
            });
            if let Some(entity) = entity {
                self.world.block_entities.put(pos, entity);
            }
            let _ = self.process_level_events();
        }
        let command = s.command.clone();
        let Some(c) = command_block_mut(&mut self.world, pos) else {
            return;
        };
        c.set_command(&command);
        c.track_output = s.track_output;
        if !s.track_output {
            c.last_output = None;
        }
        // CommandBlockEntity.setAutomatic, then onModeSwitch.
        let (was_auto, powered) = (c.auto, c.powered);
        c.auto = s.auto;
        if !was_auto && s.auto && !powered && s.mode != Mode::Sequence {
            self.schedule_command_block_tick(pos);
        }
        if old_mode != s.mode && s.mode == Mode::Auto && (powered || s.auto) {
            self.schedule_command_block_tick(pos);
        }
        if !command.is_empty() {
            let key = if self.world_data.rules.bool("command_blocks_work") {
                "advMode.setCommand.success"
            } else {
                "advMode.setCommand.disabled"
            };
            self.system_message(Text::translate(key, &[Text::literal(command)]));
        }
    }
}
