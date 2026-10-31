//! `FeatureSorter.buildFeaturesPerStep`: the order features are placed in within each
//! decoration step, so that every biome's own order is kept across all the biomes of a generator.
//!
//! Each biome's features, step after step, are a chain; a feature is a node of its step and its
//! first-encounter index, and the chains' links are the edges of one graph, whose depth-first
//! topological order (nodes visited in step, index order) is the result.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{FeatureRegistry, PlacedFeatureId};

/// A feature in a step, ordered as vanilla's `FeatureData` comparator: step, then the feature's
/// index of first encounter.
type Node = (usize, usize);

/// One decoration step's features in placement order (`StepFeatureData`).
#[derive(Clone, Debug)]
pub struct StepFeatureData {
    features: Vec<PlacedFeatureId>,
    index: HashMap<PlacedFeatureId, usize>,
}

impl StepFeatureData {
    pub fn features(&self) -> &[PlacedFeatureId] {
        &self.features
    }
    /// `indexMapping`: the feature's index in the step's list.
    pub fn index_of(&self, feature: PlacedFeatureId) -> Option<usize> {
        self.index.get(&feature).copied()
    }
}

/// Two biomes want incompatible orders (`Feature order cycle found`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Feature order cycle found")]
pub struct FeatureCycle;

/// `buildFeaturesPerStep` over each source's per-step feature lists.
pub fn build_features_per_step(
    sources: &[Vec<Vec<PlacedFeatureId>>],
) -> Result<Vec<StepFeatureData>, FeatureCycle> {
    let mut first_index = HashMap::new();
    let mut by_index = Vec::new();
    let mut edges: BTreeMap<Node, BTreeSet<Node>> = BTreeMap::new();
    let mut max_step = 0;
    for source in sources {
        max_step = max_step.max(source.len());
        let mut chain = Vec::new();
        for (step, features) in source.iter().enumerate() {
            for &feature in features {
                let index = *first_index.entry(feature).or_insert_with(|| {
                    by_index.push(feature);
                    by_index.len() - 1
                });
                chain.push((step, index));
            }
        }
        for (i, &node) in chain.iter().enumerate() {
            let next = edges.entry(node).or_default();
            if let Some(&following) = chain.get(i + 1) {
                next.insert(following);
            }
        }
    }

    let mut discovered = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    let mut sorted = Vec::new();
    for &node in edges.keys() {
        if !discovered.contains(&node)
            && depth_first(&edges, &mut discovered, &mut visiting, &mut sorted, node)
        {
            return Err(FeatureCycle);
        }
    }
    sorted.reverse();

    let mut steps: Vec<StepFeatureData> = (0..max_step)
        .map(|_| StepFeatureData {
            features: Vec::new(),
            index: HashMap::new(),
        })
        .collect();
    for (step, index) in sorted {
        let feature = by_index[index];
        let data = &mut steps[step];
        data.index.insert(feature, data.features.len());
        data.features.push(feature);
    }
    Ok(steps)
}

/// `Graph.depthFirstSearch`: true when a cycle is found.
fn depth_first(
    edges: &BTreeMap<Node, BTreeSet<Node>>,
    discovered: &mut BTreeSet<Node>,
    visiting: &mut BTreeSet<Node>,
    sorted: &mut Vec<Node>,
    node: Node,
) -> bool {
    if discovered.contains(&node) {
        return false;
    }
    if visiting.contains(&node) {
        return true;
    }
    visiting.insert(node);
    for &next in edges.get(&node).into_iter().flatten() {
        if depth_first(edges, discovered, visiting, sorted, next) {
            return true;
        }
    }
    visiting.remove(&node);
    discovered.insert(node);
    sorted.push(node);
    false
}

impl FeatureRegistry {
    /// A generator's feature order: `FeatureSorter` over the biomes its biome source can
    /// produce (a chunk generator builds it once, from `possibleBiomes()`).
    pub fn features_per_step(&self, biomes: &[u32]) -> Result<Vec<StepFeatureData>, FeatureCycle> {
        let sources: Vec<_> = biomes
            .iter()
            .map(|&b| self.biome(b).steps.clone())
            .collect();
        build_features_per_step(&sources)
    }
}
