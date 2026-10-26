//! The canvas on the GPU: the engine's display list replayed into a `vello`
//! scene and drawn with WebGPU. One `Gpu` — one device — serves every canvas
//! on the page; each canvas is a `Target`, and it keeps a `vello` renderer
//! only while the drawings it shows paint the very same images, under the
//! same blobs, in the same order: its image atlas then holds those images
//! and nothing else, placed as `devpage_design::atlas` places them — which
//! is how the engine knows they fit. Any other drawing — another design, a
//! file swapped, a shadow changed — gets a fresh renderer.

use vello::kurbo::Affine;
use vello::wgpu;
use wasm_bindgen::prelude::*;

use crate::Design;
use crate::scene::scene;

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
    blitter: wgpu::util::TextureBlitter,
    texture: Option<(u32, u32, wgpu::Texture)>,
    /// The renderer for the images the canvas's drawing paints, by their
    /// blobs, in the order it first paints them: what its atlas holds.
    vello: Option<(Vec<u64>, vello::Renderer)>,
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
        let blitter = wgpu::util::TextureBlitter::new(&self.device, format);
        Ok(Target {
            surface,
            format,
            blitter,
            texture: None,
            vello: None,
        })
    }

    /// Draws `design` into `target`, `width` × `height` device pixels, the
    /// design scaled by `zoom` and its top-left at `(x, y)`, over `background`
    /// (`0xRRGGBB`) — the color the page shows behind the canvas, so the
    /// canvas is opaque and the design's transparent parts look as they do on
    /// the CPU. Returns false when there is nothing to draw yet or the frame
    /// was lost.
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
    ) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        let Some(laid_out) = design.laid_out() else {
            return false;
        };
        let area = laid_out.area;
        let scene = {
            // The page places the drawing's top-left at `(x, y)`.
            let t = Affine::translate((x as f64, y as f64))
                * Affine::scale(zoom as f64)
                * Affine::translate((-area.x as f64, -area.y as f64));
            scene(&laid_out.list, t, area)
        };
        let blobs = || laid_out.images.iter().map(|i| i.data.id());
        if !matches!(&target.vello, Some((held, _)) if held.iter().copied().eq(blobs())) {
            // A fresh atlas for other images.
            match vello::Renderer::new(&self.device, vello::RendererOptions::default()) {
                Ok(renderer) => target.vello = Some((blobs().collect(), renderer)),
                Err(_) => return false,
            }
        }
        let Some((_, renderer)) = target.vello.as_mut() else {
            return false;
        };
        target.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: target.format,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                color_space: Default::default(),
            },
        );
        // The texture vello renders into is kept while the canvas keeps its size.
        if !matches!(&target.texture, Some((w, h, _)) if (*w, *h) == (width, height)) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            target.texture = Some((width, height, texture));
        }
        let Some((_, _, texture)) = &target.texture else {
            return false;
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let params = vello::RenderParams {
            base_color: vello::peniko::Color::from_rgb8(
                (background >> 16) as u8,
                (background >> 8) as u8,
                background as u8,
            ),
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
        };
        if renderer
            .render_to_texture(&self.device, &self.queue, &scene, &view, &params)
            .is_err()
        {
            return false;
        }
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = target.surface.get_current_texture()
        else {
            return false;
        };
        let out = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        target.blitter.copy(&self.device, &mut encoder, &view, &out);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        true
    }
}
