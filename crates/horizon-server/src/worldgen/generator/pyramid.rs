//! The chunk status pipeline's shape: 26.3's `ChunkStep`s of the generation and loading pyramids
//! (`ChunkPyramid`: each step's neighbour requirements, accumulated over the steps before it,
//! and the block write radius), `ChunkLevel.generationStatus`, and what Moonrise adds per status
//! (`ChunkTaskScheduler`'s static tables: write radius, parallel capability, empty load statuses,
//! the access radius of each status, `ChunkStep.byRadius`).
//!
//! Plain constant data computed once ([`Pyramids::get`]); vanilla's builder is followed exactly
//! (`ChunkStep.Builder.addRequirement` and `buildAccumulatedDependencies`), so the tables come
//! out as the jar's (`LeafChunkGenOracle` prints them).

use std::sync::LazyLock;

use crate::world::chunk::ChunkStatus;

/// `ChunkStatus.getStatusList()`, by index.
pub const STATUSES: [ChunkStatus; 10] = ChunkStatus::ALL;

/// The status after `status` (`moonrise$getNextStatus`; `FULL` stays `FULL`).
pub fn next_status(status: ChunkStatus) -> ChunkStatus {
    STATUSES[(status as usize + 1).min(STATUSES.len() - 1)]
}

/// The status before `status` (`getParent`; `EMPTY` stays `EMPTY`).
pub fn parent_status(status: ChunkStatus) -> ChunkStatus {
    STATUSES[(status as usize).saturating_sub(1)]
}

/// `ChunkDependencies`: the status required at each distance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependencies(Vec<ChunkStatus>);

impl Dependencies {
    pub fn as_slice(&self) -> &[ChunkStatus] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `getRadius`.
    pub fn radius(&self) -> i32 {
        (self.0.len() as i32 - 1).max(0)
    }
    /// `get(distance)`.
    pub fn get(&self, distance: usize) -> ChunkStatus {
        self.0[distance]
    }
    /// `getRadiusOf`: the largest distance whose requirement is at or after `status`. `None` for
    /// a status past the first requirement (vanilla throws).
    pub fn radius_of(&self, status: ChunkStatus) -> Option<i32> {
        let first = *self.0.first()?;
        if status > first {
            return None;
        }
        Some(
            self.0
                .iter()
                .rposition(|&dependency| dependency >= status)
                .map_or(0, |r| r as i32),
        )
    }
}

/// `ChunkStep` (with Moonrise's `byRadius`).
#[derive(Clone, Debug)]
pub struct ChunkStep {
    pub target: ChunkStatus,
    pub direct: Dependencies,
    pub accumulated: Dependencies,
    /// `blockStateWriteRadius` (-1: none set).
    pub block_state_write_radius: i32,
    by_radius: Vec<ChunkStatus>,
}

impl ChunkStep {
    /// `getAccumulatedRadiusOf`.
    pub fn accumulated_radius_of(&self, status: ChunkStatus) -> i32 {
        if status == self.target {
            0
        } else {
            self.accumulated
                .radius_of(status)
                .expect("a status the step depends on")
        }
    }

    /// `moonrise$getRequiredStatusAtRadius`.
    pub fn required_status_at(&self, radius: i32) -> ChunkStatus {
        self.by_radius[radius as usize]
    }

    /// How far the step reads: `getAccumulatedRadiusOf(EMPTY)`.
    pub fn read_radius(&self) -> i32 {
        self.accumulated_radius_of(ChunkStatus::Empty)
    }
}

/// `ChunkStep.Builder`.
struct StepBuilder<'a> {
    status: ChunkStatus,
    parent: Option<&'a ChunkStep>,
    direct: Vec<ChunkStatus>,
    write_radius: i32,
}

impl<'a> StepBuilder<'a> {
    fn new(status: ChunkStatus, parent: Option<&'a ChunkStep>) -> Self {
        Self {
            status,
            parent,
            direct: parent.map(|p| vec![p.target]).unwrap_or_default(),
            write_radius: -1,
        }
    }

    fn require(mut self, status: ChunkStatus, radius: usize) -> Self {
        assert!(status < self.status, "a step requires earlier statuses");
        let previous = std::mem::take(&mut self.direct);
        let new_len = radius + 1;
        self.direct = if new_len > previous.len() {
            vec![status; new_len]
        } else {
            previous.clone()
        };
        for (slot, &before) in self.direct.iter_mut().zip(&previous).take(new_len) {
            *slot = before.max(status);
        }
        self
    }

    fn write_radius(mut self, radius: i32) -> Self {
        self.write_radius = radius;
        self
    }

    fn parent_radius(&self, status: ChunkStatus) -> usize {
        self.direct.iter().rposition(|&d| d >= status).unwrap_or(0)
    }

