//! Scheduled block and fluid ticks (`net.minecraft.world.ticks`): what a level has queued to tick
//! later, per chunk ([`LevelChunkTicks`]), per level ([`LevelTicks`]) and while a chunk is
//! generated ([`ProtoChunkTicks`]), and their saved form ([`SavedTick`], `block_ticks` and
//! `fluid_ticks` in the chunk NBT).
//!
//! Plain data the tick thread owns. Running a due tick is the caller's: [`LevelTicks::tick`] hands
//! each one, in vanilla's order, to a closure that also gets the container back (a tick that
//! schedules another one); [`LevelTicks::collect_ticks`], [`LevelTicks::pop_due`] and
//! [`LevelTicks::cleanup_after_tick`] are its three phases for a caller that cannot lend itself
//! to a closure. What a block does when it ticks is not here.
//!
//! Where it differs from the Java classes:
//! - vanilla's containers are shared between the chunk and the level (`LevelChunk` holds the
//!   `LevelChunkTicks` that `LevelTicks` registered, with an `onTickAdded` callback); here the
//!   level owns a container from `add_container` to `remove_container` and the callback is the
//!   return value of [`LevelChunkTicks::schedule`] / [`LevelChunkTicks::unpack`];
//! - vanilla's priority queues break ties between equal ticks (same time, priority and sub-tick
//!   order, which happens between the ticks two chunks unpack from disk) by the heap's and
//!   fastutil's iteration order; here by position, then by chunk, so a given state always drains
//!   the same way (derived from the sources: no oracle can fix a hash order);
//! - Moonrise's dirty tracking (`moonrise$isDirty`: a chunk with queued ticks is unsaved whenever
//!   the game time moved since its ticks were packed) is [`LevelChunkTicks::is_dirty`].

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::hash::Hash;

use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt::{Compound, JavaString, Tag};

/// `TickPriority`: lower runs first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TickPriority {
    ExtremelyHigh,
    VeryHigh,
    High,
    Normal,
    Low,
    VeryLow,
    ExtremelyLow,
}

impl TickPriority {
    pub fn value(self) -> i32 {
        self as i32 - 3
    }

    /// `byValue`: values past the ends clamp to them.
    pub fn by_value(value: i32) -> Self {
        [
            Self::ExtremelyHigh,
            Self::VeryHigh,
            Self::High,
            Self::Normal,
            Self::Low,
            Self::VeryLow,
            Self::ExtremelyLow,
        ][(value.clamp(-3, 3) + 3) as usize]
    }
}

/// `ScheduledTick`: what ticks at `pos`, when (`trigger_tick` is the level's game time), in which
/// priority and in which order among ticks of one time and priority (`sub_tick_order`, the
/// level's counter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledTick<T> {
    pub ty: T,
    pub pos: BlockPos,
    pub trigger_tick: i64,
    pub priority: TickPriority,
    pub sub_tick_order: i64,
}

impl<T> ScheduledTick<T> {
    /// `DRAIN_ORDER`.
    fn drain_key(&self) -> (i64, TickPriority, i64) {
        (self.trigger_tick, self.priority, self.sub_tick_order)
    }

    /// `INTRA_TICK_DRAIN_ORDER`.
    fn intra_key(&self) -> (TickPriority, i64) {
        (self.priority, self.sub_tick_order)
    }

    /// `toSavedTick`: the delay is relative to `current_tick` (cast to `int` as Java does).
    pub fn to_saved(&self, current_tick: i64) -> SavedTick<T>
    where
        T: Copy,
    {
        SavedTick {
            ty: self.ty,
            pos: self.pos,
            delay: self.trigger_tick.wrapping_sub(current_tick) as i32,
            priority: self.priority,
        }
    }
}

/// `SavedTick`: a tick as the chunk NBT keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedTick<T> {
    pub ty: T,
    pub pos: BlockPos,
    pub delay: i32,
    pub priority: TickPriority,
}

