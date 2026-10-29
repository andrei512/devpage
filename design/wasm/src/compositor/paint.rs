//! Painting a tile on the GPU: the engine's one paint entry point into a
//! `vello` scene, rendered into a texture of its own — with the renderer
//! whose atlas was first given every image of the drawing, in its order, so
//! it holds them as the engine placed them.

use devpage_design::display::List;
use devpage_design::tiles::{Key, TILE};
use vello::kurbo::Affine;
use vello::wgpu;

use super::{Gpu, Tile};

impl Gpu {
    /// A renderer whose atlas holds exactly `images`, placed as the engine
    /// placed them; `None` when vello can't make one.
    pub(super) fn renderer(&self, images: &[vello::peniko::ImageData]) -> Option<vello::Renderer> {
        let mut renderer =
            vello::Renderer::new(&self.device, vello::RendererOptions::default()).ok()?;
        self.prime(&mut renderer, images);
        Some(renderer)
    }

    /// Tile `key` of `list` painted with `renderer`, whose atlas holds the
    /// drawing's images, `first` the first of them; bound for the compositor
    /// by `layout` and `sampler`; `None` when the render failed.
    pub(super) fn paint(
        &self,
        renderer: &mut vello::Renderer,
        list: &List,
        key: &Key,
        first: Option<&vello::peniko::ImageData>,
        (layout, sampler): (&wgpu::BindGroupLayout, &wgpu::Sampler),
    ) -> Option<Tile> {
        let scale = key.level.scale();
        let area = devpage_design::tiles::tile_rect(list, scale, key.col, key.row);
        let mut gpu = crate::vello_backend::Vello {
            scene: vello::Scene::new(),
        };
        devpage_design::display::paint(list, area, scale, &mut gpu);
        // vello resolves a scene that binds no image, gradient or glyph run
        // as needing a 1×1 atlas: it frees the atlas texture while its image
        // cache still counts every image as uploaded, and every tile painted
        // after finds them gone. A tile that binds nothing names one of the
        // drawing's images, outside itself, so the atlas stays as it is.
        if let Some(image) = first
            && gpu.scene.encoding().resources.patches.is_empty()
        {
            gpu.scene.fill(
                vello::peniko::Fill::NonZero,
                Affine::IDENTITY,
                &vello::peniko::ImageBrush::new(image.clone()),
                None,
                &vello::kurbo::Rect::new(-1.0, -1.0, 0.0, 0.0),
            );
        }
        let texture = self.tile_texture();
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let params = vello::RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: TILE,
            height: TILE,
            antialiasing_method: vello::AaConfig::Area,
        };
        renderer
            .render_to_texture(&self.device, &self.queue, &gpu.scene, &view, &params)
            .ok()?;
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Some(Tile {
            _texture: texture,
            group,
        })
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
