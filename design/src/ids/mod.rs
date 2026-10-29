//! Element ids kept across edits: a file's new parse matched against its
//! last, sibling list by sibling list, so an element keeps its id however
//! the lines around it change. Siblings with a `key` are matched by it; the
//! others by a shortest edit script over the list (Myers' diff), comparing
//! each element whole — its name, its own attributes and everything in it —
//! and then, among what is left between two matches, an element with one of
//! the same name, in order, as an edit of it. A matched element's children
//! are matched the same way; an element left over is new and takes the
//! file's next id.
//!
//! The cost is a pass over the file: lists are trimmed of what they share at
//! either end, and Myers' diff, which costs a list's length times its edits,
//! is searched to [`MAX_EDITS`] edits. Past that — a save that rewrote the
//! list — the list is matched by a histogram diff: every element that occurs
//! once on each side is matched wherever it now is, the longest run of them
//! in order splits the list into gaps, and each gap is matched again the same
//! way. That is near-linear however many edits there are, and never pairs
//! the list by position.
//!
//! A store that keeps designs between saves reads its files through
//! [`Remembering`], which keeps each design file's last parse and matches
//! the next against it.

use std::collections::HashMap;

use crate::parse::{Attr, Child, Element, File, Value};

mod remembering;

pub use remembering::{Parses, Remembering};

/// The longest edit script searched for between two lists of siblings.
pub const MAX_EDITS: usize = 256;

/// How deep the histogram diff recurses into the gaps between its matches.
const MAX_DEPTH: usize = 64;

/// `new`, its elements given the ids of the elements of `last` they match,
/// and fresh ones from `last`'s next id where they match none.
pub fn matched(mut new: File, last: &File) -> File {
    let mut next = last.next_id;
    if new.root.name == last.root.name {
        pair(&mut new.root, &last.root, &mut next);
    } else {
        fresh(&mut new.root, &mut next);
    }
    new.next_id = next;
    new
}

/// The key an element names itself by among its siblings, if it has one.
pub fn key(el: &Element) -> Option<&str> {
    el.attrs.iter().find_map(|a| match (&*a.name, &a.value) {
        ("key", Value::Str(s, _)) => Some(s.as_str()),
        _ => None,
    })
}

fn fresh(el: &mut Element, next: &mut u32) {
    el.id = *next;
    *next += 1;
    for child in &mut el.children {
        if let Child::Element(e) = child {
            fresh(e, next);
        }
    }
}

fn pair(new: &mut Element, old: &Element, next: &mut u32) {
    new.id = old.id;
    let olds: Vec<&Element> = elements(&old.children).collect();
    let mut news: Vec<&mut Element> = new
        .children
        .iter_mut()
        .filter_map(|c| match c {
            Child::Element(e) => Some(e),
            Child::Text(..) => None,
        })
        .collect();
    let matches = siblings(&news.iter().map(|e| &**e).collect::<Vec<_>>(), &olds);
    for (n, el) in news.iter_mut().enumerate() {
        match matches[n] {
            Some(o) => pair(el, olds[o], next),
            None => fresh(el, next),
        }
    }
}

fn elements(children: &[Child]) -> impl Iterator<Item = &Element> {
    children.iter().filter_map(|c| match c {
        Child::Element(e) => Some(e),
        Child::Text(..) => None,
    })
}

/// For each of `news`, the one of `olds` it is, if any.
fn siblings(news: &[&Element], olds: &[&Element]) -> Vec<Option<usize>> {
    let mut out = vec![None; news.len()];
    let mut taken = vec![false; olds.len()];
    // By key, wherever each now is.
    let mut keyed: HashMap<(&str, &str), usize> = HashMap::new();
    for (o, el) in olds.iter().enumerate() {
        if let Some(k) = key(el) {
            keyed.entry((&el.name, k)).or_insert(o);
        }
    }
    for (n, el) in news.iter().enumerate() {
        if let Some(k) = key(el)
            && let Some(&o) = keyed.get(&(el.name.as_str(), k))
            && !taken[o]
        {
            out[n] = Some(o);
            taken[o] = true;
        }
    }
    // The rest, by an edit script over the elements without a key.
    let free_new: Vec<usize> = (0..news.len())
        .filter(|&n| out[n].is_none() && key(news[n]).is_none())
        .collect();
    let free_old: Vec<usize> = (0..olds.len())
        .filter(|&o| !taken[o] && key(olds[o]).is_none())
        .collect();
    // The edit script keeps what is the same throughout — its own
    // attributes and everything in it — so a row inserted before a run of
    // rows alike in their own attributes is that row, not a shift of them all.
    let shapes_new: Vec<u64> = free_new.iter().map(|&n| shape(news[n])).collect();
    let shapes_old: Vec<u64> = free_old.iter().map(|&o| shape(olds[o])).collect();
    let same = |n: usize, o: usize| {
        shapes_new[n] == shapes_old[o] && same_own(news[free_new[n]], olds[free_old[o]])
    };
    let pairs = common(
        free_new.len(),
        free_old.len(),
        &same,
        &shapes_new,
        &shapes_old,
    );
    let mut new_taken = vec![false; free_new.len()];
    let mut old_taken = vec![false; free_old.len()];
    for &(n, o) in &pairs {
        out[free_new[n]] = Some(free_old[o]);
        new_taken[n] = true;
        old_taken[o] = true;
    }
    // Between two matches in order, what is left pairs up by name, in order;
    // a match out of order — an element moved — is taken, not a bound.
    let anchors = increasing(&pairs);
    let mut last = (0, 0);
    let ends = anchors
        .iter()
        .copied()
        .chain([(free_new.len(), free_old.len())]);
    for (an, ao) in ends {
        let mut olds_left: Vec<usize> = (last.1..ao).filter(|&o| !old_taken[o]).collect();
        for n in (last.0..an).filter(|&n| !new_taken[n]) {
            let name = &news[free_new[n]].name;
            if let Some(at) = olds_left
                .iter()
                .position(|&o| olds[free_old[o]].name == *name)
            {
                let o = olds_left.remove(at);
                out[free_new[n]] = Some(free_old[o]);
                // What was before it can no longer pair with what is after.
                olds_left.drain(..at);
            }
        }
        last = (an + 1, ao + 1);
    }
    out
}

