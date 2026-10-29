//! The compositor on the GPU, on WebGPU: it carries out the design's frame
//! plan. One `Gpu` — one device — serves every canvas on the page; each
//! canvas is a `Target` with the textures of its tiles, by the ticket each
//! was painted under. A frame places every tile the plan draws ([`pass`]),
//! then paints the tiles the plan asks for at once ([`paint`]) — so none is
//! ever in flight past the frame — and frees the ones it evicts. A canvas
//! keeps its `vello` renderer only while its drawings paint the very same
//! images, under the same blobs, in the same order: its atlas then holds
//! those images and nothing else, placed as `devpage_design::atlas` places
//! them — which is how the engine knows they fit. Any other drawing gets a
//! fresh renderer. A tile that paints none of the images still names one,
//! or vello would let its atlas go ([`paint`]).

use std::collections::HashMap;

use vello::wgpu;
use wasm_bindgen::prelude::*;

use crate::Design;
use devpage_design::tiles::{Key, Ticket};

mod paint;
mod pass;

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
    /// The tiles the plan holds, by the ticket each was painted under.
    tiles: HashMap<Ticket, Tile>,
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
            tiles: HashMap::new(),
            design: None,
            painted: 0,
            vello: None,
        })
    }

    /// Carries out a frame of the design's plan in `target`, `width` ×
    /// `height` device pixels, the design scaled by `zoom` and its top-left
    /// at `(x, y)`, over `background` (`0xRRGGBB`): the held tiles of the
    /// view, moved and scaled. After the frame, when `settled`, the tiles
    /// the plan asks for are painted at once, for the next frame to show.
    /// Returns how many tiles of the view at `zoom` the frame lacked — 0
    /// when it showed the view current — or -1 when there is nothing to
    /// draw yet or the frame was lost.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        target: &mut Target,
        design: &mut Design,
        width: u32,
        height: u32,
        zoom: f64,
        x: f64,
        y: f64,
        background: u32,
        settled: bool,
    ) -> i32 {
        let (width, height) = (width.max(1), height.max(1));
        // Another design: none of the tiles are its.
        if target.design != Some(design.id()) {
            target.design = Some(design.id());
            target.tiles.clear();
        }
        let Some(work) = design.plan(width, height, zoom, (x, y), settled) else {
            return -1;
        };
        for ticket in &work.evict {
            target.tiles.remove(ticket);
        }
        // The frame first: what the plan draws, bottom first — stale tiles
        // and other scales standing in for what is still to paint.
        let quads: Vec<(vello::kurbo::Rect, &Tile)> = work
            .draw
            .iter()
            .filter_map(|(ticket, rect)| Some((*rect, target.tiles.get(ticket)?)))
            .collect();
        let shown = pass::composite(
            &self.device,
            &self.queue,
            &target.surface,
            target.format,
            &target.pipeline,
            (width, height),
            background,
            &quads,
        );
        // Then, after it, the tiles the plan asks for: swapped in by the
        // next frame. A tile that can't be painted is asked for again.
        let painted = if shown {
            self.paint_all(target, design, &work.request)
        } else {
            0
        };
        for &(ticket, _) in &work.request[painted..] {
            design.missed(ticket);
        }
        match (shown, painted == work.request.len()) {
            (true, true) => work.missing as i32,
            _ => -1,
        }
    }
}

impl Gpu {
    /// Paints the tiles `asked`, in turn, each into a texture held under
    /// its ticket while the plan keeps it, up to the first it can't paint:
    /// how many it painted.
    fn paint_all(
        &self,
        target: &mut Target,
        design: &mut Design,
        asked: &[(Ticket, Key)],
    ) -> usize {
        let theme = design.theme();
        let Some((held, renderer)) = design.held() else {
            return 0;
        };
        let Some(images) = held.images(renderer, theme).map(<[_]>::to_vec) else {
            return 0;
        };
        let blobs: Vec<u64> = images.iter().map(|i| i.data.id()).collect();
        if !matches!(&target.vello, Some((had, _)) if *had == blobs) {
            // A fresh atlas for other images. Tiles already painted are
            // textures of their own, and keep.
            match self.renderer(&images) {
                Some(renderer) => target.vello = Some((blobs, renderer)),
                None => return 0,
            }
        }
        for (n, &(ticket, key)) in asked.iter().enumerate() {
            let (Some((_, vello)), Some((held, renderer))) = (target.vello.as_mut(), design.held())
            else {
                return n;
            };
            // What the tile shows, rounded where its runs now place it.
            let Some(list) = held.tile(renderer, theme, key.level.scale(), key.col, key.row) else {
                return n;
            };
            let Some(tile) = self.paint(
                vello,
                list,
                &key,
                images.first(),
                (&target.layout, &target.sampler),
            ) else {
                return n;
            };
            target.painted += 1;
            if design.arrived(ticket) {
                target.tiles.insert(ticket, tile);
            }
        }
        asked.len()
    }
}
