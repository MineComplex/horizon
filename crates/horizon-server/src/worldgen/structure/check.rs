//! `StructureCheck` (Paper's rewrite of it): a level's quick answer to "does this chunk hold a start
//! of this structure", without loading the chunk. Locating (`ChunkGenerator.findNearestMapStructure`)
//! asks it before it loads anything.
//!
//! A chunk's answer comes from, in order: the cache of chunks seen (`loadedChunks`: a loaded
//! chunk's starts with their reference counts, [`StructureCheck::on_structure_load`]); the saved
//! chunk's `structures.starts`, read without loading the chunk ([`ChunkScan`]); then, for a chunk
//! with nothing saved, whether the placement's extra restrictions allow a start and whether the
//! structure could be made there at all (`canCreateStructure`, cached per structure and chunk),
//! which gives [`StructureCheckResult::ChunkLoadNeeded`] or `StartNotPresent`.
//!
//! Stand-ins: a saved chunk is read in its current format only. Vanilla runs the chunk data
//! fixers over the partial tag first; a chunk in the pre-1.18 `Level` layout (or one that fails
//! to read) answers `ChunkLoadNeeded`, which makes the caller load, and so upgrade, the chunk.
//! Paper's size limits on the caches (50 locate searches' worth) are a plain clear here once a
//! cache passes its limit; they only bound memory.

use std::collections::{HashMap, HashSet};

use super::{StructureId, StructureRegistry};
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag};

/// `StructureCheckResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureCheckResult {
    StartNotPresent,
    StartPresent,
    ChunkLoadNeeded,
}

/// `ChunkScanAccess`: reads the fields of a saved chunk without loading it.
pub trait ChunkScan {
    /// The chunk's saved root tag (at least `DataVersion` and `structures`), `None` when nothing
    /// is saved for it, `Err` when reading failed.
    fn scan_chunk(&self, pos: ChunkPos) -> Result<Option<Compound>, String>;
}

/// A level without saved chunks.
pub struct NoSavedChunks;

impl ChunkScan for NoSavedChunks {
    fn scan_chunk(&self, _pos: ChunkPos) -> Result<Option<Compound>, String> {
        Ok(None)
    }
}

/// `NO_STRUCTURE`'s counterpart: a structure's starts in a chunk, by reference count.
type Starts = Vec<(StructureId, i32)>;

/// Paper's `CHUNK_TOTAL_LIMIT` and `PER_FEATURE_CHECK_LIMIT`: 50 searches of radius 100.
const CACHE_LIMIT: usize = 50 * 201 * 201;
/// `MAX_CHUNKS_WITHOUT_STORED_STARTS`.
const MAX_CHUNKS_WITHOUT_STORED_STARTS: usize = 131072;

/// A level's `StructureCheck`.
#[derive(Debug, Default)]
pub struct StructureCheck {
    /// `loadedChunksSafe`: the starts of every chunk seen, by packed chunk.
    loaded: HashMap<i64, Starts>,
    /// `featureChecksSafe`: `canCreateStructure` per structure and chunk.
    feature_checks: HashMap<StructureId, HashMap<i64, bool>>,
    /// `chunksWithoutStartsInStorage`.
    without_starts: HashSet<i64>,
}

impl StructureCheck {
    pub fn new() -> Self {
        Self::default()
    }

