//! Shadows: a box's shape blurred by a true Gaussian, as display-list
//! commands both backends replay.
//!
//! Neither vello's blurred rounded rectangle — one radius for all four corners
//! — nor any filter both backends share draws that, so the engine computes the
//! blur itself, sampling the shape every half standard deviation, which is fine
//! enough that the samples, drawn smoothly, are the blur at any zoom.
//!
//! A small shadow is one image of those samples. A large one would need too
//! many — a 16,000-pixel box blurred by 2 has a billion — but away from its
//! corners a blurred rounded rectangle doesn't vary along its edges, so it is
//! drawn as pieces instead: solid where the blur can't reach, each edge's
//! one-dimensional profile stretched along it, and each corner on its own —
//! sampled in two dimensions, or for a corner far rounder than the blur, the
//! exact blur of a disc's edge as a radial gradient. The pieces meet edge to
//! edge, each added to what is under it, so no seam shows where two meet at any
//! zoom; together, in one layer at the shadow color's opacity, they are the
//! shadow. An ellipse that isn't a circle has no straight edges, so its shadow
//! is tiled: solid tiles inside, sampled tiles along its edge, nothing outside.

use std::collections::BTreeMap;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Ellipse as KEllipse, Point, Rect, RoundedRect, Shape};
use peniko::{Blob, Color as PColor, Gradient, ImageAlphaType, ImageData, ImageFormat};

use crate::display::{Blend, Builder};
use crate::display::{Draw, Paint};
use crate::scene::Rgba;

/// How far a blur reaches, in samples: three standard deviations at two
/// samples a standard deviation. Past it the Gaussian is under 0.15%, less
/// than half of one step of 255.
const REACH: usize = 6;
/// The most samples a shadow drawn as one image may have, and its longest side.
pub const SINGLE: usize = 1 << 16;
const SINGLE_SIDE: usize = 1024;
/// A corner at least this many standard deviations round is drawn as a disc's
/// blurred edge, within 0.1 of 255 of the exact blur; a tighter one is sampled.
const ROUND: f64 = 16.0;
/// The side, in samples, of a tile of an ellipse's shadow.
const TILE: usize = 128;
/// How far the layer a piece is added from reaches past the piece, in scene
/// pixels: so far that no pixel the layer's own edge crosses holds any of the
/// piece, at any zoom out to 1/128.
const MARGIN: f64 = 128.0;

/// The shape a shadow blurs: a rounded rectangle — each corner's radius as
/// kurbo clamps it — or an ellipse.
#[derive(Clone, Copy)]
pub enum Outline {
    Rounded(Rect, [f64; 4]),
    Ellipse(Rect),
}

impl Outline {
    /// A rounded rectangle with radii `[top-left, top-right, bottom-right,
    /// bottom-left]`, or a circle as one with every radius half its side.
    pub fn rounded(rect: Rect, radii: [f64; 4]) -> Outline {
        let r = RoundedRect::from_rect(rect, (radii[0], radii[1], radii[2], radii[3])).radii();
        Outline::Rounded(
            rect,
            [r.top_left, r.top_right, r.bottom_right, r.bottom_left],
        )
    }

    pub fn ellipse(rect: Rect) -> Outline {
        if rect.width() == rect.height() {
            Outline::rounded(rect, [rect.width() / 2.0; 4])
        } else {
            Outline::Ellipse(rect)
        }
    }

    pub fn path(&self) -> BezPath {
        match *self {
            Outline::Rounded(r, [tl, tr, br, bl]) => {
                RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(0.1)
            }
            Outline::Ellipse(r) => KEllipse::from_rect(r).to_path(0.1),
        }
    }

    /// The outline as straight lines within `tolerance` of it. The
    /// rasterizer would flatten a curve itself, within a quarter of a sample
    /// — inside it, as a chord is — which moves a blurred curve's edge by as
    /// much as 2% of its alpha.
    fn lines(&self, tolerance: f64) -> BezPath {
        let mut out = BezPath::new();
        let shape = match *self {
            Outline::Rounded(r, [tl, tr, br, bl]) => {
                RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(tolerance / 10.0)
            }
            Outline::Ellipse(r) => KEllipse::from_rect(r).to_path(tolerance / 10.0),
        };
        kurbo::flatten(shape, tolerance, |el| out.push(el));
        out
    }

    fn rect(&self) -> Rect {
        match *self {
            Outline::Rounded(r, _) | Outline::Ellipse(r) => r,
        }
    }

    /// The signed distance from `p` to the edge, negative inside.
    fn distance(&self, p: Point) -> f64 {
        let r = self.rect();
        let c = r.center();
        let (hx, hy) = (r.width() / 2.0, r.height() / 2.0);
        let (dx, dy) = (p.x - c.x, p.y - c.y);
        match *self {
            Outline::Rounded(_, [tl, tr, br, bl]) => {
                let radius = match (dx >= 0.0, dy >= 0.0) {
                    (false, false) => tl,
                    (true, false) => tr,
                    (true, true) => br,
                    (false, true) => bl,
                };
                let (qx, qy) = (dx.abs() - hx + radius, dy.abs() - hy + radius);
                qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - radius
            }
            Outline::Ellipse(_) => ellipse_distance(hx, hy, dx, dy),
        }
    }

