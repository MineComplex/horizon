//! The changed blocks the chunk holders have to send (`ChunkHolder.changedBlocksPerSection`,
//! `hasChangedSections`) and the holders queued for `broadcastChangedChunks`.
//!
//! As vanilla: a holder is queued by the first change after its last broadcast; a holder whose
//! chunk cannot be sent when the broadcast runs keeps its changes and is not queued again until it
//! is broadcast (it never is: vanilla's quirk, kept). Positions of a section are sent in the order
//! they first changed (vanilla iterates a `ShortOpenHashSet`, whose order the client ignores).

use std::collections::HashMap;

use crate::core::position::ChunkPos;

#[derive(Debug, Default)]
struct ChunkChanges {
    /// By section index: the section-relative positions, in the order they changed.
    sections: Vec<(usize, Vec<u16>)>,
    /// One bit per position of each section listed.
    seen: HashMap<usize, Box<[u64; 64]>>,
}

#[derive(Debug, Default)]
pub struct BlockChanges {
    chunks: HashMap<ChunkPos, ChunkChanges>,
    /// `chunkHoldersToBroadcast`, in the order they were queued.
    queued: Vec<ChunkPos>,
}

impl BlockChanges {
    /// `ChunkHolder.blockChanged` (the holder is queued when it had no changes).
    pub fn changed(&mut self, chunk: ChunkPos, section: usize, relative: u16) {
        let entry = self.chunks.entry(chunk).or_default();
        if entry.sections.is_empty() {
            self.queued.push(chunk);
        }
        let seen = entry.seen.entry(section).or_insert_with(|| {
            entry.sections.push((section, Vec::new()));
            Box::new([0; 64])
        });
        let (word, bit) = (relative as usize / 64, relative % 64);
        if seen[word] >> bit & 1 == 0 {
            seen[word] |= 1 << bit;
            let list = entry
                .sections
                .iter_mut()
                .find(|(s, _)| *s == section)
                .expect("listed with its bits");
            list.1.push(relative);
        }
    }

    /// The queued holders, the queue cleared.
    pub fn take_queued(&mut self) -> Vec<ChunkPos> {
        std::mem::take(&mut self.queued)
    }

    /// A holder's changes by section index (ascending), cleared.
    pub fn take(&mut self, chunk: ChunkPos) -> Option<Vec<(usize, Vec<u16>)>> {
        let mut sections = self.chunks.remove(&chunk)?.sections;
        sections.sort_by_key(|(index, _)| *index);
        Some(sections)
    }

    /// Drops the changes of chunks for which `keep` is false (their holders are gone).
    pub fn retain(&mut self, mut keep: impl FnMut(ChunkPos) -> bool) {
        if !self.chunks.is_empty() {
            self.chunks.retain(|&pos, _| keep(pos));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queues_once_and_keeps_first_order() {
        let mut c = BlockChanges::default();
        let a = ChunkPos::new(1, 2);
        c.changed(a, 4, 300);
        c.changed(a, 2, 7);
        c.changed(a, 4, 5);
        c.changed(a, 4, 300);
        assert_eq!(c.take_queued(), vec![a]);
        assert_eq!(c.take(a), Some(vec![(2, vec![7]), (4, vec![300, 5])]));
        assert_eq!(c.take(a), None);
        c.changed(a, 0, 1);
        assert_eq!(c.take_queued(), vec![a]);
    }
}
