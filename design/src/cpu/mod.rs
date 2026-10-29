//! The `vello_cpu` backend — an adapter of [`crate::display::Target`]: any
//! area of a display list painted into a pixmap, and a tile's straight-alpha
//! pixels, for the server's tiles, the PNG and a browser without WebGPU.

use std::collections::HashMap;

use kurbo::{Affine, BezPath, Rect as KRect, Shape};
use peniko::{BlendMode, Compose, ImageBrush, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::display::List;
use crate::display::{Draw, Paint};
#[cfg(test)]
use crate::scene::{Scene, Theme};
use crate::tiles::{TILE, tile_rect};

/// The `vello_cpu` backend of [`crate::display::paint`].
pub struct Cpu<'a> {
    pub ctx: &'a mut RenderContext,
    pub sources: HashMap<u64, vello_cpu::ImageSource>,
    pub resources: Resources,
}

impl crate::display::Target for Cpu<'_> {
    fn draw(&mut self, draw: &Draw, t: Affine) {
        let ctx = &mut *self.ctx;
        ctx.set_transform(t);
        match draw {
            Draw::Fill(path, paint) => {
                match paint {
                    Paint::Solid(c) => ctx.set_paint(*c),
                    Paint::Gradient(g) => ctx.set_paint(g.clone()),
                    Paint::Image(data, place) => {
                        let image = self
                            .sources
                            .entry(data.data.id())
                            .or_insert_with(|| vello_cpu::ImageSource::from_peniko_image_data(data))
                            .clone();
                        ctx.set_paint(ImageBrush {
                            image,
                            sampler: Default::default(),
                        });
                        ctx.set_paint_transform(*place);
                    }
                }
                ctx.fill_path(path);
                ctx.reset_paint_transform();
            }
            Draw::Stroke(path, width, c) => {
                ctx.set_paint(*c);
                ctx.set_stroke(kurbo::Stroke::new(*width as f64));
                ctx.stroke_path(path);
            }
            Draw::Glyphs(font, size, c, coords, glyphs) => {
                ctx.set_paint(*c);
                let _ = ctx
                    .glyph_run(&mut self.resources, font)
                    .font_size(*size)
                    .normalized_coords(coords)
                    .hint(true)
                    .fill_glyphs(glyphs.iter().map(|g| vello_cpu::Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    }));
            }
        }
    }
    fn push_clip(&mut self, path: &BezPath, t: Affine) {
        self.ctx.set_transform(t);
        self.ctx.push_clip_layer(path);
    }
    fn push_layer(
        &mut self,
        blend: crate::display::Blend,
        bounds: Option<&BezPath>,
        t: Affine,
        area: KRect,
    ) {
        let whole;
        let bounds = match bounds {
            Some(b) => {
                self.ctx.set_transform(t);
                b
            }
            None => {
                self.ctx.set_transform(Affine::IDENTITY);
                whole = area.to_path(0.1);
                &whole
            }
        };
        match blend {
            crate::display::Blend::Opacity(o) => {
                self.ctx.push_layer(Some(bounds), None, Some(o), None, None)
            }
            crate::display::Blend::Add => self.ctx.push_layer(
                Some(bounds),
                Some(BlendMode::new(Mix::Normal, Compose::Plus)),
                None,
                None,
                None,
            ),
        }
    }
    fn pop(&mut self) {
        self.ctx.pop_layer();
    }
}

/// Paints `area` of `list` — in the design's coordinates, whole pixels at
/// `scale` — with `vello_cpu`: one tile, band or whole drawing.
pub fn paint_area(list: &crate::display::List, area: KRect, scale: f64) -> Pixmap {
    // Whole pixels, the last partly covered one included — but not one a
    // rounding error leaves past a tile's exact edge.
    let side = |v: f64| ((v * scale - 1e-6).ceil().clamp(1.0, u16::MAX as f64)) as u16;
    let (w, h) = (side(area.width()), side(area.height()));
    let mut ctx = RenderContext::new(w, h);
    let mut cpu = Cpu {
        ctx: &mut ctx,
        sources: HashMap::new(),
        resources: Resources::default(),
    };
    crate::display::paint(list, area, scale, &mut cpu);
    let mut resources = std::mem::take(&mut cpu.resources);
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render(&mut pixmap, &mut resources);
    pixmap
}

#[cfg(test)]
impl crate::render::Renderer {
    /// The scene's whole drawing painted with `vello_cpu` at `scale`.
    pub fn paint(&mut self, scene: &Scene, theme: Theme, scale: f32) -> Pixmap {
        let d = self.drawing(scene, theme, None).expect("drawable");
        let a = d.area();
        let area = KRect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        paint_area(&d.list, area, scale as f64)
    }
}

/// A tile's pixels, straight RGBA, `TILE` a side.
pub fn tile(list: &List, scale: f64, col: u32, row: u32) -> Vec<u8> {
    let pixmap = paint_area(list, tile_rect(list, scale, col, row), scale);
    let mut rgba = pixmap.data_as_u8_slice().to_vec();
    unpremultiply(&mut rgba);
    rgba
}

/// A strip of the drawing at 1×, straight RGBA: `TILE` pixels wide at tile
/// column `col`, `rows` tall from pixel row `y` — a piece of a PNG band too
/// wide for whole tiles.
pub fn strip(list: &List, col: u32, y: u32, rows: u32) -> Vec<u8> {
    let (x0, y0) = (list.area.x as f64, list.area.y as f64);
    let area = KRect::new(
        x0 + (col * TILE) as f64,
        y0 + y as f64,
        x0 + ((col + 1) * TILE) as f64,
        y0 + (y + rows) as f64,
    );
    let mut rgba = paint_area(list, area, 1.0).data_as_u8_slice().to_vec();
    unpremultiply(&mut rgba);
    rgba
}

/// Straight alpha from premultiplied, rounding as the PNG encoder does.
pub fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}
