//! The canvas on the GPU: the engine's display list replayed into a `vello`
//! scene and drawn with WebGPU.

use devpage_design::render::Draw;
use devpage_design::{Renderer, Scene};
use vello::kurbo::Affine;
use vello::peniko::Fill;
use vello::wgpu;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    vello: vello::Renderer,
    blitter: wgpu::util::TextureBlitter,
    engine: Renderer,
}

#[wasm_bindgen]
impl Gpu {
    /// Opens WebGPU on `canvas`; fails where the browser has none.
    pub async fn open(canvas: web_sys::HtmlCanvasElement) -> Result<Gpu, JsValue> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let format = surface.get_capabilities(&adapter).formats[0];
        let vello = vello::Renderer::new(&device, vello::RendererOptions::default())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let blitter = wgpu::util::TextureBlitter::new(&device, format);
        Ok(Gpu {
            device,
            queue,
            surface,
            format,
            vello,
            blitter,
            engine: Renderer::new(),
        })
    }

    /// The scene's size in scene pixels, `[width, height]`, or empty.
    pub fn size(&mut self) -> Vec<f32> {
        crate::SCENE.with(|s| match s.borrow().as_ref() {
            Some(scene) => {
                let root = self.engine.layout(scene)[0];
                vec![root.width, root.height]
            }
            None => Vec::new(),
        })
    }

    /// Draws the scene the watch stream built, at `scale` device pixels per
    /// scene pixel. Returns false when there is no scene yet.
    pub fn draw(&mut self, scale: f32) -> bool {
        crate::SCENE.with(|s| match s.borrow().as_ref() {
            Some(scene) => {
                self.draw_scene(scene, scale);
                true
            }
            None => false,
        })
    }
}

impl Gpu {
    fn draw_scene(&mut self, scene: &Scene, scale: f32) {
        let (root, list) = self.engine.display_list(scene);
        let (w, h) = (
            ((root.width * scale).ceil() as u32).max(1),
            ((root.height * scale).ceil() as u32).max(1),
        );
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                width: w,
                height: h,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                color_space: Default::default(),
            },
        );
        let t = Affine::scale(scale as f64);
        let mut v = vello::Scene::new();
        for draw in &list {
            match draw {
                Draw::Fill(path, c) => v.fill(Fill::NonZero, t, *c, None, path),
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
                    &vello::kurbo::Rect::new(0.0, 0.0, root.width as f64, root.height as f64),
                ),
                Draw::Pop => v.pop_layer(),
            }
        }
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let params = vello::RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: w,
            height: h,
            antialiasing_method: vello::AaConfig::Area,
        };
        if self
            .vello
            .render_to_texture(&self.device, &self.queue, &v, &view, &params)
            .is_err()
        {
            return;
        }
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = self.surface.get_current_texture()
        else {
            return;
        };
        let out = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.blitter.copy(&self.device, &mut encoder, &view, &out);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }
}