impl<T> SavedTick<T> {
    /// `unpack`.
    pub fn unpack(&self, current_tick: i64, sub_tick_order: i64) -> ScheduledTick<T>
    where
        T: Copy,
    {
        ScheduledTick {
            ty: self.ty,
            pos: self.pos,
            trigger_tick: current_tick.wrapping_add(i64::from(self.delay)),
            priority: self.priority,
            sub_tick_order,
        }
    }
}

/// `filterTickListForChunk`: the ticks whose position is in the chunk.
pub fn filter_ticks_for_chunk<T>(ticks: &mut Vec<SavedTick<T>>, chunk: ChunkPos) {
    ticks.retain(|t| t.pos.chunk() == chunk);
}

/// `SavedTick.codec(..).listOf()` read from a chunk's `block_ticks` or `fluid_ticks` (`None` when
/// the tag is absent). Any element that does not decode (a missing field, an unknown id: `ty`
/// says) fails the whole list, which reads as empty (`.orElse(List.of())`).
pub fn read_saved_ticks<T>(
    tag: Option<&Tag>,
    ty: impl Fn(&str) -> Result<T, String>,
) -> Vec<SavedTick<T>> {
    let Some(Tag::List(list)) = tag else {
        return Vec::new();
    };
    let mut ticks = Vec::with_capacity(list.len());
    for element in list {
        let Some(tick) = read_saved_tick(element, &ty) else {
            return Vec::new();
        };
        ticks.push(tick);
    }
    ticks
}

/// `Codec.INT` over a tag: any number, through `Number.intValue`.
fn codec_int(tag: Option<&Tag>) -> Option<i32> {
    Some(match tag? {
        Tag::Byte(v) => i32::from(*v),
        Tag::Short(v) => i32::from(*v),
        Tag::Int(v) => *v,
        Tag::Long(v) => *v as i32,
        Tag::Float(v) => f32::from_bits(*v) as i32,
        Tag::Double(v) => f64::from_bits(*v) as i32,
        _ => return None,
    })
}

fn read_saved_tick<T>(tag: &Tag, ty: &impl Fn(&str) -> Result<T, String>) -> Option<SavedTick<T>> {
    let Tag::Compound(map) = tag else {
        return None;
    };
    let field = |key: &str| map.get(&JavaString::from(key));
    let Some(Tag::String(name)) = field("i") else {
        return None;
    };
    let ty = ty(&name.to_string_lossy()).ok()?;
    let int = |key: &str| codec_int(field(key));
    Some(SavedTick {
        ty,
        pos: BlockPos::new(int("x")?, int("y")?, int("z")?),
        delay: int("t")?,
        priority: TickPriority::by_value(int("p")?),
    })
}

/// The `block_ticks` / `fluid_ticks` list of ticks (`i`, `x`, `y`, `z`, `t`, `p`).
pub fn write_saved_ticks<T: Copy>(ticks: &[SavedTick<T>], name: impl Fn(T) -> String) -> Tag {
    Tag::List(
        ticks
            .iter()
            .map(|t| {
                let mut map = Compound::new();
                let mut put = |key: &str, value: Tag| {
                    map.insert(key.into(), value);
                };
                put("i", Tag::String(name(t.ty).as_str().into()));
                put("x", Tag::Int(t.pos.x));
                put("y", Tag::Int(t.pos.y));
                put("z", Tag::Int(t.pos.z));
                put("t", Tag::Int(t.delay));
                put("p", Tag::Int(t.priority.value()));
                Tag::Compound(map)
            })
            .collect(),
    )
}

/// A queued tick, ordered by `DRAIN_ORDER` (smallest first out of the heap) then by position.
#[derive(Clone, Copy, Debug)]
struct Queued<T>(ScheduledTick<T>);

impl<T> Queued<T> {
    fn key(&self) -> ((i64, TickPriority, i64), (i32, i32, i32)) {
        let p = self.0.pos;
        (self.0.drain_key(), (p.x, p.y, p.z))
    }
}
impl<T> PartialEq for Queued<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl<T> Eq for Queued<T> {}
impl<T> PartialOrd for Queued<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Queued<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.key().cmp(&self.key())
    }
}