/// The pairs `(n, o)` of elements of two lists `same` compares that are the
/// same element, sorted by `n`: a longest common subsequence where Myers'
/// diff finds one within [`MAX_EDITS`] edits, and otherwise a histogram
/// diff's matches, which may be out of order — elements moved. `keys` are a
/// hash of each element, which `same` agrees with.
fn common(
    n: usize,
    m: usize,
    same: &dyn Fn(usize, usize) -> bool,
    keys_new: &[u64],
    keys_old: &[u64],
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let news: Vec<usize> = (0..n).collect();
    let olds: Vec<usize> = (0..m).collect();
    diff(&news, &olds, same, keys_new, keys_old, 0, &mut out);
    out.sort_unstable();
    out
}

/// Adds to `out` the matches between the elements `news` and `olds` — each
/// list of indices in order: shared ends trimmed, then Myers' diff, or past
/// its cap the histogram diff, recursing into the gaps between the matches it
/// keeps in order.
fn diff(
    news: &[usize],
    olds: &[usize],
    same: &dyn Fn(usize, usize) -> bool,
    keys_new: &[u64],
    keys_old: &[u64],
    depth: usize,
    out: &mut Vec<(usize, usize)>,
) {
    let (n, m) = (news.len(), olds.len());
    let mut pre = 0;
    while pre < n && pre < m && same(news[pre], olds[pre]) {
        out.push((news[pre], olds[pre]));
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && same(news[n - 1 - suf], olds[m - 1 - suf]) {
        out.push((news[n - 1 - suf], olds[m - 1 - suf]));
        suf += 1;
    }
    let (news, olds) = (&news[pre..n - suf], &olds[pre..m - suf]);
    if news.is_empty() || olds.is_empty() {
        return;
    }
    if let Some(pairs) = myers(news.len(), olds.len(), &|i, j| same(news[i], olds[j])) {
        out.extend(pairs.into_iter().map(|(i, j)| (news[i], olds[j])));
        return;
    }
    // Past the cap: every element that occurs once on each side is matched,
    // wherever it now is.
    let mut count: HashMap<u64, (u32, u32, usize, usize)> = HashMap::new();
    for (i, &x) in news.iter().enumerate() {
        let c = count.entry(keys_new[x]).or_insert((0, 0, 0, 0));
        c.0 += 1;
        c.2 = i;
    }
    for (j, &y) in olds.iter().enumerate() {
        if let Some(c) = count.get_mut(&keys_old[y]) {
            c.1 += 1;
            c.3 = j;
        }
    }
    let mut unique: Vec<(usize, usize)> = count
        .values()
        .filter(|&&(a, b, i, j)| a == 1 && b == 1 && same(news[i], olds[j]))
        .map(|&(_, _, i, j)| (i, j))
        .collect();
    // Gaps nest only as deep as uniqueness keeps appearing inside them; past
    // [`MAX_DEPTH`] what is left pairs by name in place, so no list can
    // recurse without bound.
    if unique.is_empty() || depth == MAX_DEPTH {
        // Nothing tells these apart: what is left pairs by name in place.
        return;
    }
    unique.sort_unstable();
    out.extend(unique.iter().map(|&(i, j)| (news[i], olds[j])));
    // The longest run of them in order splits the lists into gaps, each
    // matched again — without the elements matched out of order.
    let mut used_new = vec![false; news.len()];
    let mut used_old = vec![false; olds.len()];
    for &(i, j) in &unique {
        used_new[i] = true;
        used_old[j] = true;
    }
    let mut last = (0, 0);
    for (i, j) in increasing(&unique)
        .into_iter()
        .chain([(news.len(), olds.len())])
    {
        let gap_new: Vec<usize> = (last.0..i)
            .filter(|&k| !used_new[k])
            .map(|k| news[k])
            .collect();
        let gap_old: Vec<usize> = (last.1..j)
            .filter(|&k| !used_old[k])
            .map(|k| olds[k])
            .collect();
        diff(&gap_new, &gap_old, same, keys_new, keys_old, depth + 1, out);
        last = (i + 1, j + 1);
    }
}

/// A longest run of `pairs` — sorted by their first — increasing in their
/// second too: the matches that are in order. `n log n`.
fn increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // `tails[k]`: the index in `pairs` of the smallest second ending a run of
    // length `k + 1`; `prev`: each pair's predecessor in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; pairs.len()];
    for (at, &(_, o)) in pairs.iter().enumerate() {
        let k = tails.partition_point(|&t| pairs[t].1 < o);
        if k > 0 {
            prev[at] = tails[k - 1];
        }
        if k == tails.len() {
            tails.push(at);
        } else {
            tails[k] = at;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied().unwrap_or(usize::MAX);
    while at != usize::MAX {
        run.push(pairs[at]);
        at = prev[at];
    }
    run.reverse();
    run
}

/// Myers' shortest edit script between two lists of lengths `n` and `m`,
/// as the pairs it keeps; `None` when it would take more than [`MAX_EDITS`].
fn myers(n: usize, m: usize, same: &dyn Fn(usize, usize) -> bool) -> Option<Vec<(usize, usize)>> {
    if n == 0 || m == 0 {
        return Some(Vec::new());
    }
    let max = (n + m).min(MAX_EDITS);
    let off = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let at = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[at - 1] < v[at + 1]) {
                v[at + 1]
            } else {
                v[at - 1] + 1
            };
            let mut y = x - k;
            while (x as usize) < n && y >= 0 && (y as usize) < m && same(x as usize, y as usize) {
                x += 1;
                y += 1;
            }
            v[at] = x;
            if x >= n as isize && y >= m as isize {
                return Some(backtrack(&trace, d, n, m, off));
            }
            k += 2;
        }
    }
    None
}

