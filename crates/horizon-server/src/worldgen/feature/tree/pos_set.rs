//! A set of block positions that iterates in the order vanilla's `HashSet<BlockPos>` does.
//!
//! Tree and vegetation features draw random numbers while walking such sets (a tree's trunks and
//! leaves, `updateLeaves`' distance queues, a vegetation patch's ground), so the order is part of
//! the generated world. The order is a pure function of the operations: positions sit in buckets by
//! their spread `Vec3i.hashCode`, a bucket keeps insertion order, the table doubles at 3/4 load
//! (splitting each bucket in order), and a bucket that reaches 9 positions in a table of 64 or more
//! becomes a red-black tree whose linked order depends on its shape. All of that is kept here; the
//! map API around it (values, removal by key, fail-fast iterators) is not, because no feature uses it.
//!
//! Positions are `BlockPos`, which is not `Comparable` as its own class, so vanilla orders two
//! positions with the same hash inside a tree bucket by identity hash codes. Those cannot be
//! reproduced; such ties are ordered by (y, z, x) instead.

use std::cmp::Ordering;

type Link = Option<usize>;

struct Node {
    pos: [i32; 3],
    /// `HashMap.hash`: the hash code with its high half folded into the low one.
    hash: i32,
    next: Link,
    prev: Link,
    parent: Link,
    left: Link,
    right: Link,
    red: bool,
    tree: bool,
}

/// See the module docs. Nodes live in one `Vec` and refer to each other by index; removed slots are
/// reused.
#[derive(Default)]
pub(crate) struct PosSet {
    table: Vec<Link>,
    nodes: Vec<Option<Node>>,
    free: Vec<usize>,
    size: usize,
    threshold: usize,
    /// Every bucket below this one is empty (lets `pop_first` skip them).
    first_bucket: usize,
}

/// `Vec3i.hashCode` spread by `HashMap.hash`.
fn spread_hash([x, y, z]: [i32; 3]) -> i32 {
    let h = y
        .wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x) as u32;
    (h ^ (h >> 16)) as i32
}

/// The order of two positions with the same hash in a tree bucket (see the module docs). Never
/// `Equal`, as `HashMap.tieBreakOrder`.
fn tie_break(a: [i32; 3], b: [i32; 3]) -> Ordering {
    let key = |p: [i32; 3]| (p[1], p[2], p[0]);
    match key(a).cmp(&key(b)) {
        Ordering::Equal => Ordering::Less,
        ordering => ordering,
    }
}

impl PosSet {
    /// Adds `p`; false if it was already there (the order is then unchanged).
    pub(crate) fn insert(&mut self, p: [i32; 3]) -> bool {
        if self.find(p).is_some() {
            return false;
        }
        self.put_new(p);
        true
    }

