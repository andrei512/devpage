//! Element ids kept across edits: a file's new parse matched against its
//! last, sibling list by sibling list, so an element keeps its id however
//! the lines around it change. Siblings with a `key` are matched by it; the
//! others by a shortest edit script over the list (Myers' diff), comparing
//! each element's name and own attributes, and then, among what is left
//! between two matches, an element with one of the same name, in order, as
//! an edit of it. A matched element's children are matched the same way; an
//! element left over is new and takes the file's next id.
//!
//! The cost is a pass over the file: lists are trimmed of what they share at
//! either end, and an edit script longer than [`MAX_EDITS`] gives way to
//! matching the rest in order, so no list costs more than its length times
//! that.

use std::collections::HashMap;

use crate::parse::{Attr, Child, Element, File, Value};

/// The longest edit script searched for between two lists of siblings.
pub const MAX_EDITS: usize = 256;

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
    let same = |n: usize, o: usize| same_own(news[free_new[n]], olds[free_old[o]]);
    let anchors = common(free_new.len(), free_old.len(), &same);
    // Between two anchors, what is left pairs up by name, in order.
    let mut last = (0, 0);
    let ends = anchors
        .iter()
        .copied()
        .chain([(free_new.len(), free_old.len())]);
    for (an, ao) in ends {
        let mut olds_left: Vec<usize> = (last.1..ao).collect();
        for n in last.0..an {
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
        if an < free_new.len() {
            out[free_new[an]] = Some(free_old[ao]);
        }
        last = (an + 1, ao + 1);
    }
    out
}

/// The pairs `(n, o)`, in order, of a longest common subsequence of two
/// lists `same` compares — shared ends trimmed first, then Myers' O(ND)
/// diff, up to [`MAX_EDITS`] edits; past that the middle has no pairs, and
/// its elements pair by name in order.
fn common(n: usize, m: usize, same: &dyn Fn(usize, usize) -> bool) -> Vec<(usize, usize)> {
    let mut pre = 0;
    while pre < n && pre < m && same(pre, pre) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && same(n - 1 - suf, m - 1 - suf) {
        suf += 1;
    }
    let mut out: Vec<(usize, usize)> = (0..pre).map(|i| (i, i)).collect();
    let (a, b) = (n - pre - suf, m - pre - suf);
    let mid = myers(a, b, &|i, j| same(pre + i, pre + j));
    out.extend(mid.into_iter().map(|(i, j)| (pre + i, pre + j)));
    out.extend((0..suf).map(|i| (n - suf + i, m - suf + i)));
    out
}

/// Myers' shortest edit script between two lists of lengths `n` and `m`,
/// as the pairs it keeps; none when it would take more than [`MAX_EDITS`].
fn myers(n: usize, m: usize, same: &dyn Fn(usize, usize) -> bool) -> Vec<(usize, usize)> {
    if n == 0 || m == 0 {
        return Vec::new();
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
                return backtrack(&trace, d, n, m, off);
            }
            k += 2;
        }
    }
    Vec::new()
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

/// Whether two elements are the same but for where they are written: the
/// same name and the same attributes, in the same order, with the same
/// values.
fn same_own(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && a.attrs.len() == b.attrs.len()
        && a.attrs.iter().zip(&b.attrs).all(|(x, y)| same_attr(x, y))
}

fn same_attr(a: &Attr, b: &Attr) -> bool {
    a.name == b.name && same_value(&a.value, &b.value)
}

fn same_value(a: &Value, b: &Value) -> bool {
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

#[cfg(test)]
pub(crate) mod testing;
