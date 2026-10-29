//! The painter: a node's own commands — shadow, fill, image, stroke and
//! text — in its own coordinates, each color its value in the theme.

use parley::PositionedLayoutItem;

use kurbo::{Affine, BezPath, Ellipse as KEllipse, Rect as KRect, RoundedRect, Shape, Vec2};
use peniko::{Color as PColor, Gradient};

use crate::display::Builder;
use crate::scene::{Color, Fill, Kind, Node, Rgba, Scene, Theme};
use crate::shadow::{self, Outline};

use super::*;
impl Renderer {
    /// A node's own commands, its box at `f` in its own coordinates — its
    /// shadow, fill, image, stroke and text — into `b`, and what each covers
    /// into `covers`. The images it paints go to `self.painted`, each image
    /// file under the blob `self.blobs` holds for it.
    pub(super) fn own(
        &mut self,
        scene: &Scene,
        node: &Node,
        f: Frame,
        theme: Theme,
        b: &mut Builder,
        covers: &mut Vec<KRect>,
    ) {
        let rect = KRect::new(
            f.x as f64,
            f.y as f64,
            (f.x + f.width) as f64,
            (f.y + f.height) as f64,
        );
        let shape = |inset: f64| node_shape(node, rect, inset);
        if let Some(s) = &node.shadow {
            // The box's own shape, widened by the spread — every corner's
            // radius by it too — and moved by the offset.
            let spread = s.spread as f64;
            let r = rect
                .inflate(spread, spread)
                .with_origin((rect.x0 - spread + s.x as f64, rect.y0 - spread + s.y as f64));
            let outline = if node.kind == Kind::Ellipse {
                Outline::ellipse(r)
            } else {
                Outline::rounded(r, node.radius.map(|v| (v as f64 + spread).max(0.0)))
            };
            let std_dev = (s.blur as f64 / 2.0).max(0.0);
            if r.width() > 0.0 && r.height() > 0.0 {
                // A blur reaches three standard deviations past its shape.
                covers.push(r.inflate(3.0 * std_dev, 3.0 * std_dev));
            }
            let mut images = Vec::new();
            shadow::shadow(outline, std_dev, s.color.get(theme), b, &mut images);
            self.painted.extend(
                images
                    .into_iter()
                    .map(|data| (Painted { data, file: None }, s.at.clone())),
            );
        }
        // A vector's fill and stroke follow its path; every other box's
        // follow the box.
        let vector = node.vector.as_ref().map(|v| {
            let [vx, vy, vw, vh] = v.view_box.map(|n| n as f64);
            let s = (f.width as f64 / vw).min(f.height as f64 / vh);
            let tx = f.x as f64 + (f.width as f64 - vw * s) / 2.0 - vx * s;
            let ty = f.y as f64 + (f.height as f64 - vh * s) / 2.0 - vy * s;
            let mut path = BezPath::from_svg(&v.d).unwrap_or_default();
            path.apply_affine(Affine::translate((tx, ty)) * Affine::scale(s));
            path
        });
        if let Some(fill) = &node.fill {
            let paint = match fill {
                Fill::Solid(c) => Paint::Solid(color(c.get(theme))),
                Fill::Linear { angle, stops } => {
                    Paint::Gradient(gradient(rect, *angle, stops, theme))
                }
            };
            let path = vector.clone().unwrap_or_else(|| shape(0.0));
            covers.push(path.bounding_box());
            b.draw(Draw::Fill(path, paint));
        }
        if let Some(image) = &node.image {
            let file = &scene.images[&image.id];
            let data = match self.blobs.get(&image.id) {
                Some(data) => data.clone(),
                None => {
                    let data = file.image_data();
                    self.blobs.insert(image.id, data.clone());
                    data
                }
            };
            self.painted.push((
                Painted {
                    data: data.clone(),
                    file: Some(image.id),
                },
                image.at.clone(),
            ));
            // Covering the box: the file at its own size scaled to fill it,
            // centered, cut to its shape — drawn from its pixels, which may
            // be fewer.
            let (iw, ih) = (file.width as f64, file.height as f64);
            let s = (f.width as f64 / iw).max(f.height as f64 / ih);
            let place = Affine::translate((
                f.x as f64 + (f.width as f64 - iw * s) / 2.0,
                f.y as f64 + (f.height as f64 - ih * s) / 2.0,
            )) * Affine::scale(s)
                * Affine::scale_non_uniform(
                    iw / file.pixels_width as f64,
                    ih / file.pixels_height as f64,
                );
            covers.push(rect);
            b.draw(Draw::Fill(shape(0.0), Paint::Image(data, place)));
        }
        if let Some(c) = node.stroke {
            let path = match &vector {
                Some(path) => path.clone(),
                None => shape(node.stroke_width as f64 / 2.0),
            };
            let half = node.stroke_width as f64 / 2.0;
            covers.push(path.bounding_box().inflate(half, half));
            b.draw(Draw::Stroke(path, node.stroke_width, color(c.get(theme))));
        }
        if let Some(text) = &node.text {
            let layout = self.text_layout(text, Some(f.width + 0.5));
            for line in layout.lines() {
                // The line's box, or as far as its fonts' ascenders and
                // descenders reach past it.
                let m = line.metrics();
                let y = |v: f32| f.y as f64 + v as f64;
                covers.push(KRect::new(
                    f.x as f64 + m.inline_min_coord as f64,
                    y(m.block_min_coord.min(m.content_block_min_coord)),
                    f.x as f64 + m.inline_max_coord as f64,
                    y(m.block_max_coord.max(m.content_block_max_coord)),
                ));
                for item in line.items() {
                    if let PositionedLayoutItem::GlyphRun(run) = item {
                        let glyphs = run
                            .positioned_glyphs()
                            .map(|g| Glyph {
                                id: g.id,
                                x: f.x + g.x,
                                y: f.y + g.y,
                            })
                            .collect();
                        b.draw(Draw::Glyphs(
                            run.run().font().clone(),
                            run.run().font_size(),
                            color(text.color.get(theme)),
                            run.run()
                                .normalized_coords()
                                .iter()
                                .map(|c| c.to_bits())
                                .collect(),
                            glyphs,
                        ));
                    }
                }
            }
        }
    }
}

/// A node's box shape at `rect`, grown by `-inset`.
pub(super) fn node_shape(node: &Node, rect: KRect, inset: f64) -> BezPath {
    let r = rect.inset(-inset);
    if node.kind == Kind::Ellipse {
        KEllipse::from_rect(r).to_path(0.1)
    } else {
        let [tl, tr, br, bl] = node.radius.map(|v| (v as f64 - inset).max(0.0));
        RoundedRect::from_rect(r, (tl, tr, br, bl)).to_path(0.1)
    }
}

pub(super) fn color(c: Rgba) -> PColor {
    PColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// A CSS linear gradient over `rect`: 0° points up, 90° right; the line runs
/// through the center, long enough that its ends touch the far corners.
/// Its stops take their colors in `theme`.
pub(super) fn gradient(rect: KRect, angle: f32, stops: &[(Color, f32)], theme: Theme) -> Gradient {
    let a = (angle as f64).to_radians();
    let dir = Vec2::new(a.sin(), -a.cos());
    let half = (rect.width() * a.sin().abs() + rect.height() * a.cos().abs()) / 2.0;
    let center = rect.center();
    let stops: Vec<(f32, PColor)> = stops
        .iter()
        .map(|(c, at)| (*at, color(c.get(theme))))
        .collect();
    Gradient::new_linear(center - dir * half, center + dir * half).with_stops(stops.as_slice())
}

#[cfg(test)]
mod tests;