    fn build(self) -> ChunkStep {
        let accumulated = match self.parent {
            None => self.direct.clone(),
            Some(parent) => {
                let radius_of_parent = self.parent_radius(parent.target);
                let parent_deps = parent.accumulated.as_slice();
                let len = (radius_of_parent + parent_deps.len()).max(self.direct.len());
                (0..len)
                    .map(|distance| {
                        let in_parent = distance as isize - radius_of_parent as isize;
                        if in_parent < 0 || in_parent as usize >= parent_deps.len() {
                            self.direct[distance]
                        } else if distance >= self.direct.len() {
                            parent_deps[in_parent as usize]
                        } else {
                            self.direct[distance].max(parent_deps[in_parent as usize])
                        }
                    })
                    .collect()
            }
        };
        let mut step = ChunkStep {
            target: self.status,
            direct: Dependencies(self.direct),
            accumulated: Dependencies(accumulated),
            block_state_write_radius: self.write_radius,
            by_radius: Vec::new(),
        };
        // Moonrise's `byRadius`.
        let size = step.read_radius() as usize + 1;
        let mut by_radius: Vec<Option<ChunkStatus>> = vec![None; size];
        by_radius[0] = Some(parent_status(step.target));
        let mut status = parent_status(step.target);
        while status != ChunkStatus::Empty {
            let radius = step.accumulated_radius_of(status) as usize;
            for slot in by_radius.iter_mut().take(radius + 1) {
                slot.get_or_insert(status);
            }
            status = parent_status(status);
        }
        step.by_radius = by_radius
            .into_iter()
            .map(|s| s.unwrap_or(ChunkStatus::Empty))
            .collect();
        step
    }
}

/// `ChunkPyramid`: one step per status.
#[derive(Clone, Debug)]
pub struct ChunkPyramid {
    steps: Vec<ChunkStep>,
}

impl ChunkPyramid {
    fn build(requirements: impl Fn(ChunkStatus, StepBuilder<'_>) -> StepBuilder<'_>) -> Self {
        let mut steps: Vec<ChunkStep> = Vec::with_capacity(STATUSES.len());
        for status in STATUSES {
            let step = requirements(status, StepBuilder::new(status, steps.last())).build();
            steps.push(step);
        }
        Self { steps }
    }

    /// `getStepTo`.
    pub fn step_to(&self, status: ChunkStatus) -> &ChunkStep {
        &self.steps[status as usize]
    }
}

/// Moonrise's per-status facts (`ChunkTaskScheduler`'s static block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusFacts {
    /// `moonrise$getWriteRadius`.
    pub write_radius: i32,
    /// `moonrise$isParallelCapable`.
    pub parallel: bool,
    /// `moonrise$isEmptyLoadStatus`: loading through it does nothing.
    pub empty_load: bool,
}

/// The two pyramids and Moonrise's tables.
#[derive(Debug)]
pub struct Pyramids {
    pub generation: ChunkPyramid,
    pub loading: ChunkPyramid,
    access_radius: [i32; 10],
}

static PYRAMIDS: LazyLock<Pyramids> = LazyLock::new(Pyramids::build);

impl Pyramids {
    pub fn get() -> &'static Pyramids {
        &PYRAMIDS
    }

    fn build() -> Self {
        use ChunkStatus::*;
        let generation = ChunkPyramid::build(|status, s| match status {
            StructureReferences | Biomes => s.require(StructureStarts, 8),
            Terrain => s
                .require(StructureStarts, 8)
                .require(Biomes, 1)
                .write_radius(0),
            Features => s
                .require(StructureStarts, 8)
                .require(Terrain, 1)
                .write_radius(1),
            Light => s.require(InitializeLight, 1),
            Spawn => s.require(Biomes, 1),
            _ => s,
        });
        let loading = ChunkPyramid::build(|_, s| s);
        let mut access_radius = [-1; 10];
        for status in STATUSES {
            let radius = |pyramid: &ChunkPyramid| {
                if status == Empty {
                    return 0;
                }
                let step = pyramid.step_to(status);
                let radius = step.read_radius();
                (0..=radius).fold(radius, |max, dist| {
                    let required = step.required_status_at(dist);
                    let r = access_radius[required as usize];
                    assert!(r != -1, "an earlier status' radius");
                    max.max(dist + r)
                })
            };
            access_radius[status as usize] = radius(&loading).max(radius(&generation));
        }
        Self {
            generation,
            loading,
            access_radius,
        }
    }

    /// `ChunkTaskScheduler.getAccessRadius(status)`.
    pub fn access_radius(&self, status: ChunkStatus) -> i32 {
        self.access_radius[status as usize]
    }

    /// `ChunkLevel.RADIUS_AROUND_FULL_CHUNK`.
    pub fn radius_around_full_chunk(&self) -> i32 {
        self.generation
            .step_to(ChunkStatus::Full)
            .accumulated
            .radius()
    }

    /// `ChunkLevel.generationStatus(level)`: the highest status a chunk at that ticket level
    /// may be generated to, `None` past `MAX_LEVEL`.
    pub fn generation_status(&self, level: u8) -> Option<ChunkStatus> {
        let distance = i32::from(level) - 33;
        if distance > self.radius_around_full_chunk() {
            None
        } else if distance <= 0 {
            Some(ChunkStatus::Full)
        } else {
            Some(
                self.generation
                    .step_to(ChunkStatus::Full)
                    .accumulated
                    .get(distance as usize),
            )
        }
    }

    /// `ChunkLevel.byStatus`.
    pub fn level_for(&self, status: ChunkStatus) -> u8 {
        33 + self
            .generation
            .step_to(ChunkStatus::Full)
            .accumulated_radius_of(status) as u8
    }
}