/// `LevelChunkTicks`: one chunk's queued ticks.
#[derive(Clone, Debug)]
pub struct LevelChunkTicks<T> {
    queue: BinaryHeap<Queued<T>>,
    /// Ticks read from disk, relative to the time they unpack at (`pendingTicks`).
    pending: Option<Vec<SavedTick<T>>>,
    /// `ticksPerPosition`.
    per_position: HashSet<(T, BlockPos)>,
    dirty: bool,
    last_saved: i64,
}

impl<T> Default for LevelChunkTicks<T> {
    fn default() -> Self {
        Self {
            queue: BinaryHeap::new(),
            pending: None,
            per_position: HashSet::new(),
            dirty: false,
            last_saved: i64::MIN,
        }
    }
}

impl<T: Copy + Eq + Hash> LevelChunkTicks<T> {
    /// `LevelChunkTicks(List<SavedTick>)`: the saved ticks wait for [`Self::unpack`].
    pub fn from_pending(pending: Vec<SavedTick<T>>) -> Self {
        let per_position = pending.iter().map(|t| (t.ty, t.pos)).collect();
        Self {
            pending: Some(pending),
            per_position,
            ..Self::default()
        }
    }

    pub fn peek(&self) -> Option<&ScheduledTick<T>> {
        self.queue.peek().map(|q| &q.0)
    }

    pub fn poll(&mut self) -> Option<ScheduledTick<T>> {
        let tick = self.queue.pop()?.0;
        self.per_position.remove(&(tick.ty, tick.pos));
        self.dirty = true;
        Some(tick)
    }

    /// `schedule`: a tick of the same thing at the same position is dropped. Returns the tick
    /// when it became the first to run, which is what `onTickAdded` tells the level.
    pub fn schedule(&mut self, tick: ScheduledTick<T>) -> Option<ScheduledTick<T>> {
        if !self.per_position.insert((tick.ty, tick.pos)) {
            return None;
        }
        self.dirty = true;
        self.push(tick)
    }

    fn push(&mut self, tick: ScheduledTick<T>) -> Option<ScheduledTick<T>> {
        self.queue.push(Queued(tick));
        (self.peek() == Some(&tick)).then_some(tick)
    }

    pub fn has_scheduled_tick(&self, pos: BlockPos, ty: T) -> bool {
        self.per_position.contains(&(ty, pos))
    }

    /// `removeIf`.
    pub fn remove_if(&mut self, mut test: impl FnMut(&ScheduledTick<T>) -> bool) {
        let before = self.queue.len();
        let mut kept = std::mem::take(&mut self.queue).into_vec();
        kept.retain(|q| {
            let remove = test(&q.0);
            if remove {
                self.per_position.remove(&(q.0.ty, q.0.pos));
            }
            !remove
        });
        self.dirty |= kept.len() != before;
        self.queue = kept.into();
    }

    /// `getAll`.
    pub fn all(&self) -> impl Iterator<Item = &ScheduledTick<T>> {
        self.queue.iter().map(|q| &q.0)
    }

    /// `count`: queued and still pending.
    pub fn count(&self) -> usize {
        self.queue.len() + self.pending.as_ref().map_or(0, Vec::len)
    }

    /// The ticks as saved at `current_tick` (`pack` without noting the time): the pending ones,
    /// then the queued ones in sub-tick order.
    pub fn saved(&self, current_tick: i64) -> Vec<SavedTick<T>> {
        let mut ticks = self.pending.clone().unwrap_or_default();
        let mut queued: Vec<&ScheduledTick<T>> = self.all().collect();
        queued.sort_by_key(|t| (t.sub_tick_order, t.drain_key(), (t.pos.x, t.pos.y, t.pos.z)));
        ticks.extend(queued.iter().map(|t| t.to_saved(current_tick)));
        ticks
    }

    /// `pack`: [`Self::saved`], noting the time for [`Self::is_dirty`].
    pub fn pack(&mut self, current_tick: i64) -> Vec<SavedTick<T>> {
        self.last_saved = current_tick;
        self.saved(current_tick)
    }