    /// Whether all of `r` is inside — for a convex shape, its four corners.
    fn holds(&self, r: Rect) -> bool {
        [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)]
            .iter()
            .all(|&(x, y)| self.distance(Point::new(x, y)) <= 0.0)
    }

    /// Whether `r` certainly lies outside.
    fn misses(&self, r: Rect) -> bool {
        if let Outline::Ellipse(e) = *self {
            // Scaled to a unit circle the rectangle is still one, and its
            // point nearest the center is the center clamped into it.
            let c = e.center();
            let (a, b) = (e.width() / 2.0, e.height() / 2.0);
            let nx = (c.x.clamp(r.x0, r.x1) - c.x) / a;
            let ny = (c.y.clamp(r.y0, r.y1) - c.y) / b;
            return nx.hypot(ny) > 1.0;
        }
        self.distance(r.center()) > r.width().hypot(r.height()) / 2.0
    }
}

/// The normalized Gaussian over `-REACH..=REACH` samples, two samples a
/// standard deviation.
fn kernel() -> [f32; 2 * REACH + 1] {
    let mut k = [0f32; 2 * REACH + 1];
    for (i, w) in k.iter_mut().enumerate() {
        let x = i as f64 - REACH as f64;
        *w = (-x * x / 8.0).exp() as f32;
    }
    let total: f32 = k.iter().sum();
    k.map(|w| w / total)
}

/// The grid a shadow is sampled on: sample `(i, j)` covers the square of side
/// `step` whose top-left is `origin + (i, j) × step`.
#[derive(Clone, Copy)]
struct Grid {
    origin: Point,
    step: f64,
}

impl Grid {
    fn x(&self, i: isize) -> f64 {
        self.origin.x + i as f64 * self.step
    }
    fn y(&self, j: isize) -> f64 {
        self.origin.y + j as f64 * self.step
    }
    fn rect(&self, i0: isize, j0: isize, i1: isize, j1: isize) -> Rect {
        Rect::new(self.x(i0), self.y(j0), self.x(i1), self.y(j1))
    }
    /// The transform from an image whose pixel `(0, 0)` is sample `(i, j)`.
    fn place(&self, i: isize, j: isize) -> Affine {
        Affine::translate((self.x(i), self.y(j))) * Affine::scale(self.step)
    }
}

/// How much of each of the `w` × `h` unit squares the closed polygon `path`
/// — straight lines only — covers under `to`, row by row, as an 8-bit
/// rasterizer gives it: the exact area, to the nearest 255th. Signed area is
/// accumulated cell by cell along each edge and summed along each row, so a
/// sample costs the same however the shape crosses it. What lies outside the
/// squares counts as a rasterizer clips it: left of them, wholly in the first
/// column; right of them, not at all.
fn coverage(path: &BezPath, to: Affine, w: usize, h: usize) -> Vec<f32> {
    // Each row has two cells past its end, which take what lies right of it.
    let stride = w + 2;
    let mut acc = vec![0f64; stride * h + 2];
    let mut cell = |p0: Point, p1: Point| {
        if (p0.y - p1.y).abs() <= 1e-12 {
            return;
        }
        let (dir, p0, p1) = if p0.y < p1.y {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let first = p0.y.max(0.0) as usize;
        for y in first..h.min(p1.y.ceil() as usize) {
            let start = (y * stride) as isize;
            let dy = ((y + 1) as f64).min(p1.y) - (y as f64).max(p0.y);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0floor = x0.floor();
            let x0i = x0floor as isize;
            let x1ceil = x1.ceil();
            let x1i = x1ceil as isize;
            let at = start + x0i;
            if at < 0 {
                x = xnext;
                continue;
            }
            let at = at as usize;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + xnext) - x0floor;
                acc[at] += d - d * xmf;
                acc[at + 1] += d * xmf;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                acc[at] += d * a0;
                if x1i == x0i + 2 {
                    acc[at + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    acc[at + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        acc[(start + xi) as usize] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f64 * s;
                    acc[(start + x1i - 1) as usize] += d * (1.0 - a2 - am);
                }
                acc[(start + x1i) as usize] += d * am;
            }
            x = xnext;
        }
    };
    // An edge split where it crosses either side, each part moved inside.
    let mut line = |p0: Point, p1: Point| {
        let mut cuts = vec![0.0, 1.0];
        for edge in [0.0, w as f64] {
            if (p0.x - edge) * (p1.x - edge) < 0.0 {
                cuts.push((edge - p0.x) / (p1.x - p0.x));
            }
        }
        cuts.sort_by(f64::total_cmp);
        let at = |t: f64| {
            let p = p0.lerp(p1, t);
            Point::new(p.x.clamp(0.0, w as f64), p.y)
        };
        for pair in cuts.windows(2) {
            cell(at(pair[0]), at(pair[1]));
        }
    };
    let (mut first, mut last) = (None, None);
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => {
                if let (Some(a), Some(b)) = (last, first) {
                    line(a, b);
                }
                let p = to * p;
                first = Some(p);
                last = Some(p);
            }
            kurbo::PathEl::LineTo(p) => {
                let p = to * p;
                if let Some(a) = last {
                    line(a, p);
                }
                last = Some(p);
            }
            kurbo::PathEl::ClosePath => {
                if let (Some(a), Some(b)) = (last, first) {
                    line(a, b);
                }
                last = first;
            }
            _ => unreachable!("an outline flattened to lines"),
        }
    }
    if let (Some(a), Some(b)) = (last, first) {
        line(a, b);
    }
    let mut out = Vec::with_capacity(w * h);
    for row in acc.chunks(stride).take(h) {
        let mut sum = 0.0;
        out.extend(row[..w].iter().map(|a| {
            sum += a;
            ((sum.abs().min(1.0) * 255.0).round() / 255.0) as f32
        }));
    }
    out
}