    /// `checkStart`. `restrictions_allow` is the placement's `applyAdditionalChunkRestrictions`
    /// for this chunk (with the structure's seed override), `can_create` is
    /// `canCreateStructure`: whether the structure finds a valid generation point here. Each is
    /// asked only when the answer needs it.
    pub fn check_start(
        &mut self,
        registry: &StructureRegistry,
        pos: ChunkPos,
        structure: StructureId,
        require_unreferenced: bool,
        scan: &dyn ChunkScan,
        restrictions_allow: impl FnOnce() -> bool,
        can_create: impl FnOnce() -> bool,
    ) -> StructureCheckResult {
        let key = pos.pack();
        if let Some(cached) = self.loaded.get(&key) {
            return check_structure_info(cached, structure, require_unreferenced);
        }
        if !self.without_starts.contains(&key) {
            if let Some(result) =
                self.try_load_from_storage(registry, pos, structure, require_unreferenced, scan)
            {
                return result;
            }
            if self.without_starts.len() >= MAX_CHUNKS_WITHOUT_STORED_STARTS {
                self.without_starts.clear();
            }
            self.without_starts.insert(key);
        }
        if !restrictions_allow() {
            return StructureCheckResult::StartNotPresent;
        }
        let checks = self.feature_checks.entry(structure).or_default();
        let creatable = match checks.get(&key) {
            Some(&known) => known,
            None => {
                if checks.len() >= CACHE_LIMIT {
                    checks.clear();
                }
                let made = can_create();
                checks.insert(key, made);
                made
            }
        };
        if creatable {
            StructureCheckResult::ChunkLoadNeeded
        } else {
            StructureCheckResult::StartNotPresent
        }
    }

    /// `tryLoadFromStorage`: `None` when the chunk has no saved starts (nothing saved, or no
    /// `structures` tag), else the answer from them (cached for the chunk).
    fn try_load_from_storage(
        &mut self,
        registry: &StructureRegistry,
        pos: ChunkPos,
        structure: StructureId,
        require_unreferenced: bool,
        scan: &dyn ChunkScan,
    ) -> Option<StructureCheckResult> {
        let chunk = match scan.scan_chunk(pos) {
            Ok(Some(chunk)) => chunk,
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!("Failed to read chunk {pos:?}: {error}");
                return Some(StructureCheckResult::ChunkLoadNeeded);
            }
        };
        if chunk.contains_key(&JavaString::from("Level")) {
            // The pre-1.18 layout needs the data fixers (module docs).
            return Some(StructureCheckResult::ChunkLoadNeeded);
        }
        let known = load_structures(registry, &chunk)?;
        self.store_full_results(pos.pack(), known.clone());
        Some(check_structure_info(
            &known,
            structure,
            require_unreferenced,
        ))
    }

    /// `onStructureLoad`: a loaded chunk's valid starts and their reference counts.
    pub fn on_structure_load(&mut self, pos: ChunkPos, starts: impl IntoIterator<Item = (StructureId, i32)>) {
        self.store_full_results(pos.pack(), starts.into_iter().collect());
    }

    fn store_full_results(&mut self, key: i64, starts: Starts) {
        if self.loaded.len() >= CACHE_LIMIT {
            self.loaded.clear();
        }
        self.loaded.insert(key, starts);
        for checks in self.feature_checks.values_mut() {
            checks.remove(&key);
        }
        self.without_starts.remove(&key);
    }

    /// `incrementReference`: a locate search took the chunk's start of `structure`.
    pub fn increment_reference(&mut self, pos: ChunkPos, structure: StructureId) {
        let key = pos.pack();
        let counts = self.loaded.entry(key).or_default();
        match counts.iter_mut().find(|(s, _)| *s == structure) {
            Some((_, count)) => *count += 1,
            None => counts.push((structure, 1)),
        }
        self.without_starts.remove(&key);
    }
}

/// `checkStructureInfo`.
fn check_structure_info(
    starts: &Starts,
    structure: StructureId,
    require_unreferenced: bool,
) -> StructureCheckResult {
    match starts.iter().find(|(s, _)| *s == structure) {
        Some(&(_, references)) if !require_unreferenced || references == 0 => {
            StructureCheckResult::StartPresent
        }
        _ => StructureCheckResult::StartNotPresent,
    }
}

