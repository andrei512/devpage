//! A scene's tree as a tree: each node's identity, its place among its
//! siblings, and the index that finds a node by its identity. Nothing here
//! knows what a node draws.

use std::collections::BTreeMap;
use std::fmt;

/// A node's identity: where it came from — the element ids from the design's
/// root through each instance down to the element that made it. It survives
/// every edit that keeps the element: an insert before it, a sibling moved
/// past it, a line added above it.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct NodeId(pub Vec<u32>);

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u32::to_string).collect();
        f.write_str(&parts.join("/"))
    }
}

/// A node's place among its siblings: a key that sorts between its
/// neighbours', chosen when it is placed, so a node placed anywhere — before
/// the first of 100,000 — takes a key of its own and renumbers none. Its
/// digits are bytes compared in order; the last is never zero, so there is
/// always room between two keys.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderKey(Box<[u8]>);

impl fmt::Debug for OrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x?}", self.0)
    }
}

impl OrderKey {
    /// The `i`th of `n` keys spread evenly, in order — for siblings placed
    /// all at once.
    pub fn nth(i: usize, n: usize) -> OrderKey {
        // As few base-255 digits, 1 to 255, as hold `n` keys.
        let mut digits = 1;
        let mut room = 255usize;
        while room < n {
            digits += 1;
            room = room.saturating_mul(255);
        }
        let mut v = i;
        let mut out = vec![0u8; digits];
        for d in out.iter_mut().rev() {
            *d = (v % 255) as u8 + 1;
            v /= 255;
        }
        // `i` fits: room ≥ n > i.
        OrderKey(out.into_boxed_slice())
    }

    /// A key after `a` — or the first, from nothing — and before `b`, or
    /// with nothing after it.
    pub fn between(a: Option<&OrderKey>, b: Option<&OrderKey>) -> OrderKey {
        let a: &[u8] = a.map_or(&[], |k| &k.0);
        let mut b: Option<&[u8]> = b.map(|k| &*k.0);
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let da = a.get(i).copied().unwrap_or(0) as u16;
            let db = match b {
                // `a < b`, so while their digits agree `b` has more.
                Some(b) => b.get(i).copied().unwrap_or(0) as u16,
                None => 256,
            };
            if db > da + 1 {
                out.push(((da + db) / 2) as u8);
                return OrderKey(out.into_boxed_slice());
            }
            out.push(da as u8);
            if db == da + 1 {
                // Anything after `a`'s digits here is below `b`.
                b = None;
            }
            i += 1;
        }
    }
}

/// A node's children in order, each under its order key.
#[derive(Clone, Default)]
pub struct Children<T>(BTreeMap<OrderKey, T>);

impl<T: fmt::Debug> fmt::Debug for Children<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.values()).finish()
    }
}

/// Two lists of children are the same when they hold the same nodes in the
/// same order, whatever keys placed them.
impl<T: PartialEq> PartialEq for Children<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.values().eq(other.0.values())
    }
}

impl<T> Children<T> {
    pub fn new() -> Self {
        Children(BTreeMap::new())
    }

