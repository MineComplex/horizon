//! `Climate.RTree`: the parameter list's search tree, built once and immutable. The previous
//! result that seeds each search (vanilla's `lastResult`, Moonrise's `lastReturned`: both
//! thread-locals per tree) is a [`ClimateSearch`] owned by the worker.
//!
//! Moonrise replaces vanilla's `search` with `findNearest`: a depth-first walk keeping the best
//! leaf so far, entering a child only when its distance is strictly smaller. Vanilla's recursion
//! passes the same best leaf down and keeps a child's result only when strictly better, so both
//! visit the same nodes and return the same leaf; this is that walk.

use std::cmp::Ordering;

use super::{Parameter, ParameterPoint, TargetPoint};

/// Why a tree could not be built (vanilla throws for an empty list; the other limits keep node
/// indices in `u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    #[error("Need at least one value to build the search tree.")]
    Empty,
    #[error("a climate parameter list is limited to 65536 entries")]
    TooManyEntries,
    #[error("climate branching must be in 2..=32")]
    InvalidBranching,
}

/// A node while building: its space and its children (an entry index for a leaf).
#[derive(Debug)]
struct BuildNode {
    space: [Parameter; 7],
    children: Option<Vec<usize>>,
    entry: usize,
}

/// `Node.distance` (Moonrise's loop over the same sum).
fn distance(space: &[Parameter; 7], values: &[i64; 7]) -> i64 {
    space
        .iter()
        .zip(values)
        .fold(0i64, |sum, (parameter, value)| {
            let distance = parameter.distance(*value);
            sum.wrapping_add(distance.wrapping_mul(distance))
        })
}

/// `RTree.comparator(dimension, absolute)` chained over the following dimensions.
fn compare_space(a: &[Parameter; 7], b: &[Parameter; 7], axis: usize, absolute: bool) -> Ordering {
    let key = |p: Parameter| {
        if absolute {
            p.center().wrapping_abs()
        } else {
            p.center()
        }
    };
    (0..7)
        .map(|i| (axis + i) % 7)
        .map(|d| key(a[d]).cmp(&key(b[d])))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// `buildParameterSpace`: the union of the children's spaces.
fn union_space(nodes: &[BuildNode], children: &[usize]) -> [Parameter; 7] {
    let mut space = nodes[children[0]].space;
    for child in &children[1..] {
        for (parameter, other) in space.iter_mut().zip(nodes[*child].space) {
            *parameter = parameter.union(other);
        }
    }
    space
}

/// `RTree.build`.
fn build(nodes: &mut Vec<BuildNode>, mut children: Vec<usize>, branching: usize) -> usize {
    if children.len() == 1 {
        return children[0];
    }
    if children.len() <= branching {
        // A stable sort by total |center|.
        children.sort_by_key(|id| {
            nodes[*id]
                .space
                .iter()
                .fold(0i64, |sum, p| sum.wrapping_add(p.center().wrapping_abs()))
        });
    } else {
        // `pow(branching, floor(log(n - 0.01) / log(branching)))`: the largest power of the
        // branching below n, computed exactly instead of through double logarithms.
        let mut bucket_size = 1;
        while bucket_size * branching < children.len() {
            bucket_size *= branching;
        }
        let mut best: Option<(i64, usize, Vec<Vec<usize>>)> = None;
        for axis in 0..7 {
            children.sort_by(|a, b| compare_space(&nodes[*a].space, &nodes[*b].space, axis, false));
            let buckets: Vec<Vec<usize>> =
                children.chunks(bucket_size).map(<[_]>::to_vec).collect();
            let cost = buckets.iter().fold(0i64, |sum, bucket| {
                union_space(nodes, bucket).iter().fold(sum, |sum, p| {
                    sum.wrapping_add(p.max.wrapping_sub(p.min).wrapping_abs())
                })
            });
            if best
                .as_ref()
                .is_none_or(|(best_cost, ..)| *best_cost > cost)
            {
                best = Some((cost, axis, buckets));
            }
        }
        let (_, axis, mut buckets) = best.expect("seven axes tried");
        let spaces: Vec<_> = buckets.iter().map(|b| union_space(nodes, b)).collect();
        let mut order: Vec<usize> = (0..buckets.len()).collect();
        order.sort_by(|a, b| compare_space(&spaces[*a], &spaces[*b], axis, true));
        children = order
            .into_iter()
            .map(|i| build(nodes, std::mem::take(&mut buckets[i]), branching))
            .collect();
    }
    let space = union_space(nodes, &children);
    nodes.push(BuildNode {
        space,
        children: Some(children),
        entry: 0,
    });
    nodes.len() - 1
}

/// `Climate.ParameterList`'s values and their tree. Immutable; any number of workers search it,
/// each with its own [`ClimateSearch`].
#[derive(Clone, Debug)]
pub struct ClimateIndex<T> {
    entries: Vec<(ParameterPoint, T)>,
    /// The tree renumbered breadth first, root at 0, so each branch's children are one run.
    spaces: Vec<[Parameter; 7]>,
    /// Per node: its first child and child count, or its entry and 0 for a leaf.
    links: Vec<(u32, u32)>,
}

/// A worker's previous result in one [`ClimateIndex`] (vanilla's thread-local `lastResult`).
/// Use one per worker and index: a search starts from the leaf the last one returned, which
/// decides between equally distant leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClimateSearch {
    last: Option<u32>,
}

impl<T> ClimateIndex<T> {
    /// `new ParameterList(values)`: a tree with 19 children per node.
    pub fn new(entries: Vec<(ParameterPoint, T)>) -> Result<Self, IndexError> {
        Self::with_branching(entries, 19)
    }

