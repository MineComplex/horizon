//! Chunk I/O: a dimension's region files (`RegionFileStorage` as Moonrise patches it) and the
//! workers that read and write them (Moonrise's `MoonriseRegionFileIO`, redesigned).
//!
//! On disk this is Leaf's behaviour: `r.<x>.<z>.mca` in the folder (created on the first write
//! only), chunks compressed with `region-file-compression` (Leaf's default `lz4`; any stored
//! compression reads), the header timestamp in seconds, a `None` write clearing the chunk, an
//! open-file cache of 256 regions (Paper's `region-file-cache-size` default; the least recently
//! used one is closed) and Moonrise's set of regions known not to exist (4096), which a read miss
//! fills without evicting an open region.
//!
//! Threads: Moonrise runs one I/O thread pool with per-chunk task queues, coalescing a queued
//! write with a later one and serving reads from queued writes. Here each worker thread owns its
//! own [`RegionStorage`] and a region always goes to the same worker (`hash(region) % workers`), so
//! the requests of a region run in the order they were sent: a load after a save reads what was
//! saved, a save after a save leaves the last one, as in Moonrise. Requests carry owned data
//! ([`ChunkData`]) and the worker decodes or encodes it with [`ChunkCodec`] over `Arc`'d read-only
//! tables; results come back as [`ChunkIoEvent`] messages on one channel the tick thread drains.
//! There is no order between regions (Moonrise has none between chunks either).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;
use tracing::error;

use super::chunk::{
    BiomeIds, ChunkCodec, ChunkData, ChunkReadError, ContainerFactory, SectionFlags,
};
use super::storage::compression::Compression;
use super::storage::region::{ChunkRecord, RegionFile};
use crate::block::BlockStates;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Limits, NamedTag, Tag};

const REGION_CACHE: usize = 256;
const NON_EXISTING_CACHE: usize = 4096;

/// `RegionFileVersion.configure`: the `region-file-compression` value; an unknown one is logged
/// and the built-in default (deflate) stays.
pub fn compression_from_property(name: &str) -> Compression {
    match name {
        "deflate" => Compression::Zlib,
        "none" => Compression::None,
        "lz4" => Compression::Lz4,
        _ => {
            error!(
                "Invalid `region-file-compression` value `{name}` in server.properties. Please use one of: lz4, none, deflate"
            );
            Compression::Zlib
        }
    }
}

/// One folder of region files (`region/`, `entities/` or `poi/` of a dimension).
pub struct RegionStorage {
    folder: PathBuf,
    compression: Compression,
    /// `sync-chunk-writes` (Paper: only with `-DPaper.enable-sync-chunk-writes=true`).
    sync: bool,
    /// Open regions, least recently used first.
    cache: IndexMap<ChunkPos, RegionFile>,
    /// Moonrise's `nonExistingRegionFiles`, least recently used first.
    non_existing: IndexSet<ChunkPos>,
}

impl RegionStorage {
    pub fn new(folder: PathBuf, compression: Compression, sync: bool) -> Self {
        Self {
            folder,
            compression,
            sync,
            cache: IndexMap::new(),
            non_existing: IndexSet::new(),
        }
    }

    /// `getRegionFile(pos, create)` with Moonrise's caches.
    fn region(&mut self, chunk: ChunkPos, create: bool) -> io::Result<Option<&mut RegionFile>> {
        let key = chunk.region();
        if let Some(index) = self.cache.get_index_of(&key) {
            let last = self.cache.len() - 1;
            self.cache.move_index(index, last);
            return Ok(self.cache.get_index_mut(last).map(|(_, r)| r));
        }
        if !create && self.non_existing.contains(&key) {
            self.non_existing.shift_remove(&key);
            self.non_existing.insert(key);
            return Ok(None);
        }
        let path = self.folder.join(format!("r.{}.{}.mca", key.x, key.z));
        if !create && !path.is_file() {
            self.non_existing.insert(key);
            while self.non_existing.len() >= NON_EXISTING_CACHE {
                self.non_existing.shift_remove_index(0);
            }
            return Ok(None);
        }
        if self.cache.len() >= REGION_CACHE {
            // Closing (dropping) the evicted region; its writes are already in the file.
            self.cache.shift_remove_index(0);
        }
        self.non_existing.shift_remove(&key);
        std::fs::create_dir_all(&self.folder)?;
        let region = RegionFile::open_writable(&path, true)?;
        self.cache.insert(key, region);
        Ok(self.cache.last_mut().map(|(_, r)| r))
    }

    /// `read`: the chunk's NBT, `None` when the region or the chunk is absent.
    pub fn read(&mut self, pos: ChunkPos) -> io::Result<Option<Compound>> {
        let Some(region) = self.region(pos, false)? else {
            return Ok(None);
        };
        let Some(record) = region.read(pos)? else {
            return Ok(None);
        };
        match record.nbt(Limits::templates())?.tag {
            Tag::Compound(c) => Ok(Some(c)),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk root is not a compound",
            )),
        }
    }

    /// `write`: `None` clears the chunk (without creating its region, as Moonrise's delete).
    pub fn write(&mut self, pos: ChunkPos, chunk: Option<Compound>) -> io::Result<()> {
        let timestamp = (jiff::Timestamp::now().as_millisecond() / 1000) as u32;
        let Some(chunk) = chunk else {
            if let Some(region) = self.region(pos, false)? {
                region.clear(pos, timestamp)?;
            }
            return Ok(());
        };
        let root = NamedTag {
            name: JavaString::default(),
            tag: Tag::Compound(chunk),
        };
        let record =
            ChunkRecord::from_nbt(&root, self.compression, timestamp, Limits::templates())?;
        let sync = self.sync;
        let region = self.region(pos, true)?.expect("created");
        region.write(pos, &record)?;
        if sync {
            region.sync()?;
        }
        Ok(())
    }

    /// `flush`: every open region to the disk.
    pub fn flush(&mut self) -> io::Result<()> {
        self.cache.values().try_for_each(RegionFile::sync)
    }
}