    /// `unpack`: the pending ticks become queued ones at `current_tick`, in order, with sub-tick
    /// orders `-n..-1`. Returns the last tick that became the first to run, as the callbacks
    /// would have told the level.
    pub fn unpack(&mut self, current_tick: i64) -> Option<ScheduledTick<T>> {
        let pending = self.pending.take()?;
        self.last_saved = current_tick;
        let mut first = None;
        let first_sub_tick = -(pending.len() as i64);
        for (sub_tick, tick) in (first_sub_tick..).zip(&pending) {
            first = self.push(tick.unpack(current_tick, sub_tick)).or(first);
        }
        first
    }

    /// `moonrise$isDirty`: changed since it was packed, or queued ticks whose delays were packed
    /// at another game time.
    pub fn is_dirty(&self, tick: i64) -> bool {
        self.dirty || (!self.queue.is_empty() && tick != self.last_saved)
    }

    /// `moonrise$clearDirty`.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

/// `ProtoChunkTicks`: the ticks scheduled while a chunk is generated, kept as saved ticks.
#[derive(Clone, Debug)]
pub struct ProtoChunkTicks<T> {
    ticks: Vec<SavedTick<T>>,
    per_position: HashSet<(T, BlockPos)>,
}

impl<T> Default for ProtoChunkTicks<T> {
    fn default() -> Self {
        Self {
            ticks: Vec::new(),
            per_position: HashSet::new(),
        }
    }
}

impl<T: Copy + Eq + Hash> ProtoChunkTicks<T> {
    /// `schedule(ScheduledTick)`: the time is dropped (the delay saves as 0).
    pub fn schedule(&mut self, pos: BlockPos, ty: T, priority: TickPriority) {
        self.schedule_saved(SavedTick {
            ty,
            pos,
            delay: 0,
            priority,
        });
    }

    fn schedule_saved(&mut self, tick: SavedTick<T>) {
        if self.per_position.insert((tick.ty, tick.pos)) {
            self.ticks.push(tick);
        }
    }

    /// `load`: a repeat of the same thing at the same position is dropped.
    pub fn load(ticks: Vec<SavedTick<T>>) -> Self {
        let mut result = Self::default();
        ticks.into_iter().for_each(|t| result.schedule_saved(t));
        result
    }

    pub fn has_scheduled_tick(&self, pos: BlockPos, ty: T) -> bool {
        self.per_position.contains(&(ty, pos))
    }

    pub fn count(&self) -> usize {
        self.ticks.len()
    }

    /// `pack` and `scheduledTicks`.
    pub fn ticks(&self) -> &[SavedTick<T>] {
        &self.ticks
    }
}

/// A `BoundingBox` of blocks, both corners inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickArea {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl TickArea {
    fn is_inside(&self, pos: BlockPos) -> bool {
        (self.min.x..=self.max.x).contains(&pos.x)
            && (self.min.y..=self.max.y).contains(&pos.y)
            && (self.min.z..=self.max.z).contains(&pos.z)
    }
}

/// What the order of containers to drain by: the first tick's `INTRA_TICK_DRAIN_ORDER`, then the
/// chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ContainerFirst {
    first: (TickPriority, i64),
    key: i64,
}

/// `LevelTicks`: every loaded chunk's ticks and the collection of the ones due.
#[derive(Debug)]
pub struct LevelTicks<T> {
    /// `allContainers`, by `ChunkPos.pack`.
    all: HashMap<i64, LevelChunkTicks<T>>,
    /// `nextTickForContainer`: the time of each container's first tick, as last told.
    next_tick_for: HashMap<i64, i64>,
    /// `containersToTick`, each with the first tick it had when queued (a queued container is
    /// not touched until it leaves).
    containers_to_tick: BinaryHeap<Reverse<ContainerFirst>>,
    to_run_this_tick: VecDeque<ScheduledTick<T>>,
    already_run_this_tick: Vec<ScheduledTick<T>>,
    to_run_this_tick_set: HashSet<(T, BlockPos)>,
}

