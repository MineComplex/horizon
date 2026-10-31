//! The generation side of Moonrise's `ChunkTaskScheduler`: a step to run ([`GenerationJob`]: the
//! chunks of its region as shared snapshots) goes to a level's worker threads
//! ([`GenerationPool`]) and comes back as a [`GenerationResult`] (the chunks it wrote) through a
//! channel the tick thread drains. The tick thread owns the chunks; a worker writes its own
//! copies of the chunks in its write radius and reads the others' snapshots.
//!
//! Moonrise's executors (`parallelGenExecutor`, the area-dependent `radiusAwareScheduler`) are
//! one queue of ready steps here: the scheduler only hands out a step once no running step
//! writes the chunks it writes (`ChunkHolderManager`'s write set), which covers both. Its
//! `chunk-system.worker-threads` default (half the processors, at least one) sizes each level's
//! pool.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::core::position::ChunkPos;
use crate::world::chunk::ChunkStatus;
use crate::worldgen::generator::{GenerationWorker, LevelGenerator};
use crate::worldgen::proto_chunk::{GenerationRegion, ProtoChunk, RegionChunk};

/// One generation step for a worker.
#[derive(Debug)]
pub struct GenerationJob {
    pub pos: ChunkPos,
    pub status: ChunkStatus,
    /// The step's read radius: `chunks` is the square of that radius around `pos`, row by row.
    pub radius: i32,
    /// The chunks and whether the step may write them (inside its write radius, and proto
    /// chunks). The snapshots are the holders' own `Arc`s: shared read-only, copied by the
    /// worker before writing.
    pub chunks: Vec<(Arc<ProtoChunk>, bool)>,
}

/// A step's answer.
#[derive(Debug)]
pub struct GenerationResult {
    pub pos: ChunkPos,
    pub status: ChunkStatus,
    /// The written chunks (each the worker's copy), or why the step failed (`None`: the level
    /// cannot generate, already logged).
    pub result: Result<Vec<ProtoChunk>, Option<String>>,
    pub nanos: u64,
}

/// `ChunkUpgradeGenericStatusTask.run` on a worker: the region built from the job, the step run.
pub fn execute(worker: &mut GenerationWorker<'_>, job: GenerationJob) -> GenerationResult {
    let start = Instant::now();
    let GenerationJob {
        pos,
        status,
        radius,
        chunks,
    } = job;
    let mut owned: Vec<ProtoChunk> = Vec::new();
    let mut shared: Vec<Option<Arc<ProtoChunk>>> = Vec::with_capacity(chunks.len());
    for (chunk, writable) in chunks {
        if writable {
            owned.push(Arc::unwrap_or_clone(chunk));
            shared.push(None);
        } else {
            shared.push(Some(chunk));
        }
    }
    let result = {
        let mut owned_iter = owned.iter_mut();
        let region_chunks: Vec<RegionChunk<'_>> = shared
            .iter()
            .map(|slot| match slot {
                Some(chunk) => RegionChunk::ReadOnly(chunk),
                None => RegionChunk::Writable(owned_iter.next().expect("one copy per writable")),
            })
            .collect();
        let mut region = GenerationRegion::new(pos, radius, region_chunks);
        worker
            .run(status, &mut region)
            .map_err(|e| Some(e.to_string()))
    };
    GenerationResult {
        pos,
        status,
        result: result.map(|()| owned),
        nanos: u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX),
    }
}

/// The jobs of a level's pool. A `Mutex` + `Condvar` queue because several workers take from it
/// (std's channels have a single receiver); the lock is held only to push or pop.
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct QueueState {
    jobs: VecDeque<GenerationJob>,
    halted: bool,
}

/// A level's generation workers (see the module docs).
pub struct GenerationPool {
    queue: Arc<Queue>,
    threads: Vec<JoinHandle<()>>,
}

impl GenerationPool {
    /// `threads` workers for `generator`, answering on `results`.
    pub fn spawn(
        generator: Arc<LevelGenerator>,
        threads: usize,
        results: Sender<GenerationResult>,
        name: &str,
    ) -> std::io::Result<Self> {
        let queue = Arc::new(Queue::default());
        let threads = (0..threads.max(1))
            .map(|i| {
                let (generator, queue, results) =
                    (generator.clone(), queue.clone(), results.clone());
                std::thread::Builder::new()
                    .name(format!("Worldgen Worker #{i} - {name}"))
                    .spawn(move || work(&generator, &queue, &results))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Self { queue, threads })
    }

    pub fn submit(&self, job: GenerationJob) {
        let mut state = self.queue.state.lock().expect("the queue lock");
        state.jobs.push_back(job);
        drop(state);
        self.queue.ready.notify_one();
    }

    /// Stops taking jobs and waits for the running ones (queued jobs are dropped: their chunks'
    /// holders keep the chunks as they were before the step).
    pub fn halt(&mut self) {
        {
            let mut state = self.queue.state.lock().expect("the queue lock");
            state.halted = true;
            state.jobs.clear();
        }
        self.queue.ready.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for GenerationPool {
    fn drop(&mut self) {
        self.halt();
    }
}

fn work(generator: &LevelGenerator, queue: &Queue, results: &Sender<GenerationResult>) {
    let mut worker = match generator.worker() {
        Ok(worker) => Some(worker),
        Err(e) => {
            tracing::error!("Could not start a world generation worker: {e}");
            None
        }
    };
    loop {
        let job = {
            let mut state = queue.state.lock().expect("the queue lock");
            loop {
                if state.halted {
                    return;
                }
                if let Some(job) = state.jobs.pop_front() {
                    break job;
                }
                state = queue.ready.wait(state).expect("the queue lock");
            }
        };
        let result = match worker.as_mut() {
            Some(worker) => execute(worker, job),
            None => GenerationResult {
                pos: job.pos,
                status: job.status,
                result: Err(Some("no generation worker".into())),
                nanos: 0,
            },
        };
        if results.send(result).is_err() {
            return;
        }
    }
}