/// The pairs the edit script found keeps, walked back from its end: each
/// step `d` taken from the state `trace[d]` it was made from.
fn backtrack(
    trace: &[Vec<isize>],
    d: isize,
    n: usize,
    m: usize,
    off: isize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut x, mut y) = (n as isize, m as isize);
    for d in (1..=d).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let at = |k: isize| v[(k + off) as usize];
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        // Where the snake that ends at `(x, y)` starts: just past the move.
        let (sx, sy) = if prev_k == k + 1 {
            (prev_x, prev_y + 1)
        } else {
            (prev_x + 1, prev_y)
        };
        while x > sx && y > sy {
            x -= 1;
            y -= 1;
            out.push((x as usize, y as usize));
        }
        x = prev_x;
        y = prev_y;
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        out.push((x as usize, y as usize));
    }
    out.reverse();
    out
}

/// A hash of an element and everything in it but for where it is written
/// and its id: what two elements the same throughout share.
fn shape(el: &Element) -> u64 {
    use std::hash::{Hash, Hasher};
    fn value(v: &Value, h: &mut std::collections::hash_map::DefaultHasher) {
        match v {
            Value::Str(s, _) => (0u8, s).hash(h),
            Value::Theme(s, _) => (1u8, s).hash(h),
            Value::Num(n) => (2u8, n.to_bits()).hash(h),
            Value::Bool(b) => (3u8, b).hash(h),
            Value::Prop(p) => (4u8, p).hash(h),
            Value::Array(items) => {
                (5u8, items.len()).hash(h);
                for i in items {
                    value(i, h);
                }
            }
            Value::Object(fields) => {
                (6u8, fields.len()).hash(h);
                for (k, v) in fields {
                    k.hash(h);
                    value(v, h);
                }
            }
            other => format!("{other:?}").hash(h),
        }
    }
    fn walk(el: &Element, h: &mut std::collections::hash_map::DefaultHasher) {
        el.name.hash(h);
        el.attrs.len().hash(h);
        for a in &el.attrs {
            a.name.hash(h);
            value(&a.value, h);
        }
        el.children.len().hash(h);
        for c in &el.children {
            match c {
                Child::Element(e) => walk(e, h),
                Child::Text(parts, _) => {
                    for p in parts {
                        match p {
                            crate::parse::TextPart::Literal(s) => (0u8, s).hash(h),
                            crate::parse::TextPart::Prop(s) => (1u8, s).hash(h),
                            crate::parse::TextPart::Theme(s, _) => (2u8, s).hash(h),
                        }
                    }
                }
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    walk(el, &mut h);
    h.finish()
}

/// Whether two elements are the same but for where they are written: the
/// same name and the same attributes, in the same order, with the same
/// values.
pub(crate) fn same_own(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && a.attrs.len() == b.attrs.len()
        && a.attrs.iter().zip(&b.attrs).all(|(x, y)| same_attr(x, y))
}

fn same_attr(a: &Attr, b: &Attr) -> bool {
    a.name == b.name && same_value(&a.value, &b.value)
}

pub(crate) fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x, _), Value::Str(y, _)) => x == y,
        (Value::Theme(x, _), Value::Theme(y, _)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, a), (kb, b))| ka == kb && same_value(a, b))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests;