impl<T> Default for LevelTicks<T> {
    fn default() -> Self {
        Self {
            all: HashMap::new(),
            next_tick_for: HashMap::new(),
            containers_to_tick: BinaryHeap::new(),
            to_run_this_tick: VecDeque::new(),
            already_run_this_tick: Vec::new(),
            to_run_this_tick_set: HashSet::new(),
        }
    }
}

impl<T: Copy + Eq + Hash> LevelTicks<T> {
    /// `addContainer`.
    pub fn add_container(&mut self, pos: ChunkPos, container: LevelChunkTicks<T>) {
        let key = pos.pack();
        if let Some(next) = container.peek() {
            self.next_tick_for.insert(key, next.trigger_tick);
        }
        self.all.insert(key, container);
    }

    /// `removeContainer`.
    pub fn remove_container(&mut self, pos: ChunkPos) -> Option<LevelChunkTicks<T>> {
        let key = pos.pack();
        self.next_tick_for.remove(&key);
        self.all.remove(&key)
    }

    pub fn container(&self, pos: ChunkPos) -> Option<&LevelChunkTicks<T>> {
        self.all.get(&pos.pack())
    }

    pub fn container_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunkTicks<T>> {
        self.all.get_mut(&pos.pack())
    }

    /// `LevelChunk.unpackTicks` for a registered chunk (when it starts ticking).
    pub fn unpack_container(&mut self, pos: ChunkPos, current_tick: i64) {
        let key = pos.pack();
        if let Some(first) = self.all.get_mut(&key).and_then(|c| c.unpack(current_tick)) {
            self.next_tick_for.insert(key, first.trigger_tick);
        }
    }

    /// `schedule`: into the container of the tick's chunk; none (not loaded) drops it.
    pub fn schedule(&mut self, tick: ScheduledTick<T>) {
        let key = tick.pos.chunk().pack();
        if let Some(first) = self.all.get_mut(&key).and_then(|c| c.schedule(tick)) {
            self.next_tick_for.insert(key, first.trigger_tick);
        }
    }

    /// `hasScheduledTick`.
    pub fn has_scheduled_tick(&self, pos: BlockPos, ty: T) -> bool {
        self.container(pos.chunk())
            .is_some_and(|c| c.has_scheduled_tick(pos, ty))
    }

    /// `willTickThisTick`: among the ticks collected and not yet run (the set is built on the
    /// first question after the collection, and shrinks as they run).
    pub fn will_tick_this_tick(&mut self, pos: BlockPos, ty: T) -> bool {
        if self.to_run_this_tick_set.is_empty() && !self.to_run_this_tick.is_empty() {
            self.to_run_this_tick_set
                .extend(self.to_run_this_tick.iter().map(|t| (t.ty, t.pos)));
        }
        self.to_run_this_tick_set.contains(&(ty, pos))
    }

    /// `count`.
    pub fn count(&self) -> usize {
        self.all.values().map(LevelChunkTicks::count).sum()
    }

    /// `tick`: collect what is due at `current_tick` (at most `max_ticks_to_process`), run each
    /// through `run` (which may schedule more; they are collected next tick), clean up.
    /// `is_ticking` says whether a chunk (`ChunkPos.pack`) runs its ticks.
    pub fn tick(
        &mut self,
        current_tick: i64,
        max_ticks_to_process: i32,
        is_ticking: impl Fn(i64) -> bool,
        mut run: impl FnMut(&mut Self, BlockPos, T),
    ) {
        self.collect_ticks(current_tick, max_ticks_to_process, is_ticking);
        while let Some((pos, ty)) = self.pop_due() {
            run(self, pos, ty);
        }
        self.cleanup_after_tick();
    }

