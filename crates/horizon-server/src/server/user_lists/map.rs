//! The iteration order of a `java.util.concurrent.ConcurrentHashMap<String, V>`, which is what
//! Paper's `StoredUserList` keeps its entries in (`Maps.newConcurrentMap()`): the order the lists are
//! saved in. Only the order is modelled, as a table of bins in `ConcurrentHashMap`'s own layout.
//!
//! Checked against the real class (`LeafUserListsOracle.java`, rows `chm`). Not modelled: a bin that
//! reaches 8 entries is treeified (or, in a table under 64, the table grows first); for UUID and
//! address keys that needs 8 keys on one bin.

use crate::registry::nbt_json::string_hash;

/// A bin's entry; `hash` is `ConcurrentHashMap.spread(key.hashCode())`.
struct Node<V> {
    hash: u32,
    key: String,
    value: V,
}

/// `ConcurrentHashMap`: no table until the first `put` (then 16 bins), twice the bins whenever the
/// size reaches 3/4 of them, never smaller (`clear` and removals keep the table).
pub struct ConcurrentMap<V> {
    table: Vec<Vec<Node<V>>>,
    len: usize,
}

impl<V> Default for ConcurrentMap<V> {
    fn default() -> Self {
        Self {
            table: Vec::new(),
            len: 0,
        }
    }
}

/// `ConcurrentHashMap.spread`.
fn spread(key: &str) -> u32 {
    let h = string_hash(key) as u32;
    (h ^ (h >> 16)) & 0x7fff_ffff
}

impl<V> ConcurrentMap<V> {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn bin(&self, hash: u32) -> Option<usize> {
        (!self.table.is_empty()).then(|| hash as usize & (self.table.len() - 1))
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        let hash = spread(key);
        let bin = self.bin(hash)?;
        self.table[bin]
            .iter()
            .find(|n| n.hash == hash && n.key == key)
            .map(|n| &n.value)
    }

    /// `put`: an existing key keeps its place; a new one goes to the end of its bin.
    pub fn put(&mut self, key: String, value: V) {
        if self.table.is_empty() {
            self.table = (0..16).map(|_| Vec::new()).collect();
        }
        let hash = spread(&key);
        let bin = hash as usize & (self.table.len() - 1);
        if let Some(node) = self.table[bin]
            .iter_mut()
            .find(|n| n.hash == hash && n.key == key)
        {
            node.value = value;
            return;
        }
        self.table[bin].push(Node { hash, key, value });
        self.len += 1;
        // `addCount`: `sizeCtl` is n - n/4, and a size at or over it resizes.
        while self.len >= self.table.len() - self.table.len() / 4 {
            self.transfer();
        }
    }

    /// `ConcurrentHashMap.transfer` to twice the bins. A bin splits by the old size's bit of the hash.
    /// The run of entries at the end of the bin that share a side keeps its order; the entries before
    /// it are put in front of their side one by one, so they come out reversed.
    fn transfer(&mut self) {
        let n = self.table.len();
        let mut next: Vec<Vec<Node<V>>> = (0..2 * n).map(|_| Vec::new()).collect();
        for (i, bin) in std::mem::take(&mut self.table).into_iter().enumerate() {
            if bin.is_empty() {
                continue;
            }
            let high = |node: &Node<V>| node.hash as usize & n != 0;
            let mut run_high = high(&bin[0]);
            let mut last_run = 0;
            for (j, node) in bin.iter().enumerate().skip(1) {
                if high(node) != run_high {
                    run_high = high(node);
                    last_run = j;
                }
            }
            let (mut low_side, mut high_side) = (Vec::new(), Vec::new());
            for (j, node) in bin.into_iter().enumerate() {
                let side = if j >= last_run {
                    // The run: its side's tail, in order.
                    if run_high {
                        &mut high_side
                    } else {
                        &mut low_side
                    }
                } else if high(&node) {
                    &mut high_side
                } else {
                    &mut low_side
                };
                if j >= last_run {
                    side.push(node);
                } else {
                    side.insert(0, node);
                }
            }
            next[i] = low_side;
            next[i + n] = high_side;
        }
        self.table = next;
    }

    /// `remove`.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let hash = spread(key);
        let bin = self.bin(hash)?;
        let at = self.table[bin]
            .iter()
            .position(|n| n.hash == hash && n.key == key)?;
        self.len -= 1;
        Some(self.table[bin].remove(at).value)
    }

    /// `clear`: the table keeps its size.
    pub fn clear(&mut self) {
        for bin in &mut self.table {
            bin.clear();
        }
        self.len = 0;
    }

    /// Bins in order, each from its head: the order of `values()` and `keySet()`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.table
            .iter()
            .flatten()
            .map(|n| (n.key.as_str(), &n.value))
    }

    /// `values().removeIf(test)`.
    pub fn remove_if(&mut self, test: impl Fn(&V) -> bool) {
        for bin in &mut self.table {
            let before = bin.len();
            bin.retain(|n| !test(&n.value));
            self.len -= before - bin.len();
        }
    }
}