    pub(crate) fn contains(&self, p: [i32; 3]) -> bool {
        self.find(p).is_some()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// The positions in iteration order.
    pub(crate) fn to_vec(&self) -> Vec<[i32; 3]> {
        self.table
            .iter()
            .flat_map(|first| std::iter::successors(*first, |i| self.node(*i).next))
            .map(|i| self.node(i).pos)
            .collect()
    }

    /// `iterator().next()` followed by `iterator.remove()`.
    pub(crate) fn pop_first(&mut self) -> Option<[i32; 3]> {
        let bucket = (self.first_bucket..self.table.len()).find(|b| self.table[*b].is_some())?;
        self.first_bucket = bucket;
        let first = self.table[bucket]?;
        Some(self.remove_node(bucket, first))
    }

    fn node(&self, i: usize) -> &Node {
        self.nodes[i].as_ref().expect("a live node")
    }

    fn node_mut(&mut self, i: usize) -> &mut Node {
        self.nodes[i].as_mut().expect("a live node")
    }

    fn parent(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).parent)
    }

    fn left(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).left)
    }

    fn right(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).right)
    }

    fn red(&self, i: Link) -> bool {
        i.is_some_and(|i| self.node(i).red)
    }

    fn color(&mut self, i: Link, red: bool) {
        if let Some(i) = i {
            self.node_mut(i).red = red;
        }
    }

    fn bucket(&self, hash: i32) -> usize {
        hash as usize & (self.table.len() - 1)
    }

    fn allocate(&mut self, pos: [i32; 3], hash: i32) -> usize {
        let node = Some(Node {
            pos,
            hash,
            next: None,
            prev: None,
            parent: None,
            left: None,
            right: None,
            red: false,
            tree: false,
        });
        if let Some(i) = self.free.pop() {
            self.nodes[i] = node;
            i
        } else {
            self.nodes.push(node);
            self.nodes.len() - 1
        }
    }

    fn find(&self, pos: [i32; 3]) -> Link {
        if self.table.is_empty() {
            return None;
        }
        let h = spread_hash(pos);
        let mut p = self.table[self.bucket(h)];
        if let Some(first) = p.filter(|i| self.node(*i).tree) {
            return self.find_tree(Some(self.root(first)), h, pos);
        }
        while let Some(i) = p {
            let n = self.node(i);
            if n.hash == h && n.pos == pos {
                return Some(i);
            }
            p = n.next;
        }
        None
    }

    /// `TreeNode.find` for a key that is not `Comparable`: equal hashes search both subtrees.
    fn find_tree(&self, mut p: Link, hash: i32, pos: [i32; 3]) -> Link {
        while let Some(i) = p {
            let node = self.node(i);
            if hash < node.hash {
                p = node.left;
            } else if hash > node.hash {
                p = node.right;
            } else if pos == node.pos {
                return Some(i);
            } else if node.left.is_none() {
                p = node.right;
            } else if node.right.is_none() {
                p = node.left;
            } else {
                if let Some(found) = self.find_tree(node.right, hash, pos) {
                    return Some(found);
                }
                p = node.left;
            }
        }
        None
    }

    fn root(&self, mut p: usize) -> usize {
        while let Some(parent) = self.node(p).parent {
            p = parent;
        }
        p
    }

    fn compare(&self, a: usize, b: usize) -> Ordering {
        let (a, b) = (self.node(a), self.node(b));
        a.hash.cmp(&b.hash).then_with(|| tie_break(a.pos, b.pos))
    }

    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let Some(r) = self.node(p).right else {
            return root;
        };
        let rl = self.node(r).left;
        self.node_mut(p).right = rl;
        if let Some(rl) = rl {
            self.node_mut(rl).parent = Some(p);
        }
        let pp = self.node(p).parent;
        self.node_mut(r).parent = pp;
        if let Some(pp) = pp {
            if self.node(pp).left == Some(p) {
                self.node_mut(pp).left = Some(r);
            } else {
                self.node_mut(pp).right = Some(r);
            }
        } else {
            root = r;
            self.node_mut(r).red = false;
        }
        self.node_mut(r).left = Some(p);
        self.node_mut(p).parent = Some(r);
        root
    }

    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let Some(l) = self.node(p).left else {
            return root;
        };
        let lr = self.node(l).right;
        self.node_mut(p).left = lr;
        if let Some(lr) = lr {
            self.node_mut(lr).parent = Some(p);
        }
        let pp = self.node(p).parent;
        self.node_mut(l).parent = pp;
        if let Some(pp) = pp {
            if self.node(pp).right == Some(p) {
                self.node_mut(pp).right = Some(l);
            } else {
                self.node_mut(pp).left = Some(l);
            }
        } else {
            root = l;
            self.node_mut(l).red = false;
        }
        self.node_mut(l).right = Some(p);
        self.node_mut(p).parent = Some(l);
        root
    }

    fn balance_insert(&mut self, mut root: usize, mut x: usize) -> usize {
        self.node_mut(x).red = true;
        loop {
            let Some(mut p) = self.node(x).parent else {
                self.node_mut(x).red = false;
                return x;
            };
            let Some(mut g) = self.node(p).parent else {
                return root;
            };
            if !self.node(p).red {
                return root;
            }
            let left = self.node(g).left == Some(p);
            let uncle = if left {
                self.node(g).right
            } else {
                self.node(g).left
            };
            if self.red(uncle) {
                self.color(uncle, false);
                self.node_mut(p).red = false;
                self.node_mut(g).red = true;
                x = g;
                continue;
            }
            let inner = if left {
                self.node(p).right
            } else {
                self.node(p).left
            };
            if inner == Some(x) {
                x = p;
                root = if left {
                    self.rotate_left(root, x)
                } else {
                    self.rotate_right(root, x)
                };
                p = self.node(x).parent.expect("rotated under a parent");
                g = self.node(p).parent.expect("rotated under a grandparent");
            }
            self.node_mut(p).red = false;
            self.node_mut(g).red = true;
            root = if left {
                self.rotate_right(root, g)
            } else {
                self.rotate_left(root, g)
            };
        }
    }

    /// `TreeNode.moveRootToFront`: the tree's root becomes the bucket's first position.
    fn move_root_to_front(&mut self, bucket: usize, root: usize) {
        let first = self.table[bucket];
        if first == Some(root) {
            return;
        }
        let (prev, next) = (self.node(root).prev, self.node(root).next);
        if let Some(next) = next {
            self.node_mut(next).prev = prev;
        }
        if let Some(prev) = prev {
            self.node_mut(prev).next = next;
        }
        if let Some(first) = first {
            self.node_mut(first).prev = Some(root);
        }
        self.node_mut(root).next = first;
        self.node_mut(root).prev = None;
        self.table[bucket] = Some(root);
    }

    fn treeify(&mut self, bucket: usize) {
        let mut current = self.table[bucket];
        let mut root = None;
        while let Some(x) = current {
            current = self.node(x).next;
            {
                let n = self.node_mut(x);
                n.tree = true;
                n.left = None;
                n.right = None;
                n.parent = None;
                n.red = false;
            }
            let Some(r) = root else {
                root = Some(x);
                continue;
            };
            let mut p = r;
            loop {
                let left = self.compare(x, p) != Ordering::Greater;
                let child = if left {
                    self.node(p).left
                } else {
                    self.node(p).right
                };
                if let Some(c) = child {
                    p = c;
                    continue;
                }
                self.node_mut(x).parent = Some(p);
                if left {
                    self.node_mut(p).left = Some(x);
                } else {
                    self.node_mut(p).right = Some(x);
                }
                root = Some(self.balance_insert(r, x));
                break;
            }
        }
        if let Some(root) = root {
            self.move_root_to_front(bucket, root);
        }
    }

    fn untreeify(&mut self, mut p: Link) {
        while let Some(i) = p {
            let n = self.node_mut(i);
            n.tree = false;
            n.parent = None;
            n.left = None;
            n.right = None;
            n.red = false;
            p = n.next;
        }
    }

    fn resize(&mut self) {
        let old_len = self.table.len();
        let new_len = if old_len == 0 {
            if self.threshold > 0 {
                self.threshold
            } else {
                16
            }
        } else {
            old_len * 2
        };
        let old = std::mem::replace(&mut self.table, vec![None; new_len]);
        self.threshold = new_len * 3 / 4;
        self.first_bucket = 0;
        for (bucket, first) in old.into_iter().enumerate() {
            let Some(first) = first else {
                continue;
            };
            let tree = self.node(first).tree;
            // Split in order: the low half stays at `bucket`, the high half moves `old_len` up.
            let mut groups = [Vec::new(), Vec::new()];
            let mut p = Some(first);
            while let Some(i) = p {
                p = self.node(i).next;
                groups[usize::from((self.node(i).hash as usize & old_len) != 0)].push(i);
            }
            for side in 0..2 {
                let group = &groups[side];
                if group.is_empty() {
                    continue;
                }
                let at = bucket + side * old_len;
                self.table[at] = Some(group[0]);
                for (j, &i) in group.iter().enumerate() {
                    let n = self.node_mut(i);
                    n.prev = j.checked_sub(1).map(|j| group[j]);
                    n.next = group.get(j + 1).copied();
                }
                if tree {
                    if group.len() <= 6 {
                        self.untreeify(Some(group[0]));
                    } else if !groups[1 - side].is_empty() {
                        self.treeify(at);
                    }
                }
            }
        }
    }

    /// `HashMap.putVal` for a position not in the set.
    fn put_new(&mut self, pos: [i32; 3]) {
        if self.table.is_empty() {
            self.resize();
        }
        let hash = spread_hash(pos);
        let bucket = self.bucket(hash);
        let x = self.allocate(pos, hash);
        self.first_bucket = self.first_bucket.min(bucket);
        match self.table[bucket] {
            None => self.table[bucket] = Some(x),
            Some(first) if self.node(first).tree => self.put_tree(bucket, first, x),
            Some(first) => {
                let mut tail = first;
                let mut count = 1;
                while let Some(next) = self.node(tail).next {
                    tail = next;
                    count += 1;
                }
                self.node_mut(tail).next = Some(x);
                self.node_mut(x).prev = Some(tail);
                if count >= 8 {
                    if self.table.len() < 64 {
                        self.resize();
                    } else {
                        self.treeify(bucket);
                    }
                }
            }
        }
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
    }

    /// `TreeNode.putTreeVal`: the new node goes right after its tree parent in the bucket's order.
    fn put_tree(&mut self, bucket: usize, first: usize, x: usize) {
        self.node_mut(x).tree = true;
        let root = self.root(first);
        let mut p = root;
        loop {
            let left = self.compare(x, p) != Ordering::Greater;
            let child = if left {
                self.node(p).left
            } else {
                self.node(p).right
            };
            if let Some(child) = child {
                p = child;
                continue;
            }
            let next = self.node(p).next;
            {
                let n = self.node_mut(x);
                n.parent = Some(p);
                n.prev = Some(p);
                n.next = next;
            }
            if let Some(next) = next {
                self.node_mut(next).prev = Some(x);
            }
            self.node_mut(p).next = Some(x);
            if left {
                self.node_mut(p).left = Some(x);
            } else {
                self.node_mut(p).right = Some(x);
            }
            let root = self.balance_insert(root, x);
            self.move_root_to_front(bucket, root);
            return;
        }
    }

    fn balance_delete(&mut self, mut root: usize, mut x: Link) -> usize {
        loop {
            let Some(xi) = x else {
                return root;
            };
            if xi == root {
                return root;
            }
            let Some(p) = self.parent(x) else {
                self.color(x, false);
                return xi;
            };
            if self.red(x) {
                self.color(x, false);
                return root;
            }
            let left = self.left(Some(p)) == x;
            let mut sibling = if left {
                self.right(Some(p))
            } else {
                self.left(Some(p))
            };
            if self.red(sibling) {
                self.color(sibling, false);
                self.color(Some(p), true);
                root = if left {
                    self.rotate_left(root, p)
                } else {
                    self.rotate_right(root, p)
                };
                let parent = self.parent(x);
                sibling = if left {
                    self.right(parent)
                } else {
                    self.left(parent)
                };
            }
            let parent = self.parent(x);
            let Some(s) = sibling else {
                x = parent;
                continue;
            };
            let near = if left {
                self.left(sibling)
            } else {
                self.right(sibling)
            };
            let far = if left {
                self.right(sibling)
            } else {
                self.left(sibling)
            };
            if !self.red(near) && !self.red(far) {
                self.color(sibling, true);
                x = parent;
                continue;
            }
            if !self.red(far) {
                self.color(near, false);
                self.color(sibling, true);
                root = if left {
                    self.rotate_right(root, s)
                } else {
                    self.rotate_left(root, s)
                };
                let parent = self.parent(x);
                sibling = if left {
                    self.right(parent)
                } else {
                    self.left(parent)
                };
            }
            let parent = self.parent(x);
            self.color(sibling, self.red(parent));
            let far = if left {
                self.right(sibling)
            } else {
                self.left(sibling)
            };
            self.color(far, false);
            self.color(parent, false);
            if let Some(parent) = parent {
                root = if left {
                    self.rotate_left(root, parent)
                } else {
                    self.rotate_right(root, parent)
                };
            }
            x = Some(root);
        }
    }

    /// `TreeNode.removeTreeNode` as an iterator's `remove` calls it (`movable` false: the bucket is
    /// neither untreeified for being small nor has its root moved to the front).
    fn remove_tree(&mut self, bucket: usize, p: usize) {
        let first = self.table[bucket].expect("a tree bucket");
        let mut root = self.root(first);
        let (prev, next) = (self.node(p).prev, self.node(p).next);
        if let Some(prev) = prev {
            self.node_mut(prev).next = next;
        } else {
            self.table[bucket] = next;
        }
        if let Some(next) = next {
            self.node_mut(next).prev = prev;
        }
        if self.table[bucket].is_none() {
            return;
        }
        let (left, right) = (self.node(p).left, self.node(p).right);
        let replacement;
        if let (Some(left), Some(right)) = (left, right) {
            let mut successor = right;
            while let Some(l) = self.node(successor).left {
                successor = l;
            }
            let red = self.node(successor).red;
            self.node_mut(successor).red = self.node(p).red;
            self.node_mut(p).red = red;
            let sr = self.node(successor).right;
            let pp = self.node(p).parent;
            if successor == right {
                self.node_mut(p).parent = Some(successor);
                self.node_mut(successor).right = Some(p);
            } else {
                let sp = self.node(successor).parent;
                self.node_mut(p).parent = sp;
                if let Some(sp) = sp {
                    if self.node(sp).left == Some(successor) {
                        self.node_mut(sp).left = Some(p);
                    } else {
                        self.node_mut(sp).right = Some(p);
                    }
                }
                self.node_mut(successor).right = Some(right);
                self.node_mut(right).parent = Some(successor);
            }
            self.node_mut(p).left = None;
            self.node_mut(p).right = sr;
            if let Some(sr) = sr {
                self.node_mut(sr).parent = Some(p);
            }
            self.node_mut(successor).left = Some(left);
            self.node_mut(left).parent = Some(successor);
            self.node_mut(successor).parent = pp;
            if let Some(pp) = pp {
                if self.node(pp).left == Some(p) {
                    self.node_mut(pp).left = Some(successor);
                } else {
                    self.node_mut(pp).right = Some(successor);
                }
            } else {
                root = successor;
            }
            replacement = sr.unwrap_or(p);
        } else {
            replacement = left.or(right).unwrap_or(p);
        }
        if replacement != p {
            let pp = self.node(p).parent;
            self.node_mut(replacement).parent = pp;
            if let Some(pp) = pp {
                if self.node(pp).left == Some(p) {
                    self.node_mut(pp).left = Some(replacement);
                } else {
                    self.node_mut(pp).right = Some(replacement);
                }
            } else {
                root = replacement;
                self.node_mut(replacement).red = false;
            }
            let n = self.node_mut(p);
            n.left = None;
            n.right = None;
            n.parent = None;
        }
        if !self.node(p).red {
            self.balance_delete(root, Some(replacement));
        }
        if replacement == p
            && let Some(pp) = self.node(p).parent
        {
            self.node_mut(p).parent = None;
            if self.node(pp).left == Some(p) {
                self.node_mut(pp).left = None;
            } else if self.node(pp).right == Some(p) {
                self.node_mut(pp).right = None;
            }
        }
    }

    fn remove_node(&mut self, bucket: usize, i: usize) -> [i32; 3] {
        if self.node(i).tree {
            self.remove_tree(bucket, i);
        } else {
            let (prev, next) = (self.node(i).prev, self.node(i).next);
            if let Some(prev) = prev {
                self.node_mut(prev).next = next;
            } else {
                self.table[bucket] = next;
            }
            if let Some(next) = next {
                self.node_mut(next).prev = prev;
            }
        }
        self.size -= 1;
        self.free.push(i);
        self.nodes[i].take().expect("a live node").pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl PosSet {
        /// How many buckets are tree bins.
        fn tree_buckets(&self) -> usize {
            self.table
                .iter()
                .flatten()
                .filter(|i| self.node(**i).tree)
                .count()
        }
    }

    #[test]
    fn iterates_in_hash_bucket_order() {
        let mut set = PosSet::default();
        let inserted = [[0, 64, 1], [1, 64, 0], [0, 65, 0], [0, 64, 0]];
        for p in inserted {
            set.insert(p);
        }
        // `BlockPos.hashCode` spread by `h ^ (h >>> 16)` into 16 buckets; ties keep insertion order.
        let mut expected = inserted.to_vec();
        expected.sort_by_key(|p| {
            let h = (p[1] + p[2] * 31) * 31 + p[0];
            (h as u32 ^ (h as u32 >> 16)) & 15
        });
        assert_eq!(set.to_vec(), expected);
        assert!(!set.insert(expected[1]));
        assert_eq!(set.pop_first(), Some(expected[0]));
        assert!(!set.contains(expected[0]));
        assert!(set.contains(expected[1]));
    }

    /// Positions along (1, 2, 1) share a bucket (their hashes step by 1024): the bucket becomes a
    /// tree, and popping everything empties the set in its iteration order.
    #[test]
    fn colliding_positions_make_a_tree_bin() {
        let mut set = PosSet::default();
        for k in 0..64 {
            assert!(set.insert([k, 2 * k, k]));
        }
        assert!(set.tree_buckets() > 0);
        for k in 0..64 {
            assert!(set.contains([k, 2 * k, k]));
        }
        let order = set.to_vec();
        let popped: Vec<_> = std::iter::from_fn(|| set.pop_first()).collect();
        assert_eq!(popped, order);
        assert!(set.is_empty());
    }

    fn parse_pos(text: &str) -> [i32; 3] {
        let v: Vec<i32> = text
            .split(' ')
            .map(|f| f.parse().expect("a coordinate"))
            .collect();
        [v[0], v[1], v[2]]
    }

    #[test]
    #[ignore = "needs HORIZON_POS_SET_ORACLE (LeafPosSetOracle.java on the 26.3 jar)"]
    fn pos_set_matches_java() {
        let path = std::env::var_os("HORIZON_POS_SET_ORACLE").expect("HORIZON_POS_SET_ORACLE");
        let text = std::fs::read_to_string(path).expect("the oracle's output");
        let (mut set, mut case) = (PosSet::default(), String::new());
        let (mut rows, mut cases, mut most_trees) = (0, 0, 0);
        for (n, line) in text.lines().enumerate() {
            let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
            let at = || format!("line {} (case {case})", n + 1);
            match kind {
                "case" => {
                    set = PosSet::default();
                    case = rest.to_owned();
                    cases += 1;
                }
                "insert" => {
                    let (pos, added) = rest.rsplit_once(' ').expect("insert row");
                    assert_eq!(set.insert(parse_pos(pos)), added == "true", "{}", at());
                }
                "pop" => assert_eq!(set.pop_first(), Some(parse_pos(rest)), "{}", at()),
                "order" => {
                    let java: Vec<_> = rest
                        .split(',')
                        .filter(|p| !p.is_empty())
                        .map(parse_pos)
                        .collect();
                    assert_eq!(set.to_vec(), java, "{}", at());
                }
                other => panic!("unknown row {other} at {}", at()),
            }
            most_trees = most_trees.max(set.tree_buckets());
            rows += 1;
        }
        assert!(cases > 0 && most_trees > 0, "the rows reach no tree bin");
        eprintln!("{rows} rows in {cases} cases equal; up to {most_trees} tree bins at once");
    }
}
