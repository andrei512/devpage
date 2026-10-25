//! The canvas on the GPU: the engine's display list replayed into a `vello`
//! scene and drawn with WebGPU. One `Gpu` — one device — serves every canvas
//! on the page; each canvas is a `Target`.

use devpage_design::render::{Draw, Paint};
use vello::kurbo::Affine;
use vello::peniko::{Fill, ImageBrush};
use vello::wgpu;
use wasm_bindgen::prelude::*;

use crate::Design;

#[wasm_bindgen]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    vello: vello::Renderer,
}

/// One canvas the GPU draws into.
#[wasm_bindgen]
pub struct Target {
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    blitter: wgpu::util::TextureBlitter,
    texture: Option<(u32, u32, wgpu::Texture)>,
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
        let vello =
            vello::Renderer::new(&device, vello::RendererOptions::default()).map_err(error)?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
            vello,
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
        })
    }

    /// Draws `design` into `target`, `width` × `height` device pixels, the
    /// design scaled by `zoom` and its top-left at `(x, y)`. Returns false when
    /// there is nothing to draw yet or the frame was lost.
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
    ) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        let Some(laid_out) = design.laid_out() else {
            return false;
        };
        let root = laid_out.frames[0];
        let t = Affine::translate((x as f64, y as f64)) * Affine::scale(zoom as f64);
        let scene = scene(&laid_out.list, t, root.width, root.height);
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
            base_color: vello::peniko::Color::TRANSPARENT,
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
        };
        if self
            .vello
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

/// The display list replayed into a vello scene under `t`: each command
/// translated into vello's call, nothing decided here.
fn scene(list: &[Draw], t: Affine, width: f32, height: f32) -> vello::Scene {
    let mut v = vello::Scene::new();
    for draw in list {
        match draw {
            Draw::Fill(path, Paint::Solid(c)) => v.fill(Fill::NonZero, t, *c, None, path),
            Draw::Fill(path, Paint::Gradient(g)) => v.fill(Fill::NonZero, t, g, None, path),
            Draw::Fill(path, Paint::Image(data, place)) => v.fill(
                Fill::NonZero,
                t,
                &ImageBrush::new(data.clone()),
                Some(*place),
                path,
            ),
            Draw::Stroke(path, width, c) => {
                v.stroke(&vello::kurbo::Stroke::new(*width as f64), t, *c, None, path)
            }
            Draw::Shadow(rect, radius, std_dev, c) => {
                v.draw_blurred_rounded_rect(t, *rect, *c, *radius as f64, *std_dev as f64)
            }
            Draw::Glyphs(font, size, c, glyphs) => v
                .draw_glyphs(font)
                .font_size(*size)
                .transform(t)
                .hint(true)
                .brush(*c)
                .draw(
                    Fill::NonZero,
                    glyphs.iter().map(|g| vello::Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    }),
                ),
            Draw::PushClip(path) => v.push_clip_layer(Fill::NonZero, t, path),
            Draw::PushOpacity(o) => v.push_layer(
                Fill::NonZero,
                vello::peniko::Mix::Normal,
                *o,
                t,
                &vello::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
            ),
            Draw::Pop => v.pop_layer(),
        }
    }
    v
}
