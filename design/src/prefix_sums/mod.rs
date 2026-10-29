//! A sequence in key order that knows the summary of every prefix in log
//! time: a treap whose every node holds the summary of its subtree. What a
//! run keeps its children's sizes in, so a child's change of size, an insert
//! before the first of 100,000 or a search for the child at a place along
//! the run costs log time, and moves no sibling.

use std::cmp::Ordering;

/// What a stretch of items adds up to: `then` is the stretch followed by
/// `next`, and `Default` is the empty stretch.
pub trait Summary: Clone + Default {
    fn then(&self, next: &Self) -> Self;
}

/// An item of a [`Sums`] and what it adds up to by itself.
pub trait Item {
    type Sum: Summary;
    fn summary(&self) -> Self::Sum;
}

type Link = Option<u32>;

struct Node<K, V: Item> {
    key: K,
    value: V,
    priority: u64,
    left: Link,
    right: Link,
    /// The summary of this subtree, in key order.
    sum: V::Sum,
    count: usize,
}

/// Items by key, each prefix's summary in log time.
pub struct Sums<K, V: Item> {
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<u32>,
    root: Link,
    seed: u64,
}

impl<K: Ord + Clone, V: Item> Default for Sums<K, V> {
    fn default() -> Self {
        Sums::new()
    }
}