/// Moonrise's per-status facts (see [`StatusFacts`]).
pub fn status_facts(status: ChunkStatus) -> StatusFacts {
    use ChunkStatus::*;
    StatusFacts {
        write_radius: match status {
            Features => 1,
            Light => 2,
            _ => 0,
        },
        parallel: matches!(
            status,
            Empty | StructureStarts | StructureReferences | Biomes | Terrain | InitializeLight
        ),
        empty_load: matches!(
            status,
            Empty | StructureReferences | Biomes | Terrain | Features | Spawn
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ChunkStatus::*;

    #[test]
    fn the_full_step_needs_eleven_chunks_around() {
        let p = Pyramids::get();
        let full = p.generation.step_to(Full);
        assert_eq!(
            full.accumulated.as_slice(),
            [
                Spawn,
                InitializeLight,
                Terrain,
                Biomes,
                StructureStarts,
                StructureStarts,
                StructureStarts,
                StructureStarts,
                StructureStarts,
                StructureStarts,
                StructureStarts,
                StructureStarts
            ]
        );
        assert_eq!(p.radius_around_full_chunk(), 11);
        assert_eq!(p.generation_status(33), Some(Full));
        assert_eq!(p.generation_status(30), Some(Full));
        assert_eq!(p.generation_status(34), Some(InitializeLight));
        assert_eq!(p.generation_status(44), Some(StructureStarts));
        assert_eq!(p.generation_status(45), None);
        assert_eq!(p.level_for(Full), 33);
        assert_eq!(p.level_for(StructureStarts), 44);
        assert_eq!(p.level_for(Empty), 44);
    }

    #[test]
    fn steps_read_their_requirements_and_moonrise_tables_follow() {
        let p = Pyramids::get();
        // FEATURES reads 10 chunks: TERRAIN's own 9 (biomes 1, then starts 8) one chunk further.
        let features = p.generation.step_to(Features);
        assert_eq!(features.read_radius(), 10);
        assert_eq!(features.required_status_at(0), Terrain);
        assert_eq!(features.required_status_at(1), Terrain);
        assert_eq!(features.required_status_at(2), Biomes);
        assert_eq!(features.required_status_at(3), StructureStarts);
        assert_eq!(features.block_state_write_radius, 1);
        let terrain = p.generation.step_to(Terrain);
        assert_eq!(terrain.required_status_at(0), Biomes);
        assert_eq!(terrain.required_status_at(1), Biomes);
        assert_eq!(terrain.required_status_at(9), StructureStarts);
        assert_eq!(terrain.read_radius(), 9);
        let light = p.generation.step_to(Light);
        assert_eq!(light.read_radius(), 11);
        assert_eq!(light.required_status_at(1), InitializeLight);
        assert_eq!(light.required_status_at(2), Terrain);
        // The loading pyramid needs no neighbours.
        assert!(
            STATUSES
                .iter()
                .all(|&s| p.loading.step_to(s).read_radius() == 0)
        );
        assert_eq!(p.access_radius(Empty), 0);
        assert_eq!(p.access_radius(StructureStarts), 0);
        assert_eq!(p.access_radius(StructureReferences), 8);
        assert_eq!(p.access_radius(Biomes), 8);
        assert_eq!(p.access_radius(Terrain), 9);
        assert_eq!(p.access_radius(Features), 10);
        assert_eq!(p.access_radius(Light), 11);
        assert_eq!(p.access_radius(Full), 11);
        assert!(status_facts(Terrain).parallel && !status_facts(Features).parallel);
        assert_eq!(status_facts(Light).write_radius, 2);
        assert!(status_facts(Spawn).empty_load && !status_facts(InitializeLight).empty_load);
    }
}