    pub fn with_branching(
        entries: Vec<(ParameterPoint, T)>,
        branching: usize,
    ) -> Result<Self, IndexError> {
        if entries.is_empty() {
            return Err(IndexError::Empty);
        }
        if entries.len() > 65536 {
            return Err(IndexError::TooManyEntries);
        }
        if !(2..=32).contains(&branching) {
            return Err(IndexError::InvalidBranching);
        }
        let mut nodes: Vec<BuildNode> = entries
            .iter()
            .enumerate()
            .map(|(entry, (point, _))| BuildNode {
                space: point.parameter_space(),
                children: None,
                entry,
            })
            .collect();
        let leaves = (0..nodes.len()).collect();
        let root = build(&mut nodes, leaves, branching);
        let mut order = vec![root];
        let mut links = Vec::with_capacity(nodes.len());
        let mut next = 0;
        while next < order.len() {
            let node = &nodes[order[next]];
            next += 1;
            links.push(match &node.children {
                None => (node.entry as u32, 0),
                Some(children) => {
                    let first = order.len() as u32;
                    order.extend_from_slice(children);
                    (first, children.len() as u32)
                }
            });
        }
        Ok(Self {
            spaces: order.iter().map(|id| nodes[*id].space).collect(),
            entries,
            links,
        })
    }

    /// The values in list order.
    pub fn entries(&self) -> &[(ParameterPoint, T)] {
        &self.entries
    }

    /// `findValue` (Moonrise's `findNearest`), starting from and updating `search`.
    pub fn find(&self, search: &mut ClimateSearch, target: TargetPoint) -> &T {
        if let (entry, 0) = self.links[0] {
            // A single-entry tree: the root is the leaf.
            return &self.entries[entry as usize].1;
        }
        let values = target.parameters();
        let mut best = match search.last {
            Some(leaf) => (
                Some(leaf as usize),
                distance(&self.spaces[leaf as usize], &values),
            ),
            None => (None, i64::MAX),
        };
        self.nearest(0, &values, &mut best);
        // Vanilla fails the same way (a null leaf) when every distance is `Long.MAX_VALUE`.
        let leaf = best.0.expect("a leaf nearer than i64::MAX");
        search.last = Some(leaf as u32);
        &self.entries[self.links[leaf].0 as usize].1
    }

    fn nearest(&self, node: usize, values: &[i64; 7], best: &mut (Option<usize>, i64)) {
        let (first, count) = self.links[node];
        for child in first as usize..(first + count) as usize {
            let child_distance = distance(&self.spaces[child], values);
            if child_distance >= best.1 {
                continue;
            }
            if self.links[child].1 == 0 {
                *best = (Some(child), child_distance);
            } else {
                self.nearest(child, values, best);
            }
        }
    }

    /// `findValueBruteForce`: the first entry of least fitness, in list order (a reference path
    /// that neither reads nor moves any search state).
    pub fn find_brute_force(&self, target: TargetPoint) -> &T {
        let mut best = 0;
        let mut fitness = self.entries[0].0.fitness(target);
        for (i, (point, _)) in self.entries.iter().enumerate().skip(1) {
            let next = point.fitness(target);
            if next < fitness {
                fitness = next;
                best = i;
            }
        }
        &self.entries[best].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::random::{LegacyRandom, RandomSource};

    fn point(values: [i64; 6], width: i64, offset: i64) -> ParameterPoint {
        ParameterPoint {
            coordinates: values.map(|v| Parameter::new(v, v + width)),
            offset,
        }
    }

    #[test]
    fn the_tree_finds_the_least_fitness() {
        let mut random = LegacyRandom::new(42);
        let mut coordinate = || i64::from(random.next_int_bound(40_001).unwrap()) - 20_000;
        let entries: Vec<_> = (0..700)
            .map(|i| {
                let values = std::array::from_fn(|_| coordinate());
                (point(values, (i % 5) * 1000, (i % 3) * 100), i)
            })
            .collect();
        let index = ClimateIndex::new(entries).unwrap();
        let mut search = ClimateSearch::default();
        for _ in 0..2000 {
            let target = TargetPoint {
                values: std::array::from_fn(|_| coordinate()),
            };
            let found = *index.find(&mut search, target);
            let brute = *index.find_brute_force(target);
            assert_eq!(
                index.entries()[found as usize].0.fitness(target),
                index.entries()[brute as usize].0.fitness(target)
            );
        }
    }

    #[test]
    fn single_entries_and_limits() {
        let index = ClimateIndex::new(vec![(point([0; 6], 0, 0), 'a')]).unwrap();
        let target = TargetPoint { values: [9; 6] };
        assert_eq!(*index.find(&mut ClimateSearch::default(), target), 'a');
        assert_eq!(
            ClimateIndex::<u8>::new(Vec::new()).unwrap_err(),
            IndexError::Empty
        );
    }

    #[test]
    fn ties_keep_the_previous_result() {
        // Two identical points: the first search takes the first in tree order; a search
        // seeded with the other keeps it.
        let entries = vec![
            (point([0; 6], 0, 0), 0),
            (point([0; 6], 0, 0), 1),
            (point([5000; 6], 0, 0), 2),
        ];
        let index = ClimateIndex::new(entries).unwrap();
        let target = TargetPoint { values: [0; 6] };
        let mut search = ClimateSearch::default();
        let first = *index.find(&mut search, target);
        let far = TargetPoint { values: [5000; 6] };
        assert_eq!(*index.find(&mut search, far), 2);
        assert_eq!(*index.find(&mut search, target), first);
    }
}
