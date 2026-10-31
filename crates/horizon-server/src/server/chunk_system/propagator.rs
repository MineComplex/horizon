//! Ticket level propagation: what Moonrise's `ThreadedTicketLevelPropagator` computes, without its
//! concurrency (sections, locks, the sharded update queue). A chunk's level is the lowest of
//! `source + Chebyshev distance` over the chunks with tickets (each step to one of the 8
//! neighbours adds 1: Moonrise propagates over the 3x3 around each position, as vanilla's
//! `ChunkTracker`), and chunks above [`MAX_LEVEL`] have none. Derived from the sources; the tests
//! check the incremental result against that formula.
//!
//! Updates are batched like Moonrise's: sources change at once ([`Propagator::set_source`]), the
//! levels only on [`Propagator::process`], which returns the chunks whose level changed. It runs
//! the classic two-phase update: lowered or removed sources clear the levels they gave (walking
//! outwards while a neighbour's level is exactly one more), then every cleared chunk's own source
//! and the uncleared borders spread again, lowest level first.

use std::collections::HashMap;

use super::ticket::MAX_LEVEL;
use crate::core::position::ChunkPos;

/// Above every level: no level.
const NONE: u8 = MAX_LEVEL + 1;

#[derive(Default)]
pub(super) struct Propagator {
    sources: HashMap<ChunkPos, u8>,
    /// Every chunk at or below `MAX_LEVEL`.
    levels: HashMap<ChunkPos, u8>,
    /// Source changes since the last `process`: the source before the first change.
    pending: HashMap<ChunkPos, u8>,
}

fn neighbours(pos: ChunkPos) -> impl Iterator<Item = ChunkPos> {
    (-1..=1).flat_map(move |dx| {
        (-1..=1)
            .filter(move |&dz| dx != 0 || dz != 0)
            .map(move |dz| ChunkPos::new(pos.x + dx, pos.z + dz))
    })
}

impl Propagator {
    /// `setSource`/`removeSource`: `None` (or a level above `MAX_LEVEL`) removes it.
    pub fn set_source(&mut self, pos: ChunkPos, level: Option<u8>) {
        let old = self.source(pos);
        self.pending.entry(pos).or_insert(old);
        match level.filter(|&l| l <= MAX_LEVEL) {
            Some(level) => self.sources.insert(pos, level),
            None => self.sources.remove(&pos),
        };
    }

    fn source(&self, pos: ChunkPos) -> u8 {
        self.sources.get(&pos).copied().unwrap_or(NONE)
    }

    pub fn level(&self, pos: ChunkPos) -> u8 {
        self.levels.get(&pos).copied().unwrap_or(NONE)
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn set_level(&mut self, pos: ChunkPos, level: u8, changed: &mut HashMap<ChunkPos, u8>) {
        let old = self.level(pos);
        if old == level {
            return;
        }
        changed.entry(pos).or_insert(old);
        if level > MAX_LEVEL {
            self.levels.remove(&pos);
        } else {
            self.levels.insert(pos, level);
        }
    }

    /// Applies the pending source changes; returns `(chunk, old level, new level)` for every chunk
    /// whose level changed (levels above `MAX_LEVEL` read as `MAX_LEVEL + 1`), in no set order.
    pub fn process(&mut self) -> Vec<(ChunkPos, u8, u8)> {
        let mut changed = HashMap::new();
        let mut buckets: Vec<Vec<ChunkPos>> = vec![Vec::new(); usize::from(NONE)];
        // Phase 1: clear what weakened sources gave (a strengthened source is spread in phase 2).
        let mut clear: Vec<(ChunkPos, u8)> = Vec::new();
        for (pos, old) in std::mem::take(&mut self.pending) {
            let new = self.source(pos);
            if new < old {
                buckets[usize::from(new)].push(pos);
            } else if new > old && self.level(pos) == old {
                clear.push((pos, old));
            }
        }
        // Chunks whose level (read once clearing is done) spreads again over what was cleared.
        let mut borders = Vec::new();
        while let Some((pos, level)) = clear.pop() {
            let current = self.level(pos);
            if current != level {
                if current < level {
                    borders.push(pos); // given by another source
                }
                continue;
            }
            let own = self.source(pos);
            if own == current {
                borders.push(pos); // a source of its own level: still right
                continue;
            }
            // `own > current`: the level was given by a cleared chain.
            self.set_level(pos, own, &mut changed);
            borders.push(pos);
            if level < MAX_LEVEL {
                clear.extend(neighbours(pos).map(|n| (n, level + 1)));
            }
        }
        for pos in borders {
            let level = self.level(pos);
            if level <= MAX_LEVEL {
                buckets[usize::from(level)].push(pos);
            }
        }
        // Phase 2: spread, lowest level first (each chunk is final once its bucket is reached).
        for level in 0..NONE {
            let positions = std::mem::take(&mut buckets[usize::from(level)]);
            for pos in positions {
                if self.level(pos) < level {
                    continue; // lowered since it was queued
                }
                self.set_level(pos, level, &mut changed);
                if level < MAX_LEVEL {
                    for n in neighbours(pos) {
                        if self.level(n) > level + 1 {
                            self.set_level(n, level + 1, &mut changed);
                            buckets[usize::from(level + 1)].push(n);
                        }
                    }
                }
            }
        }
        changed
            .into_iter()
            .filter_map(|(pos, old)| {
                let new = self.level(pos);
                (new != old).then_some((pos, old, new))
            })
            .collect()
    }
}