/// The read-only tables a worker decodes and encodes chunks with (shared, never mutated).
pub struct CodecTables<F> {
    pub blocks: Arc<BlockStates>,
    pub flags: Arc<F>,
    pub biomes: Arc<BiomeIds>,
    pub factory: ContainerFactory,
    pub min_section_y: i32,
    pub max_section_y: i32,
}

impl<F> CodecTables<F> {
    pub fn codec(&self) -> ChunkCodec<'_, F> {
        ChunkCodec {
            blocks: self.blocks.as_ref(),
            flags: self.flags.as_ref(),
            biomes: self.biomes.as_ref(),
            factory: self.factory,
            min_section_y: self.min_section_y,
            max_section_y: self.max_section_y,
        }
    }
}

#[derive(Debug, Error)]
pub enum ChunkIoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Read(#[from] ChunkReadError),
}

/// What a worker reports back.
#[derive(Debug)]
pub enum ChunkIoEvent {
    /// `None`: no chunk saved there (or no `Status`, which vanilla reads as none).
    Loaded {
        pos: ChunkPos,
        result: Result<Option<Box<ChunkData>>, ChunkIoError>,
    },
    Saved {
        pos: ChunkPos,
        result: Result<(), ChunkIoError>,
    },
    /// One per worker after [`ChunkIo::flush`]: its earlier writes are on the disk.
    Flushed {
        worker: usize,
        result: io::Result<()>,
    },
}

enum Request {
    Load(ChunkPos),
    Save(ChunkPos, Option<Box<ChunkData>>),
    Flush,
}

/// The workers of one region folder (see the module docs).
pub struct ChunkIo {
    workers: Vec<(Sender<Request>, JoinHandle<()>)>,
}

impl ChunkIo {
    /// Starts `workers` threads (at least one) over `folder`; events arrive on the receiver.
    pub fn spawn<F: SectionFlags + Send + Sync + 'static>(
        folder: PathBuf,
        compression: Compression,
        sync: bool,
        tables: Arc<CodecTables<F>>,
        workers: usize,
    ) -> io::Result<(Self, Receiver<ChunkIoEvent>)> {
        let (events, receiver) = mpsc::channel();
        let workers = (0..workers.max(1))
            .map(|index| {
                let (sender, requests) = mpsc::channel();
                let mut storage = RegionStorage::new(folder.clone(), compression, sync);
                let (events, tables) = (events.clone(), tables.clone());
                let thread = std::thread::Builder::new()
                    .name(format!("Region I/O #{index}"))
                    .spawn(move || {
                        for request in requests {
                            let event = run(&mut storage, &tables, index, request);
                            if events.send(event).is_err() {
                                break;
                            }
                        }
                        if let Err(e) = storage.flush() {
                            error!("Failed to flush region files: {e}");
                        }
                    })?;
                Ok((sender, thread))
            })
            .collect::<io::Result<_>>()?;
        Ok((Self { workers }, receiver))
    }

    fn send(&self, pos: ChunkPos, request: Request) {
        let mut hasher = DefaultHasher::new();
        pos.region().hash(&mut hasher);
        let worker = (hasher.finish() % self.workers.len() as u64) as usize;
        // A worker only stops when its sender is dropped, which `shutdown` does.
        let _ = self.workers[worker].0.send(request);
    }
    /// Reads the chunk: answered by [`ChunkIoEvent::Loaded`].
    pub fn load(&self, pos: ChunkPos) {
        self.send(pos, Request::Load(pos));
    }
    /// Writes the chunk (`None` deletes it): answered by [`ChunkIoEvent::Saved`].
    pub fn save(&self, pos: ChunkPos, chunk: Option<Box<ChunkData>>) {
        self.send(pos, Request::Save(pos, chunk));
    }
    /// Syncs every worker's open regions after the requests already sent.
    pub fn flush(&self) {
        for (sender, _) in &self.workers {
            let _ = sender.send(Request::Flush);
        }
    }
    /// Runs the queued requests, syncs and closes the regions, and joins the workers.
    pub fn shutdown(self) {
        for (sender, thread) in self.workers {
            drop(sender);
            if thread.join().is_err() {
                error!("A region I/O worker panicked");
            }
        }
    }
}

fn run<F: SectionFlags>(
    storage: &mut RegionStorage,
    tables: &CodecTables<F>,
    worker: usize,
    request: Request,
) -> ChunkIoEvent {
    match request {
        Request::Load(pos) => {
            let result = (|| {
                let Some(tag) = storage.read(pos)? else {
                    return Ok(None);
                };
                let Some(mut chunk) = tables.codec().read(&tag)? else {
                    return Ok(None);
                };
                if chunk.pos != pos {
                    error!(
                        "Chunk file at {pos:?} is in the wrong location; relocating. (Expected {pos:?}, got {:?})",
                        chunk.pos
                    );
                    chunk.pos = pos;
                }
                Ok(Some(Box::new(chunk)))
            })();
            ChunkIoEvent::Loaded { pos, result }
        }
        Request::Save(pos, chunk) => {
            let tag = chunk.map(|c| tables.codec().write(&c));
            let result = storage.write(pos, tag).map_err(ChunkIoError::from);
            ChunkIoEvent::Saved { pos, result }
        }
        Request::Flush => ChunkIoEvent::Flushed {
            worker,
            result: storage.flush(),
        },
    }
}