/// The blurred shape at samples `i0..i0 + w` × `j0..j0 + h`, row by row: its
/// coverage of each sample out to the blur's reach, blurred down each column
/// and then across each row.
fn blurred(path: &BezPath, grid: Grid, i0: isize, j0: isize, w: usize, h: usize) -> Vec<f32> {
    let (cw, ch) = (w + 2 * REACH, h + 2 * REACH);
    let to = Affine::scale(1.0 / grid.step)
        * Affine::translate((-grid.x(i0 - REACH as isize), -grid.y(j0 - REACH as isize)));
    let cov = coverage(path, to, cw, ch);
    let k = kernel();
    let mut down = vec![0f32; h * cw];
    for y in 0..h {
        let row = &mut down[y * cw..][..cw];
        for (t, weight) in k.iter().enumerate() {
            let from = &cov[(y + t) * cw..][..cw];
            for (o, c) in row.iter_mut().zip(from) {
                *o += weight * c;
            }
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let from = &down[y * cw..][..cw];
        for (x, o) in out[y * w..][..w].iter_mut().enumerate() {
            *o = k.iter().zip(&from[x..]).map(|(a, b)| a * b).sum();
        }
    }
    out
}

/// The blur of the interval `lo..hi` at samples `from..from + len` of a line
/// whose sample `0` starts at `origin`: a straight edge's profile.
fn profile(lo: f64, hi: f64, origin: f64, step: f64, from: isize, len: usize) -> Vec<f32> {
    let cov: Vec<f32> = (0..len + 2 * REACH)
        .map(|n| {
            let a = origin + (from + n as isize - REACH as isize) as f64 * step;
            ((hi.min(a + step) - lo.max(a)) / step).clamp(0.0, 1.0) as f32
        })
        .collect();
    let k = kernel();
    (0..len)
        .map(|i| k.iter().zip(&cov[i..]).map(|(a, b)| a * b).sum())
        .collect()
}

/// `values`, `w` × `h`, as an image of `c` whose alpha is each value times
/// `alpha`.
fn image(values: &[f32], w: usize, h: usize, c: Rgba, alpha: f32) -> ImageData {
    let pixels: Vec<u8> = values
        .iter()
        .flat_map(|v| [c.r, c.g, c.b, (v.clamp(0.0, 1.0) * alpha).round() as u8])
        .collect();
    ImageData {
        data: Blob::new(Arc::new(pixels)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w as u32,
        height: h as u32,
    }
}

/// The normal distribution's cumulative function, to 1.5 × 10⁻⁷
/// (Abramowitz and Stegun, 7.1.26).
fn phi_cdf(x: f64) -> f64 {
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-z * z).exp();
    0.5 * (1.0 + if x < 0.0 { -erf } else { erf })
}

/// One piece of a large shadow: the area it covers and how it is painted.
enum Piece {
    /// Every sample inside the blur's reach of the shape: solid.
    Solid(BezPath),
    /// Where the blur varies only down: the profile of rows `j0..j1`.
    Down(BezPath, isize, isize),
    /// Where it varies only across: the profile of columns `i0..i1`.
    Across(BezPath, isize, isize),
    /// A round corner: the blurred edge of the disc centered at the point,
    /// of the radius.
    Disc(BezPath, Point, f64),
    /// Samples `i0..i1` × `j0..j1`, blurred in two dimensions, cut to the
    /// path, or else to those samples.
    Patch(Option<BezPath>, isize, isize, isize, isize),
}

/// Adds `outline`'s shadow — blurred by a Gaussian of `std_dev` pixels and
/// colored `c` — to `list`, and each image it paints, in order, to `images`.
pub fn shadow(
    outline: Outline,
    std_dev: f64,
    c: Rgba,
    list: &mut Builder,
    images: &mut Vec<ImageData>,
) {
    shadow_with(outline, std_dev, c, list, images, SINGLE)
}

/// [`shadow`], drawing one image when it takes no more than `single` samples.
pub fn shadow_with(
    outline: Outline,
    std_dev: f64,
    c: Rgba,
    list: &mut Builder,
    images: &mut Vec<ImageData>,
    single: usize,
) {
    let rect = outline.rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let color = PColor::from_rgba8(c.r, c.g, c.b, c.a);
    if std_dev <= 0.0 {
        list.draw(Draw::Fill(outline.path(), Paint::Solid(color)));
        return;
    }
    let step = std_dev / 2.0;
    let reach = REACH as f64 * step;
    let grid = Grid {
        origin: Point::new(rect.x0 - reach, rect.y0 - reach),
        step,
    };
    let nx = ((rect.width() + 2.0 * reach) / step).ceil() as usize;
    let ny = ((rect.height() + 2.0 * reach) / step).ceil() as usize;
    // A hundredth of a sample.
    let path = outline.lines(step / 100.0);
    if nx * ny <= single && nx <= SINGLE_SIDE && ny <= SINGLE_SIDE {
        let values = blurred(&path, grid, 0, 0, nx, ny);
        let data = image(&values, nx, ny, c, c.a as f32);
        images.push(data.clone());
        list.draw(Draw::Fill(
            grid.rect(0, 0, nx as isize, ny as isize).to_path(0.1),
            Paint::Image(data, grid.place(0, 0)),
        ));
        return;
    }
    let (nx, ny) = (nx as isize, ny as isize);
    let pieces = match outline {
        Outline::Rounded(r, radii) => rounded(r, radii, std_dev, grid, nx, ny),
        Outline::Ellipse(e) => ellipse(e, std_dev, grid, nx, ny),
    };
    let extent = grid.rect(0, 0, nx, ny);
    list.push_layer(
        Blend::Opacity(c.a as f32 / 255.0),
        Some(extent.to_path(0.1)),
    );
    let opaque = Rgba { a: 255, ..c };
    let paint_over = |clip: &BezPath| clip.bounding_box().inflate(MARGIN, MARGIN).to_path(0.1);
    for piece in pieces {
        let (clip, paint) = match piece {
            Piece::Solid(clip) => (clip, Paint::Solid(color.with_alpha(1.0))),
            Piece::Down(clip, j0, j1) => {
                let len = (j1 - j0 + 2) as usize;
                let values = profile(rect.y0, rect.y1, grid.origin.y, step, j0 - 1, len);
                let data = image(&values, 1, len, opaque, 255.0);
                images.push(data.clone());
                let place = Affine::translate((0.0, grid.y(j0 - 1))) * Affine::scale(step);
                (clip, Paint::Image(data, place))
            }
            Piece::Across(clip, i0, i1) => {
                let len = (i1 - i0 + 2) as usize;
                let values = profile(rect.x0, rect.x1, grid.origin.x, step, i0 - 1, len);
                let data = image(&values, len, 1, opaque, 255.0);
                images.push(data.clone());
                let place = Affine::translate((grid.x(i0 - 1), 0.0)) * Affine::scale(step);
                (clip, Paint::Image(data, place))
            }
            Piece::Disc(clip, center, radius) => {
                (clip, Paint::Gradient(disc(center, radius, std_dev, c)))
            }
            Piece::Patch(clip, i0, j0, i1, j1) => {
                let (w, h) = ((i1 - i0 + 2) as usize, (j1 - j0 + 2) as usize);
                let values = blurred(&path, grid, i0 - 1, j0 - 1, w, h);
                let data = image(&values, w, h, opaque, 255.0);
                images.push(data.clone());
                (
                    clip.unwrap_or_else(|| grid.rect(i0, j0, i1, j1).to_path(0.1)),
                    Paint::Image(data, grid.place(i0 - 1, j0 - 1)),
                )
            }
        };
        // The piece's own edge cuts it, anti-aliased, so where two meet each
        // adds its share of the pixel; the layer reaches well past it.
        list.push_layer(Blend::Add, Some(paint_over(&clip)));
        list.draw(Draw::Fill(clip, paint));
        list.pop();
    }
    list.pop();
}

/// The blurred edge of a disc of `radius` around `center`, as a radial
/// gradient from where it is solid to where it is gone. A Gaussian of σ
/// across the edge of a disc of radius R is, to within 0.1 of 255 once R is
/// [`ROUND`] σ, the straight edge's minus φ(d)·σ/2R — the disc's curve taking
/// a little from it.
fn disc(center: Point, radius: f64, std_dev: f64, c: Rgba) -> Gradient {
    let reach = REACH as f64 * std_dev / 2.0;
    let (r0, r1) = (radius - reach, radius + reach);
    const STOPS: usize = 49;
    let stops: Vec<(f32, PColor)> = (0..STOPS)
        .map(|k| {
            let t = k as f64 / (STOPS - 1) as f64;
            let d = (r0 + t * (r1 - r0) - radius) / std_dev;
            let density = (-d * d / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
            let v = (phi_cdf(-d) - density * std_dev / (2.0 * radius)).clamp(0.0, 1.0);
            (
                t as f32,
                PColor::new([
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    v as f32,
                ]),
            )
        })
        .collect();
    Gradient::new_two_point_radial(center, r0 as f32, center, r1 as f32)
        .with_stops(stops.as_slice())
}

/// The pieces of a rounded rectangle's shadow over samples `0..nx` ×
/// `0..ny`. Away from a corner's reach the blur depends on one coordinate
/// only: down where the column lies a blur's reach inside both sides, across
/// where the row does, and is solid where both do.
fn rounded(
    rect: Rect,
    radii: [f64; 4],
    std_dev: f64,
    grid: Grid,
    nx: isize,
    ny: isize,
) -> Vec<Piece> {
    let reach = REACH as f64 * grid.step;
    let (sx, sy) = (
        |x: f64| (x - grid.origin.x) / grid.step,
        |y: f64| (y - grid.origin.y) / grid.step,
    );
    let clamp_x = |v: f64| (v as isize).clamp(0, nx);
    let clamp_y = |v: f64| (v as isize).clamp(0, ny);
    // Where the blur no longer reaches either side.
    let (xl, xr) = (2 * REACH as isize, clamp_x(sx(rect.x1 - reach).floor()));
    let (yt, yb) = (2 * REACH as isize, clamp_y(sy(rect.y1 - reach).floor()));
    let [tl, tr, br, bl] = radii;
    let (w, h) = (rect.width(), rect.height());
    // The straight run between each pair of corners.
    let (top, right, bottom, left) = (w - tl - tr, h - tr - br, w - br - bl, h - tl - bl);
    // A corner is a disc's edge when it is round enough and each side
    // leaving it runs straight for the blur's reach — or into a corner of
    // the same circle.
    let disc_ok = |r: f64, a: (f64, f64), b: (f64, f64)| {
        let run = |(len, other): (f64, f64)| len >= reach || (len <= 1e-9 && other == r);
        r >= ROUND * std_dev && run(a) && run(b)
    };
    struct Corner {
        zone: [isize; 4],
        disc: Option<Point>,
        radius: f64,
    }
    let mut corners = Vec::new();
    for (r, at_right, at_bottom, a, b) in [
        (tl, false, false, (top, tr), (left, bl)),
        (tr, true, false, (top, tl), (right, br)),
        (br, true, true, (bottom, bl), (right, tr)),
        (bl, false, true, (bottom, br), (left, tl)),
    ] {
        let disc = disc_ok(r, a, b);
        // A disc's quadrant reaches its center; a sampled corner, the blur's
        // reach past the corner's square.
        let inner = if disc { r } else { r + reach };
        let (x0, x1) = if at_right {
            let x = sx(rect.x1 - inner);
            (clamp_x(if disc { x.round() } else { x.floor() }), nx)
        } else {
            let x = sx(rect.x0 + inner);
            (0, clamp_x(if disc { x.round() } else { x.ceil() }))
        };
        let (y0, y1) = if at_bottom {
            let y = sy(rect.y1 - inner);
            (clamp_y(if disc { y.round() } else { y.floor() }), ny)
        } else {
            let y = sy(rect.y0 + inner);
            (0, clamp_y(if disc { y.round() } else { y.ceil() }))
        };
        let center = Point::new(
            if at_right { rect.x1 - r } else { rect.x0 + r },
            if at_bottom { rect.y1 - r } else { rect.y0 + r },
        );
        corners.push(Corner {
            zone: [x0, y0, x1, y1],
            disc: disc.then_some(center),
            radius: r,
        });
    }
    let mut xs = vec![0, nx, xl.min(nx), xr];
    let mut ys = vec![0, ny, yt.min(ny), yb];
    for c in &corners {
        xs.extend([c.zone[0], c.zone[2]]);
        ys.extend([c.zone[1], c.zone[3]]);
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let mut solid = BezPath::new();
    let mut down: BTreeMap<(isize, isize), BezPath> = BTreeMap::new();
    let mut across: BTreeMap<(isize, isize), BezPath> = BTreeMap::new();
    let mut discs: BTreeMap<usize, BezPath> = BTreeMap::new();
    let mut patches = Vec::new();
    // A sampled corner whose zone meets no other's is one patch.
    let meets =
        |a: &[isize; 4], b: &[isize; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
    let whole: Vec<bool> = corners
        .iter()
        .enumerate()
        .map(|(n, c)| {
            c.disc.is_none()
                && corners
                    .iter()
                    .enumerate()
                    .all(|(m, o)| m == n || !meets(&c.zone, &o.zone))
        })
        .collect();
    for (c, _) in corners.iter().zip(&whole).filter(|(_, w)| **w) {
        let [i0, j0, i1, j1] = c.zone;
        if i0 < i1 && j0 < j1 {
            patches.push((i0, j0, i1, j1));
        }
    }
    let add = |path: &mut BezPath, r: Rect| path.extend(r.path_elements(0.1));
    for ys in ys.windows(2) {
        let (j0, j1) = (ys[0], ys[1]);
        for xs in xs.windows(2) {
            let (i0, i1) = (xs[0], xs[1]);
            let cell = grid.rect(i0, j0, i1, j1);
            let within = |z: &[isize; 4]| z[0] <= i0 && i1 <= z[2] && z[1] <= j0 && j1 <= z[3];
            if corners
                .iter()
                .zip(&whole)
                .any(|(c, w)| *w && within(&c.zone))
            {
                // Already a patch of its own.
            } else if corners.iter().any(|c| c.disc.is_none() && within(&c.zone)) {
                patches.push((i0, j0, i1, j1));
            } else if let Some(n) = corners
                .iter()
                .position(|c| c.disc.is_some() && within(&c.zone))
            {
                add(discs.entry(n).or_default(), cell);
            } else {
                let central_x = xl <= i0 && i1 <= xr;
                let central_y = yt <= j0 && j1 <= yb;
                match (central_x, central_y) {
                    (true, true) => add(&mut solid, cell),
                    (true, false) => add(down.entry((j0, j1)).or_default(), cell),
                    (false, true) => add(across.entry((i0, i1)).or_default(), cell),
                    // Never, as a corner's zone covers where neither holds;
                    // if it did, sampling it is still exact.
                    (false, false) => patches.push((i0, j0, i1, j1)),
                }
            }
        }
    }
    let outline = Outline::Rounded(rect, radii);
    let mut pieces = Vec::new();
    if !solid.elements().is_empty() {
        pieces.push(Piece::Solid(solid));
    }
    // Bands that meet are one piece, with one image: every join between two
    // pieces inside a pixel costs a rounding in each, so a straight run has
    // none.
    pieces.extend(runs(down).map(|((j0, j1), p)| Piece::Down(p, j0, j1)));
    pieces.extend(runs(across).map(|((i0, i1), p)| Piece::Across(p, i0, i1)));
    pieces.extend(
        discs
            .into_iter()
            .map(|(n, p)| Piece::Disc(p, corners[n].disc.expect("a disc"), corners[n].radius)),
    );
    for (i0, j0, i1, j1) in merged(patches) {
        pieces.extend(tiles(&outline, grid, i0, j0, i1, j1));
    }
    pieces
}

/// Bands `(from, to)` of cells, merged where one ends as the next begins.
fn runs(
    bands: BTreeMap<(isize, isize), BezPath>,
) -> impl Iterator<Item = ((isize, isize), BezPath)> {
    let mut out: Vec<((isize, isize), BezPath)> = Vec::new();
    for ((from, to), path) in bands {
        match out.last_mut() {
            Some(((_, end), run)) if *end == from => {
                *end = to;
                run.extend(path);
            }
            _ => out.push(((from, to), path)),
        }
    }
    out.into_iter()
}

/// Cells `(i0, j0, i1, j1)` of a grid, none overlapping, merged into fewer
/// rectangles: along each band first, then down where the runs line up.
fn merged(mut cells: Vec<(isize, isize, isize, isize)>) -> Vec<(isize, isize, isize, isize)> {
    cells.sort_unstable_by_key(|&(i0, j0, _, j1)| (j0, j1, i0));
    let mut rows: Vec<(isize, isize, isize, isize)> = Vec::new();
    for c in cells {
        match rows.last_mut() {
            Some(r) if (r.1, r.3, r.2) == (c.1, c.3, c.0) => r.2 = c.2,
            _ => rows.push(c),
        }
    }
    rows.sort_unstable_by_key(|&(i0, j0, i1, _)| (i0, i1, j0));
    let mut out: Vec<(isize, isize, isize, isize)> = Vec::new();
    for c in rows {
        match out.last_mut() {
            Some(r) if (r.0, r.2, r.3) == (c.0, c.2, c.1) => r.3 = c.3,
            _ => out.push(c),
        }
    }
    out
}

/// Samples `i0..i1` × `j0..j1` of `outline`'s shadow as tiles of at most
/// [`TILE`] samples a side: none where the blur can't reach the shape, one
/// solid piece where the shape fills all it reaches, and each other tile
/// sampled.
fn tiles(outline: &Outline, grid: Grid, i0: isize, j0: isize, i1: isize, j1: isize) -> Vec<Piece> {
    let tile = TILE as isize;
    if (i1 - i0) <= tile && (j1 - j0) <= tile {
        return vec![Piece::Patch(None, i0, j0, i1, j1)];
    }
    // A tile's samples, its border and how far the blur reaches.
    let reach = REACH as isize + 1;
    let mut solid = BezPath::new();
    let mut pieces = Vec::new();
    let mut j = j0;
    while j < j1 {
        let je = (j + tile).min(j1);
        let mut run: Option<isize> = None;
        let mut i = i0;
        while i < i1 {
            let ie = (i + tile).min(i1);
            let reached = grid.rect(i - reach, j - reach, ie + reach, je + reach);
            let full = outline.holds(reached);
            if full {
                run.get_or_insert(i);
            } else {
                if let Some(start) = run.take() {
                    solid.extend(grid.rect(start, j, i, je).path_elements(0.1));
                }
                if !outline.misses(reached) {
                    pieces.push(Piece::Patch(None, i, j, ie, je));
                }
            }
            i = ie;
        }
        if let Some(start) = run {
            solid.extend(grid.rect(start, j, i1, je).path_elements(0.1));
        }
        j = je;
    }
    if !solid.elements().is_empty() {
        pieces.insert(0, Piece::Solid(solid));
    }
    pieces
}

/// The signed distance from `(x, y)` to the ellipse of semi-axes `a` and `b`
/// around the origin: from its nearest point, found by Newton's method on the
/// angle where the offset to it is normal to the curve.
fn ellipse_distance(a: f64, b: f64, x: f64, y: f64) -> f64 {
    // By symmetry, in the first quadrant, where the nearest point is too.
    let (px, py) = (x.abs(), y.abs());
    let mut t = (a * py).atan2(b * px);
    for _ in 0..40 {
        let (sin, cos) = t.sin_cos();
        let (ex, ey) = (a * cos, b * sin);
        // f(t) = (p − e(t)) · e'(t), and its derivative.
        let f = (px - ex) * -a * sin + (py - ey) * b * cos;
        let df =
            (a * a * sin * sin + b * b * cos * cos) - (px - ex) * a * cos - (py - ey) * b * sin;
        if df.abs() < 1e-12 {
            break;
        }
        let next = (t - f / df).clamp(0.0, std::f64::consts::FRAC_PI_2);
        if (next - t).abs() < 1e-12 {
            t = next;
            break;
        }
        t = next;
    }
    let (sin, cos) = t.sin_cos();
    let d = (px - a * cos).hypot(py - b * sin);
    if (px / a).hypot(py / b) <= 1.0 { -d } else { d }
}

/// The pieces of an ellipse's shadow. Cut by rays from its center into
/// wedges, each short enough that the ellipse stays within a hundredth of a
/// standard deviation of its circle of curvature there, a wedge is that
/// circle's blurred edge — or, where the ellipse curves too tightly for that,
/// sampled; inside the wedges it is solid. An ellipse too thin to be solid
/// anywhere is tiled instead.
fn ellipse(e: Rect, std_dev: f64, grid: Grid, nx: isize, ny: isize) -> Vec<Piece> {
    let outline = Outline::Ellipse(e);
    let step = grid.step;
    let (a, b) = (e.width() / 2.0, e.height() / 2.0);
    let c = e.center();
    // How far inside every sample a blur reaches is covered, and how far
    // outside none is.
    let inside = (REACH as f64 * step + step) * std::f64::consts::SQRT_2;
    let outside = inside + step;
    if a.min(b) <= 4.0 * inside {
        return tiles(&outline, grid, 0, 0, nx, ny);
    }
    let at = |t: f64| Point::new(c.x + a * t.cos(), c.y + b * t.sin());
    let normal = |t: f64| {
        let n = kurbo::Vec2::new(b * t.cos(), a * t.sin());
        n / n.hypot()
    };
    let rho = |t: f64| {
        let (sin, cos) = t.sin_cos();
        (a * a * sin * sin + b * b * cos * cos).powf(1.5) / (a * b)
    };
    let distance = |p: Point| ellipse_distance(a, b, p.x - c.x, p.y - c.y);
    // Where the ray through the point at `t` crosses `depth` inside and
    // `outside` outside, or `None` when no point on it lies that deep.
    let cut = |t: f64| -> Option<(Point, Point)> {
        let p = at(t);
        let ray = p - c;
        let u = ray / ray.hypot();
        let cos = u.dot(normal(t)).max(1e-6);
        let mut s = inside / cos;
        while distance(p - u * s) > -inside {
            s += step;
            if s >= ray.hypot() {
                return None;
            }
        }
        Some((p - u * s, p + u * (outside / cos)))
    };
    let tolerance = std_dev / 100.0;
    let tile = TILE as f64 * step;
    // Whether the wedge from `t0` to `t1` is short enough.
    let fine = |t0: f64, t1: f64| -> bool {
        let tm = (t0 + t1) / 2.0;
        let (p0, p1) = (at(t0), at(t1));
        let chord = (p1 - p0).hypot();
        let tightest = rho(t0).min(rho(t1)).min(rho(tm));
        // The outer edge, a chord, may come no nearer than a sample.
        if chord * chord / (8.0 * (tightest + outside)) > step {
            return false;
        }
        if tightest >= ROUND * std_dev {
            let center = at(tm) - normal(tm) * rho(tm);
            let r = rho(tm);
            [t0, (t0 + tm) / 2.0, (tm + t1) / 2.0, t1]
                .iter()
                .all(|&t| ((at(t) - center).hypot() - r).abs() <= tolerance)
        } else {
            chord <= tile
        }
    };
    // Cut at every axis, so the curvature only grows or shrinks in a wedge,
    // then halve each wedge until it is fine.
    let mut ts = Vec::new();
    let quarter = std::f64::consts::FRAC_PI_2;
    for q in 0..4 {
        let mut todo = vec![(q as f64 * quarter, (q + 1) as f64 * quarter, 0)];
        let mut done = Vec::new();
        while let Some((t0, t1, depth)) = todo.pop() {
            if depth >= 24 || fine(t0, t1) {
                done.push(t0);
            } else {
                let tm = (t0 + t1) / 2.0;
                todo.push((tm, t1, depth + 1));
                todo.push((t0, tm, depth + 1));
            }
        }
        done.sort_by(f64::total_cmp);
        ts.extend(done);
    }
    let mut cuts = Vec::with_capacity(ts.len());
    for &t in &ts {
        match cut(t) {
            Some(c) => cuts.push(c),
            None => return tiles(&outline, grid, 0, 0, nx, ny),
        }
    }
    let mut pieces = Vec::new();
    let mut solid = BezPath::new();
    for (n, (inner, _)) in cuts.iter().enumerate() {
        if n == 0 {
            solid.move_to(*inner);
        } else {
            solid.line_to(*inner);
        }
    }
    solid.close_path();
    pieces.push(Piece::Solid(solid));
    let tau = std::f64::consts::TAU;
    for n in 0..ts.len() {
        let (t0, t1) = (ts[n], if n + 1 < ts.len() { ts[n + 1] } else { tau });
        let ((i0, o0), (i1, o1)) = (cuts[n], cuts[(n + 1) % cuts.len()]);
        let mut wedge = BezPath::new();
        wedge.move_to(i0);
        wedge.line_to(i1);
        wedge.line_to(o1);
        wedge.line_to(o0);
        wedge.close_path();
        let tm = (t0 + t1) / 2.0;
        if rho(t0).min(rho(t1)).min(rho(tm)) >= ROUND * std_dev {
            pieces.push(Piece::Disc(wedge, at(tm) - normal(tm) * rho(tm), rho(tm)));
        } else {
            let bounds = wedge.bounding_box();
            let i = |x: f64| ((x - grid.origin.x) / step).floor() as isize;
            let j = |y: f64| ((y - grid.origin.y) / step).floor() as isize;
            pieces.push(Piece::Patch(
                Some(wedge),
                i(bounds.x0),
                j(bounds.y0),
                i(bounds.x1) + 1,
                j(bounds.y1) + 1,
            ));
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::compare;
    use crate::cpu::paint_area;
    use crate::display::Frame;
    use crate::display::List;

    /// The most a shadow in pieces may differ from the same shadow as one
    /// image, in any channel of 255, by how wide its blur is on the screen:
    /// σ at the zoom, in device pixels. Measured over 3,000 random outlines
    /// at 0.25×, 0.5×, 0.73×, 1×, 1.37× and 2× — what `random` makes, with
    /// any seed. Where σ is a pixel or more, only the last bit of each
    /// piece's 8-bit rounding is left; a narrower blur's edge is steeper than
    /// a pixel, and vello_cpu's 8-bit gradient and layers round a disc
    /// corner's a few hundredths of a pixel away from the sampled one.
    fn bound(std_dev: f64, scale: f32) -> u8 {
        match std_dev * scale as f64 {
            s if s >= 1.0 => 4,
            s if s >= 0.5 => 6,
            s if s >= 0.25 => 11,
            _ => 18,
        }
    }

    /// Paints `outline`'s shadow at each scale as one image and in pieces,
    /// and holds the two to the engine's tolerance — no seam — and to
    /// [`bound`].
    fn pieces_match_one_image(outline: Outline, std_dev: f64, case: &str, scales: &[f32]) {
        let color = Rgba {
            r: 20,
            g: 30,
            b: 40,
            a: 200,
        };
        let paint = |single: usize, scale: f32| {
            let area = Frame {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 220.0,
            };
            let list = List::of(area, |b| {
                shadow_with(outline, std_dev, color, b, &mut Vec::new(), single)
            });
            let p = paint_area(&list, Rect::new(0.0, 0.0, 360.0, 220.0), scale as f64);
            // How many commands, and how many layers they are drawn in.
            let chunk = list.chunk(&Vec::new()).unwrap();
            let shape = (chunk.commands.len(), chunk.made.effects.len());
            (p.data_as_u8_slice().to_vec(), p.width() as usize, shape)
        };
        for &scale in scales {
            let (one, width, single_list) = paint(usize::MAX, scale);
            let (pieces, _, pieced_list) = paint(0, scale);
            assert_eq!(single_list, (1, 0), "{case}");
            assert!(pieced_list.1 > 1, "{case} isn't in pieces");
            let d = compare(&one, &pieces, width).unwrap();
            assert!(
                d.within_tolerance() && d.max <= bound(std_dev, scale),
                "{case}, σ {std_dev}, at {scale}: {d:?}"
            );
        }
    }

    /// A shadow drawn in pieces is the shadow drawn as one image — no seam
    /// where pieces meet, no step at a corner — at 1× and at scales that put
    /// every edge between pieces part way across a pixel, for square
    /// corners, sampled corners, disc corners, a pill, a thin box, a thin
    /// ellipse, a circle, and ellipses in wedges with and without sampled
    /// tips; the box 1.48 pixels tall blurred by less than 2 that once
    /// differed by 7 at 1×; and random outlines.
    #[test]
    fn a_shadow_in_pieces_is_the_shadow_in_one() {
        let r = |x0, y0, x1, y1| Rect::new(x0, y0, x1, y1);
        let cases = [
            (Outline::rounded(r(30.3, 20.7, 250.1, 170.9), [0.0; 4]), 4.0),
            (
                Outline::rounded(r(30.3, 20.7, 250.1, 170.9), [3.0, 80.0, 12.0, 45.0]),
                4.0,
            ),
            (
                Outline::rounded(r(30.0, 40.0, 330.0, 120.0), [40.0; 4]),
                2.0,
            ),
            (Outline::rounded(r(30.0, 40.0, 330.0, 43.0), [1.0; 4]), 6.0),
            (Outline::ellipse(r(25.5, 30.25, 300.5, 140.75)), 3.0),
            (Outline::ellipse(r(25.0, 30.0, 185.0, 190.0)), 2.0),
            (Outline::ellipse(r(15.3, 30.6, 345.2, 190.1)), 2.0),
            (Outline::ellipse(r(20.0, 60.0, 340.0, 150.0)), 1.0),
            (
                Outline::rounded(
                    r(50.0986, 43.2465, 329.1681, 44.7226),
                    [0.738, 0.738, 0.738, 0.0],
                ),
                0.869,
            ),
            (
                Outline::rounded(r(63.287, 18.532, 289.892, 187.314), [80.0; 4]),
                0.584,
            ),
        ];
        for (n, (outline, std_dev)) in cases.into_iter().enumerate() {
            pieces_match_one_image(outline, std_dev, &format!("case {n}"), &[1.0, 0.73, 1.37]);
        }
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for n in 0..12 {
            let (outline, std_dev) = random(&mut seed);
            pieces_match_one_image(outline, std_dev, &format!("random {n}"), &[0.5, 1.0]);
        }
    }

    /// A random outline that fits the test's 360 × 220 area with its blur,
    /// and one image of its shadow's samples, and its σ, from `seed`: a box with square, sampled, nearly-disc,
    /// pill-round or random corners — all alike or each its own — or an
    /// ellipse, blurred by σ from half a pixel to six.
    fn random(seed: &mut u64) -> (Outline, f64) {
        let mut next = || {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (*seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let (aw, ah) = (360.0, 220.0);
        let sd = 0.5 + next() * 5.5;
        let reach = 3.0 * sd;
        // No wider than one image of samples can show, with its blur.
        let most = SINGLE_SIDE as f64 * sd / 2.0 - 2.0 * reach - sd - 1.0;
        let w = 1.0 + next() * (aw - 2.0 * reach - 20.0).min(most);
        let h = 1.0 + next() * (ah - 2.0 * reach - 20.0).min(most);
        let x0 = reach + 5.0 + next() * (aw - 2.0 * reach - 10.0 - w);
        let y0 = reach + 5.0 + next() * (ah - 2.0 * reach - 10.0 - h);
        let rect = Rect::new(x0, y0, x0 + w, y0 + h);
        let half = w.min(h) / 2.0;
        let kind = (next() * 6.0) as usize;
        if kind == 5 {
            return (Outline::ellipse(rect), sd);
        }
        let mut corners = [(0, 0.0); 4];
        for c in &mut corners {
            *c = ((next() * 5.0) as usize, next());
        }
        let alike = next() < 0.5;
        let radius = |(kind, u): (usize, f64)| match kind {
            0 => 0.0,
            1 => u * ROUND * sd,
            2 => ROUND * sd * (0.98 + 0.04 * u),
            3 => half,
            4 => half - u * 3.0 * sd,
            _ => u * half,
        };
        let radii = if alike {
            [radius(corners[0]); 4]
        } else {
            corners.map(radius)
        };
        (Outline::rounded(rect, radii), sd)
    }
}
