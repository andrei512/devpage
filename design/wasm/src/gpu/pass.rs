//! The compositor's one pass: a textured quad per cached tile, each placed
//! and scaled where the view shows it, blended over the page's background
//! into the canvas.

use vello::wgpu;

use super::Tile;

/// The compositor's one pass: a textured quad per tile, blended over what is
/// under it.
pub(super) fn compositor(
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

/// Draws `quads` — each tile's place in device pixels and what binds it —
/// into `surface`, `width` × `height`, over `background` (`0xRRGGBB`); false
/// when the frame was lost.
#[allow(clippy::too_many_arguments)]
pub(super) fn composite(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    surface: &wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    pipeline: &wgpu::RenderPipeline,
    (width, height): (u32, u32),
    background: u32,
    quads: &[(vello::kurbo::Rect, &Tile)],
) -> bool {
    surface.configure(
        device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
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
    | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = surface.get_current_texture()
    else {
        return false;
    };
    let out = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    // Each quad in clip space: x right, y up.
    let ndc = |r: &vello::kurbo::Rect| {
        [
            r.x0 / width as f64 * 2.0 - 1.0,
            1.0 - r.y0 / height as f64 * 2.0,
            r.x1 / width as f64 * 2.0 - 1.0,
            1.0 - r.y1 / height as f64 * 2.0,
        ]
    };
    let data: Vec<u8> = quads
        .iter()
        .flat_map(|(r, _)| ndc(r).into_iter().flat_map(|v| (v as f32).to_le_bytes()))
        .collect();
    let buffer = wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: None,
            contents: if data.is_empty() { &[0; 16] } else { &data },
            usage: wgpu::BufferUsages::VERTEX,
        },
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (i, (_, tile)) in quads.iter().enumerate() {
            pass.set_bind_group(0, &tile.group, &[]);
            pass.draw(0..6, i as u32..i as u32 + 1);
        }
    }
    queue.submit([encoder.finish()]);
    queue.present(frame);
    true
}
