//! Comparing two drawings of the same scene, by the engine's one rule.

use crate::resolve::{self};

/// A PNG's or JPEG's pixels, straight RGBA.
/// How far apart two drawings of the same scene may be. Two rasterizers of one
/// display list differ only in how they anti-alias an edge: a pixel on an edge
/// takes some value between the colors either side of it, and each rasterizer
/// picks a slightly different one. So a pixel is **unexplained** when, in some
/// channel, its value lies more than [`EDGE`] outside the range of the other
/// drawing's pixels around it (itself and its eight neighbours), in either
/// direction. A missing or moved part — a divider, an icon, a word — leaves
/// pixels no neighbour in the other drawing explains. At most [`SHARE`] of the
/// pixels may be unexplained, and no 8-connected cluster of them may be larger
/// than [`CLUSTER`].
pub const EDGE: u8 = 8;
/// The largest share of pixels that may be unexplained.
pub const SHARE: f64 = 0.0005;
/// The largest cluster of touching unexplained pixels.
pub const CLUSTER: usize = 4;

/// How two drawings of the same size differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difference {
    /// The share of pixels that are unexplained.
    pub share: f64,
    /// The size of the largest 8-connected cluster of unexplained pixels.
    pub cluster: usize,
    /// The largest difference in any channel between the same pixel of each.
    pub max: u8,
}

impl Difference {
    /// Whether this is only the anti-aliasing two rasterizers may differ by.
    pub fn within_tolerance(&self) -> bool {
        self.share <= SHARE && self.cluster <= CLUSTER
    }
}

/// How two RGBA buffers `width` pixels wide differ; `None` when their sizes
/// differ.
pub fn compare(a: &[u8], b: &[u8], width: usize) -> Option<Difference> {
    if a.len() != b.len() || width == 0 || !a.len().is_multiple_of(4 * width) {
        return None;
    }
    let count = a.len() / 4;
    let height = count / width;
    let neighbours = |i: usize| {
        let (x, y) = (i % width, i / width);
        let xs = x.saturating_sub(1)..=(x + 1).min(width - 1);
        (y.saturating_sub(1)..=(y + 1).min(height - 1))
            .flat_map(move |ny| xs.clone().map(move |nx| ny * width + nx))
    };
    // Whether pixel `i` of `p` lies outside what `q` has around it.
    let outside = |p: &[u8], q: &[u8], i: usize| {
        (0..4).any(|c| {
            let v = p[4 * i + c];
            let (lo, hi) = neighbours(i).fold((u8::MAX, 0), |(lo, hi), n| {
                (lo.min(q[4 * n + c]), hi.max(q[4 * n + c]))
            });
            v.saturating_add(EDGE) < lo || v > hi.saturating_add(EDGE)
        })
    };
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    let unexplained: Vec<bool> = (0..count)
        .map(|i| outside(a, b, i) || outside(b, a, i))
        .collect();
    let mut seen = vec![false; count];
    let (mut total, mut cluster) = (0, 0);
    let mut stack = Vec::new();
    for start in 0..count {
        if !unexplained[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            for n in neighbours(i) {
                if unexplained[n] && !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        total += size;
        cluster = cluster.max(size);
    }
    Some(Difference {
        share: total as f64 / count.max(1) as f64,
        cluster,
        max,
    })
}

/// Whether two RGBA buffers `width` pixels wide are the same drawing within
/// the tolerance; otherwise how they differ, or `None` for different sizes.
pub fn matches(a: &[u8], b: &[u8], width: usize) -> Result<(), Option<Difference>> {
    match compare(a, b, width) {
        Some(d) if d.within_tolerance() => Ok(()),
        other => Err(other),
    }
}

/// A PNG's or JPEG's pixels as straight RGBA, with its width and height —
/// for comparing drawings.
pub fn pixels(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let image = resolve::decode(data).ok()?;
    Some((image.data.data().to_vec(), image.width, image.height))
}

#[cfg(test)]
mod tests;