    /// `collectTicks`.
    pub fn collect_ticks(
        &mut self,
        current_tick: i64,
        max_ticks_to_process: i32,
        is_ticking: impl Fn(i64) -> bool,
    ) {
        let max = usize::try_from(max_ticks_to_process).unwrap_or(0);
        self.sort_containers_to_tick(current_tick, is_ticking);
        self.drain_containers(current_tick, max);
        // `rescheduleLeftoverContainers`.
        let leftover: Vec<i64> = self.containers_to_tick.iter().map(|c| c.0.key).collect();
        for key in leftover {
            if let Some(first) = self.all.get(&key).and_then(LevelChunkTicks::peek) {
                self.next_tick_for.insert(key, first.trigger_tick);
            }
        }
    }

    fn sort_containers_to_tick(&mut self, current_tick: i64, is_ticking: impl Fn(i64) -> bool) {
        let all = &self.all;
        let queue = &mut self.containers_to_tick;
        self.next_tick_for.retain(|&key, next| {
            if *next > current_tick {
                return true;
            }
            let Some(first) = all.get(&key).and_then(LevelChunkTicks::peek) else {
                return false;
            };
            if first.trigger_tick > current_tick {
                *next = first.trigger_tick;
                true
            } else if is_ticking(key) {
                queue.push(Reverse(ContainerFirst {
                    first: first.intra_key(),
                    key,
                }));
                false
            } else {
                true
            }
        });
    }

    fn drain_containers(&mut self, current_tick: i64, max: usize) {
        while self.to_run_this_tick.len() < max
            && let Some(Reverse(top)) = self.containers_to_tick.pop()
        {
            let Some(container) = self.all.get_mut(&top.key) else {
                continue;
            };
            let Some(tick) = container.poll() else {
                continue;
            };
            self.to_run_this_tick.push_back(tick);
            // `drainFromCurrentContainer`: while this container's next tick goes before the best
            // other container's first.
            if self.to_run_this_tick.len() < max {
                let other = self.containers_to_tick.peek().map(|c| c.0.first);
                while self.to_run_this_tick.len() < max {
                    let Some(next) = container.peek() else { break };
                    if next.trigger_tick > current_tick
                        || other.is_some_and(|o| next.intra_key() > o)
                    {
                        break;
                    }
                    let next = container.poll().expect("peeked");
                    self.to_run_this_tick.push_back(next);
                }
            }
            if let Some(next) = container.peek().copied() {
                if next.trigger_tick <= current_tick && self.to_run_this_tick.len() < max {
                    self.containers_to_tick.push(Reverse(ContainerFirst {
                        first: next.intra_key(),
                        key: top.key,
                    }));
                } else {
                    self.next_tick_for.insert(top.key, next.trigger_tick);
                }
            }
        }
    }

    /// One step of `runCollectedTicks`: the next collected tick, as it is run.
    pub fn pop_due(&mut self) -> Option<(BlockPos, T)> {
        self.pop_due_tick().map(|t| (t.pos, t.ty))
    }

    /// [`Self::pop_due`] with the whole tick (to put it back unchanged).
    pub fn pop_due_tick(&mut self) -> Option<ScheduledTick<T>> {
        let tick = self.to_run_this_tick.pop_front()?;
        if !self.to_run_this_tick_set.is_empty() {
            self.to_run_this_tick_set.remove(&(tick.ty, tick.pos));
        }
        self.already_run_this_tick.push(tick);
        Some(tick)
    }

    /// `cleanupAfterTick`.
    pub fn cleanup_after_tick(&mut self) {
        self.to_run_this_tick.clear();
        self.containers_to_tick.clear();
        self.already_run_this_tick.clear();
        self.to_run_this_tick_set.clear();
    }