impl<K: Ord + Clone, V: Item> Sums<K, V> {
    pub fn new() -> Self {
        Sums {
            nodes: Vec::new(),
            free: Vec::new(),
            root: None,
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    pub fn len(&self) -> usize {
        self.root.map_or(0, |r| self.node(r).count)
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The summary of every item.
    pub fn total(&self) -> V::Sum {
        self.sum(self.root)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut at = self.root;
        while let Some(i) = at {
            let n = self.node(i);
            at = match key.cmp(&n.key) {
                Ordering::Less => n.left,
                Ordering::Greater => n.right,
                Ordering::Equal => return Some(&n.value),
            };
        }
        None
    }

    /// Puts `value` at `key`, replacing what was there; returns it.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let old = self.remove(&key);
        let (l, r) = self.split(self.root, &key, false);
        let priority = self.next_priority();
        let sum = value.summary();
        let n = self.alloc(Node {
            key,
            value,
            priority,
            left: None,
            right: None,
            sum,
            count: 1,
        });
        let m = self.merge(l, Some(n));
        self.root = self.merge(m, r);
        old
    }

    /// Takes the item at `key` out.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (l, r) = self.split(self.root, key, false);
        let (m, r) = self.split(r, key, true);
        self.root = self.merge(l, r);
        let m = m?;
        let node = self.nodes[m as usize].take().expect("a live node");
        self.free.push(m);
        Some(node.value)
    }

    /// The summary of every item before `key`.
    pub fn before(&self, key: &K) -> V::Sum {
        let mut acc = V::Sum::default();
        let mut at = self.root;
        while let Some(i) = at {
            let n = self.node(i);
            if *key <= n.key {
                at = n.left;
            } else {
                acc = acc.then(&self.sum(n.left)).then(&n.value.summary());
                at = n.right;
            }
        }
        acc
    }

    /// The first item whose prefix — the summary of it and everything before
    /// it — `reaches`, which must be false and then true along the items;
    /// with the summary of everything before it.
    pub fn seek(&self, reaches: impl Fn(&V::Sum) -> bool) -> Option<(&K, &V, V::Sum)> {
        let mut acc = V::Sum::default();
        let mut at = self.root;
        let mut found = None;
        while let Some(i) = at {
            let n = self.node(i);
            let left = acc.then(&self.sum(n.left));
            let with = left.then(&n.value.summary());
            if reaches(&with) {
                found = Some((&n.key, &n.value, left.clone()));
                at = n.left;
            } else {
                acc = with;
                at = n.right;
            }
        }
        found
    }

    /// The items from `key` on, in order.
    pub fn from<'a>(&'a self, key: &K) -> impl Iterator<Item = (&'a K, &'a V)> + 'a {
        let mut stack = Vec::new();
        let mut at = self.root;
        while let Some(i) = at {
            let n = self.node(i);
            if n.key >= *key {
                stack.push(i);
                at = n.left;
            } else {
                at = n.right;
            }
        }
        Walk { sums: self, stack }
    }

    /// Every item, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut stack = Vec::new();
        let mut at = self.root;
        while let Some(i) = at {
            stack.push(i);
            at = self.node(i).left;
        }
        Walk { sums: self, stack }
    }

    /// Every item, last first.
    pub fn rev(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut stack = Vec::new();
        let mut at = self.root;
        while let Some(i) = at {
            stack.push(i);
            at = self.node(i).right;
        }
        Back { sums: self, stack }
    }

    fn node(&self, i: u32) -> &Node<K, V> {
        self.nodes[i as usize].as_ref().expect("a live node")
    }

    fn node_mut(&mut self, i: u32) -> &mut Node<K, V> {
        self.nodes[i as usize].as_mut().expect("a live node")
    }

    fn sum(&self, at: Link) -> V::Sum {
        at.map_or_else(V::Sum::default, |i| self.node(i).sum.clone())
    }

    fn count(&self, at: Link) -> usize {
        at.map_or(0, |i| self.node(i).count)
    }

    fn next_priority(&mut self) -> u64 {
        // xorshift64*: priorities need only be unpredictable to the keys.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn alloc(&mut self, node: Node<K, V>) -> u32 {
        match self.free.pop() {
            Some(i) => {
                self.nodes[i as usize] = Some(node);
                i
            }
            None => {
                self.nodes.push(Some(node));
                (self.nodes.len() - 1) as u32
            }
        }
    }

    fn fix(&mut self, i: u32) {
        let (l, r) = {
            let n = self.node(i);
            (n.left, n.right)
        };
        let sum = self
            .sum(l)
            .then(&self.node(i).value.summary())
            .then(&self.sum(r));
        let count = self.count(l) + 1 + self.count(r);
        let n = self.node_mut(i);
        n.sum = sum;
        n.count = count;
    }

    /// The subtree at `at` split into the keys before `key` and the rest —
    /// or, `through`, the keys up to and including it and the rest.
    fn split(&mut self, at: Link, key: &K, through: bool) -> (Link, Link) {
        let Some(i) = at else {
            return (None, None);
        };
        let goes_left = if through {
            self.node(i).key <= *key
        } else {
            self.node(i).key < *key
        };
        if goes_left {
            let right = self.node(i).right;
            let (l, r) = self.split(right, key, through);
            self.node_mut(i).right = l;
            self.fix(i);
            (Some(i), r)
        } else {
            let left = self.node(i).left;
            let (l, r) = self.split(left, key, through);
            self.node_mut(i).left = r;
            self.fix(i);
            (l, Some(i))
        }
    }

    /// Two subtrees, every key of `a` before every key of `b`, as one.
    fn merge(&mut self, a: Link, b: Link) -> Link {
        match (a, b) {
            (None, b) => b,
            (a, None) => a,
            (Some(x), Some(y)) => {
                if self.node(x).priority > self.node(y).priority {
                    let right = self.node(x).right;
                    let m = self.merge(right, Some(y));
                    self.node_mut(x).right = m;
                    self.fix(x);
                    Some(x)
                } else {
                    let left = self.node(y).left;
                    let m = self.merge(Some(x), left);
                    self.node_mut(y).left = m;
                    self.fix(y);
                    Some(y)
                }
            }
        }
    }
}

struct Walk<'a, K, V: Item> {
    sums: &'a Sums<K, V>,
    stack: Vec<u32>,
}

impl<'a, K: Ord + Clone, V: Item> Iterator for Walk<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.stack.pop()?;
        let n = self.sums.node(i);
        let mut at = n.right;
        while let Some(j) = at {
            self.stack.push(j);
            at = self.sums.node(j).left;
        }
        Some((&n.key, &n.value))
    }
}

struct Back<'a, K, V: Item> {
    sums: &'a Sums<K, V>,
    stack: Vec<u32>,
}

impl<'a, K: Ord + Clone, V: Item> Iterator for Back<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.stack.pop()?;
        let n = self.sums.node(i);
        let mut at = n.left;
        while let Some(j) = at {
            self.stack.push(j);
            at = self.sums.node(j).right;
        }
        Some((&n.key, &n.value))
    }
}

#[cfg(test)]
mod tests;