/// `loadStructures`: the chunk's `structures.starts`, each start of a known structure that is not
/// `INVALID` with its `references`. `None` without the tags.
fn load_structures(registry: &StructureRegistry, chunk: &Compound) -> Option<Starts> {
    let Some(Tag::Compound(structures)) = chunk.get(&JavaString::from("structures")) else {
        return None;
    };
    let Some(Tag::Compound(starts)) = structures.get(&JavaString::from("starts")) else {
        return None;
    };
    let mut known = Vec::new();
    for (key, tag) in starts {
        let Ok(name) = crate::registry::parse_identifier(&key.to_string_lossy()) else {
            continue;
        };
        let Some(structure) = registry.structure_id(&name) else {
            continue;
        };
        let Tag::Compound(data) = tag else { continue };
        let piece_id = match data.get(&JavaString::from("id")) {
            Some(Tag::String(id)) => id.to_string_lossy(),
            _ => String::new(),
        };
        if piece_id != "INVALID" {
            let references = (data.get(&JavaString::from("references")))
                .and_then(Tag::int_value)
                .unwrap_or(0);
            known.push((structure, references));
        }
    }
    Some(known)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Saved(Compound);
    impl ChunkScan for Saved {
        fn scan_chunk(&self, _pos: ChunkPos) -> Result<Option<Compound>, String> {
            Ok(Some(self.0.clone()))
        }
    }

    fn empty_registry() -> StructureRegistry {
        StructureRegistry {
            structures: Vec::new(),
            sets: Vec::new(),
            tags: Vec::new(),
            pools: Default::default(),
            templates: Default::default(),
            mineshaft_blocking: super::super::BiomeSet::new(Vec::new()),
            monument_surrounding: super::super::BiomeSet::new(Vec::new()),
        }
    }

    #[test]
    fn unsaved_chunks_ask_the_generator_once() {
        let registry = empty_registry();
        let mut check = StructureCheck::new();
        let pos = ChunkPos::new(3, -4);
        let structure = StructureId(0);
        let mut asked = 0;
        for _ in 0..3 {
            let result = check.check_start(
                &registry,
                pos,
                structure,
                false,
                &NoSavedChunks,
                || true,
                || {
                    asked += 1;
                    true
                },
            );
            assert_eq!(result, StructureCheckResult::ChunkLoadNeeded);
        }
        assert_eq!(asked, 1);
        // Restrictions come first; a refusal asks nothing.
        let other = check.check_start(
            &registry,
            ChunkPos::new(9, 9),
            structure,
            false,
            &NoSavedChunks,
            || false,
            || panic!("not asked"),
        );
        assert_eq!(other, StructureCheckResult::StartNotPresent);
    }

    #[test]
    fn loaded_starts_and_references() {
        let registry = empty_registry();
        let mut check = StructureCheck::new();
        let pos = ChunkPos::new(0, 0);
        let (a, b) = (StructureId(0), StructureId(1));
        check.on_structure_load(pos, [(a, 0)]);
        let ask = |check: &mut StructureCheck, structure, unreferenced| {
            check.check_start(
                &registry,
                pos,
                structure,
                unreferenced,
                &NoSavedChunks,
                || panic!("cached"),
                || panic!("cached"),
            )
        };
        assert_eq!(ask(&mut check, a, true), StructureCheckResult::StartPresent);
        assert_eq!(ask(&mut check, b, false), StructureCheckResult::StartNotPresent);
        check.increment_reference(pos, a);
        assert_eq!(ask(&mut check, a, true), StructureCheckResult::StartNotPresent);
        assert_eq!(ask(&mut check, a, false), StructureCheckResult::StartPresent);
    }

    #[test]
    fn saved_chunk_without_the_tag_falls_through() {
        let registry = empty_registry();
        let mut check = StructureCheck::new();
        let saved = Saved(Compound::new());
        let result = check.check_start(
            &registry,
            ChunkPos::new(1, 1),
            StructureId(0),
            false,
            &saved,
            || true,
            || false,
        );
        assert_eq!(result, StructureCheckResult::StartNotPresent);
        let legacy = Saved(Compound::from([(
            JavaString::from("Level"),
            Tag::Compound(Compound::new()),
        )]));
        let result = check.check_start(
            &registry,
            ChunkPos::new(2, 2),
            StructureId(0),
            false,
            &legacy,
            || true,
            || false,
        );
        assert_eq!(result, StructureCheckResult::ChunkLoadNeeded);
    }
}