    /// Children placed all at once, keys spread evenly.
    pub fn from_vec(nodes: Vec<T>) -> Self {
        let n = nodes.len();
        Children(
            nodes
                .into_iter()
                .enumerate()
                .map(|(i, t)| (OrderKey::nth(i, n), t))
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The children in order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.0.values()
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> + ExactSizeIterator {
        self.0.values_mut()
    }

    /// The children in order, with their keys.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = (&OrderKey, &T)> {
        self.0.iter()
    }

    pub fn get(&self, key: &OrderKey) -> Option<&T> {
        self.0.get(key)
    }

    pub fn get_mut(&mut self, key: &OrderKey) -> Option<&mut T> {
        self.0.get_mut(key)
    }

    /// The child `at` places from the first, counting each one before it:
    /// for tests, and for callers that hold a place, not a key.
    pub fn nth(&self, at: usize) -> Option<&T> {
        self.0.values().nth(at)
    }

    /// The child `at` places from the first, to change.
    pub fn nth_mut(&mut self, at: usize) -> Option<&mut T> {
        self.0.values_mut().nth(at)
    }

    pub fn first_key(&self) -> Option<&OrderKey> {
        self.0.keys().next()
    }

    pub fn last_key(&self) -> Option<&OrderKey> {
        self.0.keys().next_back()
    }

    /// The key of the child after the one under `key`, if any.
    pub fn key_after(&self, key: &OrderKey) -> Option<&OrderKey> {
        use std::ops::Bound::{Excluded, Unbounded};
        self.0
            .range((Excluded(key), Unbounded))
            .next()
            .map(|(k, _)| k)
    }

    /// The key of the child before the one under `key`, if any.
    pub fn key_before(&self, key: &OrderKey) -> Option<&OrderKey> {
        self.0.range(..key).next_back().map(|(k, _)| k)
    }

    /// Places `t` last, and returns its key.
    pub fn push(&mut self, t: T) -> OrderKey {
        let key = OrderKey::between(self.last_key(), None);
        self.0.insert(key.clone(), t);
        key
    }

    /// Places `t` right after the child under `after`, or first when `None`,
    /// and returns its key.
    pub fn insert_after(&mut self, after: Option<&OrderKey>, t: T) -> OrderKey {
        let next = match after {
            Some(a) => self.key_after(a).cloned(),
            None => self.first_key().cloned(),
        };
        let key = OrderKey::between(after, next.as_ref());
        self.0.insert(key.clone(), t);
        key
    }

    /// Places `t` under `key`, which no child has: where it was taken from.
    pub fn put(&mut self, key: OrderKey, t: T) {
        self.0.insert(key, t);
    }

    pub fn remove(&mut self, key: &OrderKey) -> Option<T> {
        self.0.remove(key)
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// How many children come before the one under `key`: its place.
    pub fn rank(&self, key: &OrderKey) -> usize {
        self.0.range(..key).count()
    }
}

impl<'a, T> IntoIterator for &'a Children<T> {
    type Item = &'a T;
    type IntoIter = std::collections::btree_map::Values<'a, OrderKey, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.values()
    }
}

impl<T> std::ops::Index<usize> for Children<T> {
    type Output = T;
    fn index(&self, at: usize) -> &T {
        self.nth(at).expect("a child at that place")
    }
}

impl<T> FromIterator<T> for Children<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Children::from_vec(iter.into_iter().collect())
    }
}

/// Where a node is in a tree: the order keys from the root down to it — the
/// root's is empty. Sorted, these are the nodes in tree order: a node before
/// its children, children in order.
pub type Order = Vec<OrderKey>;

/// A node of a tree, as far as its shape goes.
pub trait Node: Sized {
    fn id(&self) -> &NodeId;
    fn children(&self) -> &Children<Self>;
}

/// Where each node of a tree is, by its id.
#[derive(Clone, Debug, Default)]
pub struct Ids(std::collections::HashMap<NodeId, Order>);

impl Ids {
    /// Every node of the tree under `root`.
    pub fn of<N: Node>(root: &N) -> Ids {
        let mut ids = Ids::default();
        ids.add(root, Vec::new());
        ids
    }

    /// `n` and the nodes under it, `n` at `order`.
    pub fn add<N: Node>(&mut self, n: &N, order: Order) {
        for (k, c) in n.children().entries() {
            let mut o = order.clone();
            o.push(k.clone());
            self.add(c, o);
        }
        self.0.insert(n.id().clone(), order);
    }

    /// Forgets `n` and the nodes under it.
    pub fn remove<N: Node>(&mut self, n: &N) {
        self.0.remove(n.id());
        for c in n.children() {
            self.remove(c);
        }
    }

    pub fn get(&self, id: &NodeId) -> Option<&Order> {
        self.0.get(id)
    }

    pub fn contains(&self, id: &NodeId) -> bool {
        self.0.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests;