    fn containers_in_area(&self, area: &TickArea) -> Vec<i64> {
        let (x0, x1) = (area.min.x >> 4, area.max.x >> 4);
        let (z0, z1) = (area.min.z >> 4, area.max.z >> 4);
        let mut keys = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let key = ChunkPos::new(x, z).pack();
                if self.all.contains_key(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    /// `clearArea`: every tick inside, queued, collected or run this tick.
    pub fn clear_area(&mut self, area: &TickArea) {
        for key in self.containers_in_area(area) {
            let container = self.all.get_mut(&key).expect("listed");
            let previous = container.peek().copied();
            container.remove_if(|t| area.is_inside(t.pos));
            let new = container.peek().copied();
            if new != previous {
                match new {
                    Some(top) => {
                        self.next_tick_for.insert(key, top.trigger_tick);
                    }
                    None => {
                        self.next_tick_for.remove(&key);
                    }
                }
            }
        }
        self.already_run_this_tick
            .retain(|t| !area.is_inside(t.pos));
        self.to_run_this_tick.retain(|t| !area.is_inside(t.pos));
    }

    /// The ticks `copyAreaFrom` copies: those run this tick, those collected, then the queued.
    fn ticks_in_area(&self, area: &TickArea) -> Vec<ScheduledTick<T>> {
        let mut ticks: Vec<ScheduledTick<T>> = self
            .already_run_this_tick
            .iter()
            .chain(&self.to_run_this_tick)
            .filter(|t| area.is_inside(t.pos))
            .copied()
            .collect();
        for key in self.containers_in_area(area) {
            ticks.extend(self.all[&key].all().filter(|t| area.is_inside(t.pos)));
        }
        ticks
    }

    /// `copyArea`.
    pub fn copy_area(&mut self, area: &TickArea, offset: (i32, i32, i32)) {
        let ticks = self.ticks_in_area(area);
        self.schedule_copies(&ticks, offset);
    }

    /// `copyAreaFrom`: the ticks inside `area` of `source`, moved by `offset`.
    pub fn copy_area_from(&mut self, source: &Self, area: &TickArea, offset: (i32, i32, i32)) {
        self.schedule_copies(&source.ticks_in_area(area), offset);
    }

    fn schedule_copies(&mut self, ticks: &[ScheduledTick<T>], offset: (i32, i32, i32)) {
        let min = ticks.iter().map(|t| t.sub_tick_order).min();
        let max = ticks.iter().map(|t| t.sub_tick_order).max();
        let (Some(min), Some(max)) = (min, max) else {
            return;
        };
        for t in ticks {
            self.schedule(ScheduledTick {
                pos: t.pos.offset(offset.0, offset.1, offset.2),
                sub_tick_order: t.sub_tick_order - min + max + 1,
                ..*t
            });
        }
    }
}

/// A level's two tick containers and its sub-tick counter (`ServerLevel.blockTicks`,
/// `fluidTicks`, `Level.subTickCount`).
#[derive(Debug)]
pub struct ScheduledTicks<B, F> {
    pub block: LevelTicks<B>,
    pub fluid: LevelTicks<F>,
    sub_tick_count: i64,
}

impl<B, F> Default for ScheduledTicks<B, F> {
    fn default() -> Self {
        Self {
            block: LevelTicks::default(),
            fluid: LevelTicks::default(),
            sub_tick_count: 0,
        }
    }
}

impl<B: Copy + Eq + Hash, F: Copy + Eq + Hash> ScheduledTicks<B, F> {
    /// `nextSubTickCount`.
    pub fn next_sub_tick_count(&mut self) -> i64 {
        let count = self.sub_tick_count;
        self.sub_tick_count += 1;
        count
    }

    /// `createTick` for a block, `game_time + delay` from now.
    pub fn create_block_tick(
        &mut self,
        game_time: i64,
        pos: BlockPos,
        block: B,
        delay: i32,
        priority: TickPriority,
    ) -> ScheduledTick<B> {
        ScheduledTick {
            ty: block,
            pos,
            trigger_tick: game_time + i64::from(delay),
            priority,
            sub_tick_order: self.next_sub_tick_count(),
        }
    }

    /// `createTick` for a fluid.
    pub fn create_fluid_tick(
        &mut self,
        game_time: i64,
        pos: BlockPos,
        fluid: F,
        delay: i32,
        priority: TickPriority,
    ) -> ScheduledTick<F> {
        ScheduledTick {
            ty: fluid,
            pos,
            trigger_tick: game_time + i64::from(delay),
            priority,
            sub_tick_order: self.next_sub_tick_count(),
        }
    }
}
