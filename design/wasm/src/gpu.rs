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
use devpage_design::tiles::{Cache, Key, Level};

/// A tile's side, in device pixels.
const TILE: u32 = devpage_design::tiles::TILE;

/// A painted tile: its texture, and what binds it to the compositor.
struct Tile {
    _texture: wgpu::Texture,
    group: wgpu::BindGroup,
}

/// The compositor's one pass: a textured quad per tile, blended over what is
/// under it.
fn compositor(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            r#"
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@group(0) @binding(0) var tile: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@vertex fn vs(@builtin(vertex_index) i: u32, @location(0) dst: vec4<f32>) -> Out {
  var corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
  let c = corners[i];
  var o: Out;
  o.pos = vec4(mix(dst.x, dst.z, c.x), mix(dst.y, dst.w, c.y), 0.0, 1.0);
  o.uv = c;
  return o;
}
@fragment fn fs(o: Out) -> @location(0) vec4<f32> { return textureSample(tile, smp, o.uv); }
"#
            .into(),
        ),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, layout)
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
    /// The tiles painted so far, by theme, level, column and row.
    tiles: Cache<Tile>,
    /// The generation of the design the tiles were painted from.
    generation: Option<u32>,
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
        let (pipeline, layout) = compositor(&self.device, format);
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
            tiles: Cache::new(usize::MAX),
            generation: None,
            vello: None,
        })
    }

    /// Composites `design` into `target`, `width` × `height` device pixels,
    /// the design scaled by `zoom` and its top-left at `(x, y)`, over
    /// `background` (`0xRRGGBB`): the cached tiles of the view, moved and
    /// scaled. When `settled`, up to `budget` missing or stale tiles of the
    /// view are painted at `zoom` first, nearest the middle first. Returns
    /// how many tiles of the view at `zoom` are still to paint, or -1 when
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
        background: u32,
        settled: bool,
        budget: u32,
    ) -> i32 {
        let (width, height) = (width.max(1), height.max(1));
        let theme = design.theme();
        let generation = design.generation();
        let Some(laid_out) = design.laid_out() else {
            return -1;
        };
        let blobs: Vec<u64> = laid_out.images.iter().map(|i| i.data.id()).collect();
        let images = laid_out.images.clone();
        // What the design shows changed: every tile is stale, and shown
        // until it is painted again.
        if target.generation != Some(generation) {
            target.generation = Some(generation);
            target.tiles.damage_all(devpage_design::Theme::Light);
            target.tiles.damage_all(devpage_design::Theme::Dark);
        }
        if !matches!(&target.vello, Some((held, _)) if *held == blobs) {
            // A fresh atlas for other images, given every image of the
            // drawing in its order first, so it holds them as the engine
            // placed them; and the tiles painted from the old one go.
            match vello::Renderer::new(&self.device, vello::RendererOptions::default()) {
                Ok(mut renderer) => {
                    self.prime(&mut renderer, &images);
                    target.vello = Some((blobs, renderer));
                    target.tiles.clear();
                }
                Err(_) => return -1,
            }
        }
        let Some(list) = design.laid_out().map(|d| &d.list) else {
            return -1;
        };
        let level = Level::of(zoom as f64);
        let scale = level.scale();
        let side = TILE as f64;
        let (dw, dh) = (
            list.area.width as f64 * scale,
            list.area.height as f64 * scale,
        );
        let (cols, rows) = ((dw / side).ceil() as i64, (dh / side).ceil() as i64);
        let (ox, oy) = (x.round() as f64, y.round() as f64);
        let span = |o: f64, n: f64, count: i64| {
            let first = ((-o) / side).floor().max(0.0) as i64;
            let last = (((n - o) / side).ceil() as i64).min(count);
            first..last.max(first)
        };
        let (cs, rs) = (span(ox, width as f64, cols), span(oy, height as f64, rows));
        let mut missing: Vec<(i64, i64)> = Vec::new();
        for r in rs.clone() {
            for c in cs.clone() {
                let key = Key {
                    theme,
                    level,
                    col: c as u32,
                    row: r as u32,
                };
                if target.tiles.get(&key).is_none_or(|(_, stale)| stale) {
                    missing.push((c, r));
                }
            }
        }
        let (mx, my) = (
            (width as f64 / 2.0 - ox) / side,
            (height as f64 / 2.0 - oy) / side,
        );
        missing.sort_by(|a, b| {
            let d =
                |p: &(i64, i64)| (p.0 as f64 + 0.5 - mx).powi(2) + (p.1 as f64 + 0.5 - my).powi(2);
            d(a).total_cmp(&d(b))
        });
        let mut painted = 0;
        if settled {
            for &(c, r) in missing.iter().take(budget as usize) {
                let area = devpage_design::tiles::tile_rect(list, scale, c as u32, r as u32);
                let mut gpu = crate::scene::Vello {
                    scene: vello::Scene::new(),
                };
                devpage_design::display::paint(list, area, scale, &mut gpu);
                let Some((_, renderer)) = target.vello.as_mut() else {
                    return -1;
                };
                let texture = self.tile_texture();
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let params = vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: TILE,
                    height: TILE,
                    antialiasing_method: vello::AaConfig::Area,
                };
                if renderer
                    .render_to_texture(&self.device, &self.queue, &gpu.scene, &view, &params)
                    .is_err()
                {
                    return -1;
                }
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &target.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&target.sampler),
                        },
                    ],
                });
                target.tiles.insert(
                    Key {
                        theme,
                        level,
                        col: c as u32,
                        row: r as u32,
                    },
                    Tile {
                        _texture: texture,
                        group,
                    },
                    1,
                );
                painted += 1;
            }
        }
        let left = missing.len() as i32 - painted;
        // Every cached tile of this theme that the view shows, coarse levels
        // first, so the tiles at the view's own scale end on top.
        let view_rect = (
            -ox / zoom as f64 + list.area.x as f64,
            -oy / zoom as f64 + list.area.y as f64,
            (width as f64 - ox) / zoom as f64 + list.area.x as f64,
            (height as f64 - oy) / zoom as f64 + list.area.y as f64,
        );
        let mut quads: Vec<([f32; 4], &wgpu::BindGroup, Level)> = Vec::new();
        for (key, tile, _) in target.tiles.iter() {
            if key.theme != theme {
                continue;
            }
            let lv = key.level;
            let rect = devpage_design::tiles::tile_rect(list, lv.scale(), key.col, key.row);
            if rect.x1 < view_rect.0
                || rect.y1 < view_rect.1
                || rect.x0 > view_rect.2
                || rect.y0 > view_rect.3
            {
                continue;
            }
            let to = |dx: f64, dy: f64| {
                let px = ox + (dx - list.area.x as f64) * zoom as f64;
                let py = oy + (dy - list.area.y as f64) * zoom as f64;
                (
                    px / width as f64 * 2.0 - 1.0,
                    1.0 - py / height as f64 * 2.0,
                )
            };
            let (x0, y0) = to(rect.x0, rect.y0);
            let (x1, y1) = to(rect.x1, rect.y1);
            quads.push((
                [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                &tile.group,
                lv,
            ));
        }
        quads.sort_by_key(|q| (q.2 == level, std::cmp::Reverse(q.2.0)));
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
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = target.surface.get_current_texture()
        else {
            return -1;
        };
        let out = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let data: Vec<u8> = quads
            .iter()
            .flat_map(|q| q.0.iter().flat_map(|v| v.to_le_bytes()))
            .collect();
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: if data.is_empty() { &[0; 16] } else { &data },
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let bg = |shift: u32| ((background >> shift) & 0xff) as f64 / 255.0;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg(16),
                            g: bg(8),
                            b: bg(0),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&target.pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            for (i, q) in quads.iter().enumerate() {
                pass.set_bind_group(0, q.1, &[]);
                pass.draw(0..6, i as u32..i as u32 + 1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        left
    }

    /// One render of every image `images` holds, in order, into a pixel:
    /// the atlas then holds exactly them, placed as the engine placed them.
    fn prime(&self, renderer: &mut vello::Renderer, images: &[vello::peniko::ImageData]) {
        let mut scene = vello::Scene::new();
        for image in images {
            scene.fill(
                vello::peniko::Fill::NonZero,
                Affine::IDENTITY,
                &vello::peniko::ImageBrush::new(image.clone()),
                None,
                &vello::kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
            );
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let params = vello::RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: 1,
            height: 1,
            antialiasing_method: vello::AaConfig::Area,
        };
        let _ = renderer.render_to_texture(&self.device, &self.queue, &scene, &view, &params);
    }

    fn tile_texture(&self) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: TILE,
                height: TILE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }
}
