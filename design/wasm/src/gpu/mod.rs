//! The compositor on the GPU, on WebGPU. One `Gpu` — one device — serves
//! every canvas on the page; each canvas is a `Target` with its cache of
//! tile textures. A frame marks the tiles the design's damage overlaps
//! stale, paints a few of the view's missing or stale tiles once the zoom
//! has settled ([`paint`]), and places every cached tile the view shows
//! ([`pass`]). A canvas keeps its `vello` renderer only while its drawings
//! paint the very same images, under the same blobs, in the same order: its
//! atlas then holds those images and nothing else, placed as
//! `devpage_design::atlas` places them — which is how the engine knows they
//! fit. Any other drawing gets a fresh renderer.

use vello::wgpu;
use wasm_bindgen::prelude::*;

use crate::Design;
use devpage_design::tiles::{Cache, Key, Level, View};

mod paint;
mod pass;

/// The most tiles a canvas keeps: more than a 4K screen at a device pixel
/// ratio of 2 shows, about 160, so a view never evicts its own tiles.
pub const CAP: usize = 384;

/// A painted tile: its texture, and what binds it to the compositor.
pub(crate) struct Tile {
    _texture: wgpu::Texture,
    pub(crate) group: wgpu::BindGroup,
}

#[wasm_bindgen]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// One canvas the GPU draws into.
#[wasm_bindgen]
pub struct Target {
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The tiles painted so far, by theme, level, column and row, at most
    /// `CAP` of them.
    tiles: Cache<Tile>,
    /// The design the tiles were painted from.
    design: Option<u32>,
    /// How many tiles the canvas has painted, for the tests.
    painted: u32,
    /// The renderer for the images the canvas's drawing paints, by their
    /// blobs, in the order it first paints them: what its atlas holds.
    vello: Option<(Vec<u64>, vello::Renderer)>,
}

#[wasm_bindgen]
impl Target {
    /// How many tiles this canvas has painted.
    pub fn painted(&self) -> u32 {
        self.painted
    }
}

fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
impl Gpu {
    /// Opens WebGPU without touching any canvas; fails where the browser has
    /// none, leaving every canvas free for the CPU.
    pub async fn open() -> Result<Gpu, JsValue> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(error)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(error)?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Takes `canvas` for drawing on the GPU.
    pub fn target(&self, canvas: web_sys::HtmlCanvasElement) -> Result<Target, JsValue> {
        let surface = self
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(error)?;
        let format = *surface
            .get_capabilities(&self.adapter)
            .formats
            .first()
            .ok_or_else(|| error("no surface format"))?;
        let (pipeline, layout) = pass::compositor(&self.device, format);
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Target {
            surface,
            format,
            pipeline,
            layout,
            sampler,
            tiles: Cache::new(CAP),
            design: None,
            painted: 0,
            vello: None,
        })
    }

    /// Composites `design` into `target`, `width` × `height` device pixels,
    /// the design scaled by `zoom` and its top-left at `(x, y)`, over
    /// `background` (`0xRRGGBB`): the cached tiles of the view, moved and
    /// scaled. After the frame, when `settled`, up to `budget` missing or
    /// stale tiles of the view are painted at `zoom`, nearest the middle
    /// first, for the next frame to show. Returns how many tiles of the view
    /// at `zoom` the frame lacked — 0 when it showed the view current — or
    /// -1 when there is nothing to draw yet or the frame was lost.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        target: &mut Target,
        design: &mut Design,
        width: u32,
        height: u32,
        zoom: f32,
        x: f32,
        y: f32,
        background: u32,
        settled: bool,
        budget: u32,
    ) -> i32 {
        let (width, height) = (width.max(1), height.max(1));
        let theme = design.theme();
        // Another design: none of the tiles are its.
        if target.design != Some(design.id()) {
            target.design = Some(design.id());
            target.tiles.clear();
        }
        // What the updates since the last frame changed: the tiles over it
        // go stale, and are shown until they are painted again.
        for (theme, damage) in design.drain_damage() {
            match damage {
                crate::Damage::All => target.tiles.damage_all(theme),
                crate::Damage::Rects { origin, rects } => {
                    target.tiles.damage(theme, origin, &rects)
                }
            }
        }
        let Some(drawing) = design.laid_out() else {
            return -1;
        };
        let list = &drawing.list;
        let level = Level::of(zoom as f64);
        let a = list.area;
        let area = vello::kurbo::Rect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        let view = View {
            width: width as f64,
            height: height as f64,
            zoom: zoom as f64,
            origin: (x.round() as f64, y.round() as f64),
        };
        let key = |(col, row)| Key {
            theme,
            level,
            col,
            row,
        };
        let missing: Vec<Key> = drawing
            .view_tiles(&view)
            .into_iter()
            .map(key)
            .filter(|k| target.tiles.get(k).is_none_or(|(_, stale)| stale))
            .collect();
        // The frame first: every cached tile of this theme the view shows,
        // coarser levels under finer ones, the view's own on top — stale ones
        // and other scales standing in for what is still to paint.
        let mut quads: Vec<(vello::kurbo::Rect, &Tile, Level)> = target
            .tiles
            .iter()
            .filter(|(k, _, _)| k.theme == theme)
            .filter_map(|(k, tile, _)| Some((view.place(area, k)?, tile, k.level)))
            .collect();
        quads.sort_by_key(|q| (q.2 == level, q.2.0));
        let quads: Vec<(vello::kurbo::Rect, &Tile)> =
            quads.into_iter().map(|(r, t, _)| (r, t)).collect();
        if !pass::composite(
            &self.device,
            &self.queue,
            &target.surface,
            target.format,
            &target.pipeline,
            (width, height),
            background,
            &quads,
        ) {
            return -1;
        }
        // Then, after the frame, up to `budget` of the tiles it still needs,
        // nearest the middle first: swapped in by the next frame.
        if settled && !missing.is_empty() {
            let images = drawing.images.clone();
            let blobs: Vec<u64> = images.iter().map(|i| i.data.id()).collect();
            if !matches!(&target.vello, Some((held, _)) if *held == blobs) {
                // A fresh atlas for other images. Tiles already painted are
                // textures of their own, and keep.
                match self.renderer(&images) {
                    Some(renderer) => target.vello = Some((blobs, renderer)),
                    None => return -1,
                }
            }
            for k in missing.iter().take(budget as usize) {
                let Some((_, renderer)) = target.vello.as_mut() else {
                    return -1;
                };
                // What the tile shows, rounded where its runs now place it.
                let rect = devpage_design::tiles::tile_rect(
                    &design.laid_out().expect("drawn").list,
                    k.level.scale(),
                    k.col,
                    k.row,
                );
                let Some(drawing) = design.prepared(rect) else {
                    return -1;
                };
                let list = &drawing.list;
                let Some(tile) = self.paint(renderer, list, k, (&target.layout, &target.sampler))
                else {
                    return -1;
                };
                target.tiles.insert(*k, tile, 1);
                target.painted += 1;
            }
        }
        missing.len() as i32
    }
}
