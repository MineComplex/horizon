//! Chunk tickets: `TicketType`, `Ticket` and Moonrise's per-chunk `TicketSet`, and `ChunkLevel`'s
//! levels. Ticket types are a closed enum of the types something here adds; the others come with
//! their systems (`FORCED` with `/forceload`, `PORTAL` and `POST_TELEPORT` with dimension travel and
//! teleports, `ENDER_PEARL`, `DRAGON`, `SPAWN_SEARCH` with the spawn finder, Paper's plugin types
//! never: no plugins). Their counters (`FORCED`, "keep dimension active") come with the systems that
//! read them.

/// `ChunkLevel.FULL_CHUNK_LEVEL` (checked: `LeafChunkTicketsOracle`).
pub const FULL_LEVEL: u8 = 33;
/// `ChunkLevel.BLOCK_TICKING_LEVEL`.
pub const BLOCK_TICKING_LEVEL: u8 = 32;
/// `ChunkLevel.ENTITY_TICKING_LEVEL`.
pub const ENTITY_TICKING_LEVEL: u8 = 31;
/// `ChunkLevel.MAX_LEVEL` (33 + the generation pyramid's radius around a full chunk, 11): a chunk
/// above it has no holder.
pub const MAX_LEVEL: u8 = 44;
/// Moonrise's `LOADED_TICKET_LEVEL`, `ChunkLevel.byStatus(EMPTY)`: holds a chunk without
/// asking for anything but its load.
pub const LOADED_LEVEL: u8 = 44;

/// `TicketStorage.addTicketWithRadius`'s level: `byStatus(FULL) - radius`.
pub const fn level_for_radius(radius: u8) -> u8 {
    FULL_LEVEL - radius
}

/// `FullChunkStatus`, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FullChunkStatus {
    Inaccessible,
    Full,
    BlockTicking,
    EntityTicking,
}

impl FullChunkStatus {
    /// `ChunkLevel.fullStatus`.
    pub const fn from_level(level: u8) -> Self {
        if level <= ENTITY_TICKING_LEVEL {
            Self::EntityTicking
        } else if level <= BLOCK_TICKING_LEVEL {
            Self::BlockTicking
        } else if level <= FULL_LEVEL {
            Self::Full
        } else {
            Self::Inaccessible
        }
    }
}

/// The ticket types in use (see the module docs). Vanilla's are checked against the 26.3 jar;
/// Paper's and Moonrise's are derived from their sources (they are not in the vanilla jar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TicketType {
    /// Vanilla `player_spawn`: 20 ticks, around a prepared spawn (`PrepareSpawnTask`).
    PlayerSpawn,
    /// Vanilla `unknown`: 1 tick; Moonrise keeps a removed ticket's level one more tick with it.
    Unknown,
    /// Moonrise's `chunk_system:chunk_load` (`ChunkTaskScheduler.CHUNK_LOAD`): no timeout, held while
    /// a requested load (`loadChunksAsync`) runs.
    ChunkLoad,
    /// Moonrise's `chunk_system:player_ticket`: no timeout, a player chunk loader's chunks.
    Player,
    /// Moonrise's `chunk_system:player_ticket_delayed`: Paper's `chunks.delay-chunk-unloads-by`
    /// in ticks (at least 1; default 10 s = 200), what a player leaves behind.
    PlayerDelayed,
    /// Paper's `chunk_system:player_join`: 100 ticks on the joining player's chunk.
    PlayerJoin,
}

impl TicketType {
    /// `timeout()`; `PlayerDelayed` is configured (`ChunkHolderManager::new`).
    pub const fn timeout(self, player_unload_delay: i64) -> i64 {
        match self {
            Self::PlayerSpawn => 20,
            Self::Unknown => 1,
            Self::ChunkLoad | Self::Player => 0,
            Self::PlayerDelayed => player_unload_delay,
            Self::PlayerJoin => 5 * 20,
        }
    }
}

/// `ChunkHolderManager.NO_TIMEOUT_MARKER`.
pub(super) const NO_TIMEOUT: i64 = i64::MIN;

/// A ticket. Identity is (level, type, identifier), its order in a chunk's set; `id` is the
/// identifier of types that have one (a player loader's id, a load request's id), 0 otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub kind: TicketType,
    pub level: u8,
    pub id: u64,
    /// Ticks left, or [`NO_TIMEOUT`].
    pub(super) remove_delay: i64,
}

impl Ticket {
    fn key(&self) -> (u8, TicketType, u64) {
        (self.level, self.kind, self.id)
    }

    /// `ChunkHolderManager.tickTicket`: true once expired.
    fn tick(&mut self) -> bool {
        if self.remove_delay == NO_TIMEOUT {
            return false;
        }
        self.remove_delay -= 1;
        self.remove_delay <= 0
    }
}

/// Moonrise's `TicketSet`: one chunk's tickets, sorted; the first holds the chunk's level.
#[derive(Clone, Debug, Default)]
pub(super) struct TicketSet(Vec<Ticket>);

impl TicketSet {
    /// `getTicketLevelAt`: the lowest level, `MAX_LEVEL + 1` without tickets.
    pub fn level(&self) -> u8 {
        self.0.first().map_or(MAX_LEVEL + 1, |t| t.level)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn find(&self, ticket: &Ticket) -> Result<usize, usize> {
        self.0.binary_search_by(|t| t.key().cmp(&ticket.key()))
    }

    /// `replace`: the ticket takes the place of an equal one (which is returned).
    pub fn replace(&mut self, ticket: Ticket) -> Option<Ticket> {
        match self.find(&ticket) {
            Ok(i) => Some(std::mem::replace(&mut self.0[i], ticket)),
            Err(i) => {
                self.0.insert(i, ticket);
                None
            }
        }
    }

    /// `add`: false when an equal ticket is there.
    pub fn add(&mut self, ticket: Ticket) -> bool {
        match self.find(&ticket) {
            Ok(_) => false,
            Err(i) => {
                self.0.insert(i, ticket);
                true
            }
        }
    }

    pub fn remove(&mut self, probe: &Ticket) -> Option<Ticket> {
        self.find(probe).ok().map(|i| self.0.remove(i))
    }

    /// `expireAndRemoveInto`: every ticket's delay counts down; the expired ones go.
    pub fn expire(&mut self) -> usize {
        let before = self.0.len();
        self.0.retain_mut(|t| !t.tick());
        before - self.0.len()
    }

    #[cfg(test)]
    pub fn tickets(&self) -> &[Ticket] {
        &self.0
    }
}
