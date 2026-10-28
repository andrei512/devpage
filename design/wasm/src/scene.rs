//! The display list replayed into a `vello` scene: each command translated
//! into vello's call, nothing decided here. The canvas renders it on the GPU;
//! the tests hand it to vello's own resolver, to hold the engine's atlas
//! model to vello's.

use devpage_design::render::{Draw, Frame, Paint};
use vello::kurbo::Affine;
use vello::peniko::{BlendMode, Compose, Fill, ImageBrush, Mix};

/// The display list replayed into a vello scene under `t`: each command
/// translated into vello's call, nothing decided here.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn scene(list: &[Draw], t: Affine, area: Frame) -> vello::Scene {
    let mut v = vello::Scene::new();
    // vello's GPU shader samples an image at each pixel's center but a
    // gradient at its corner (`fine.wgsl`), where vello_cpu takes the center
    // for both; so a gradient goes to the GPU moved half a device pixel up
    // and left, to be sampled where the display list means.
    let centered = Some(t.inverse() * Affine::translate((-0.5, -0.5)) * t);
    for draw in list {
        match draw {
            Draw::Fill(path, Paint::Solid(c)) => v.fill(Fill::NonZero, t, *c, None, path),
            Draw::Fill(path, Paint::Gradient(g)) => v.fill(Fill::NonZero, t, g, centered, path),
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
            Draw::Glyphs(font, size, c, coords, glyphs) => v
                .draw_glyphs(font)
                .font_size(*size)
                .normalized_coords(coords)
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
            Draw::PushOpacity(o, Some(clip)) => {
                v.push_layer(Fill::NonZero, Mix::Normal, *o, t, clip)
            }
            Draw::PushOpacity(o, None) => v.push_layer(
                Fill::NonZero,
                Mix::Normal,
                *o,
                t,
                &vello::kurbo::Rect::new(
                    area.x as f64,
                    area.y as f64,
                    (area.x + area.width) as f64,
                    (area.y + area.height) as f64,
                ),
            ),
            Draw::PushAdd(bounds) => v.push_layer(
                Fill::NonZero,
                BlendMode::new(Mix::Normal, Compose::Plus),
                1.0,
                t,
                bounds,
            ),
            Draw::Pop => v.pop_layer(),
        }
    }
    v
}

/// The `vello` backend of the engine's one paint entry point: each command
/// translated into vello's call into a scene, nothing decided here.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct Gpu {
    pub scene: vello::Scene,
}

impl devpage_design::display::Target for Gpu {
    fn draw(&mut self, draw: &Draw, t: Affine) {
        let v = &mut self.scene;
        // vello's GPU shader samples a gradient at each pixel's corner, where
        // vello_cpu takes the center: so a gradient goes moved half a device
        // pixel up and left, to be sampled where the display list means.
        let centered = Some(t.inverse() * Affine::translate((-0.5, -0.5)) * t);
        match draw {
            Draw::Fill(path, Paint::Solid(c)) => v.fill(Fill::NonZero, t, *c, None, path),
            Draw::Fill(path, Paint::Gradient(g)) => v.fill(Fill::NonZero, t, g, centered, path),
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
            Draw::Glyphs(font, size, c, coords, glyphs) => v
                .draw_glyphs(font)
                .font_size(*size)
                .normalized_coords(coords)
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
            _ => unreachable!("layers are property-tree nodes"),
        }
    }
    fn push_clip(&mut self, path: &vello::kurbo::BezPath, t: Affine) {
        self.scene.push_clip_layer(Fill::NonZero, t, path);
    }
    fn push_layer(
        &mut self,
        blend: devpage_design::display::Blend,
        bounds: Option<&vello::kurbo::BezPath>,
        t: Affine,
        area: vello::kurbo::Rect,
    ) {
        let (mode, alpha) = match blend {
            devpage_design::display::Blend::Opacity(o) => {
                (BlendMode::new(Mix::Normal, Compose::SrcOver), o)
            }
            devpage_design::display::Blend::Add => {
                (BlendMode::new(Mix::Normal, Compose::Plus), 1.0)
            }
        };
        match bounds {
            Some(b) => self.scene.push_layer(Fill::NonZero, mode, alpha, t, b),
            None => self
                .scene
                .push_layer(Fill::NonZero, mode, alpha, Affine::IDENTITY, &area),
        }
    }
    fn pop(&mut self) {
        self.scene.pop_layer();
    }
}

/// The engine's atlas model held to vello's own: `devpage_design::atlas`
/// decides which drawings can be painted on the GPU, and vello's resolver —
/// the very one the canvas runs, as the module's vello ships it — places the
/// images. A vello upgrade that places or grows its atlas differently fails
/// here.
#[cfg(test)]
mod tests {
    use super::*;
    use devpage_design::Renderer;
    use devpage_design::atlas;
    use devpage_design::resolve::{self, ImageFile, ImageId};
    use std::collections::HashMap;
    use std::sync::Arc;
    use vello::low_level::vello_encoding::Resolver;
    use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

    /// Where vello's resolver, on a fresh cache, puts each image `scene`
    /// paints, by blob: those it leaves out have none.
    fn vello_places(scene: &vello::Scene) -> HashMap<u64, (u32, u32)> {
        let mut resolver = Resolver::new();
        let mut packed = Vec::new();
        let (_, _, images) = resolver.resolve(scene.encoding(), &mut packed);
        images
            .images
            .iter()
            .map(|(image, x, y)| (image.data.id(), (*x, *y)))
            .collect()
    }

    /// Asserts that the model and vello agree on `images`, painted in that
    /// order: on whether they all fit, on the first left out when they
    /// don't, and on where each one goes when they do.
    fn agree(images: &[ImageData], scene: &vello::Scene, case: &str) {
        let sizes: Vec<(u32, u32)> = images.iter().map(|i| (i.width, i.height)).collect();
        let vello = vello_places(scene);
        match atlas::place(&sizes) {
            Ok(places) => {
                let theirs: Vec<_> = images.iter().map(|i| vello.get(&i.data.id())).collect();
                let ours: Vec<_> = places.iter().collect();
                assert_eq!(
                    theirs.iter().map(|p| p.copied()).collect::<Vec<_>>(),
                    ours.iter().map(|p| Some(**p)).collect::<Vec<_>>(),
                    "{case}: {sizes:?}"
                );
            }
            Err(n) => {
                assert!(
                    images[..n].iter().all(|i| vello.contains_key(&i.data.id())),
                    "{case}: vello left out one before {n}: {sizes:?}"
                );
                assert!(
                    !vello.contains_key(&images[n].data.id()),
                    "{case}: vello placed {n}, which the model leaves out: {sizes:?}"
                );
            }
        }
    }

    /// An image of `w` × `h` under a blob of its own. vello's resolver reads
    /// only its size and its blob, so its pixels are a token.
    fn image(w: u32, h: u32) -> ImageData {
        ImageData {
            data: Blob::new(Arc::new(vec![0u8; 4])),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: w,
            height: h,
        }
    }

    /// The display list that paints `images`, each once, in order.
    fn list(images: &[ImageData]) -> (Frame, Vec<Draw>) {
        let area = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let rect = vello::kurbo::Rect::new(0.0, 0.0, 10.0, 10.0);
        let list = images
            .iter()
            .map(|i| {
                Draw::Fill(
                    vello::kurbo::Shape::to_path(&rect, 0.1),
                    Paint::Image(i.clone(), Affine::IDENTITY),
                )
            })
            .collect();
        (area, list)
    }

    /// Thousands of random sets of images — from a few pixels to the
    /// largest file, a handful to dozens, many right at the edge of what the
    /// atlas holds — and the model and vello agree on every one.
    #[test]
    fn the_atlas_model_places_images_as_vellos_resolver_does() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as u32
        };
        let (mut fit, mut left_out) = (0, 0);
        for case in 0..4000 {
            // Sizes from a few kinds of drawing: icons and shadows alone,
            // which stay in the first atlas or its first growth, and mixed
            // with photos and the largest files, which fill it by four.
            let small = case % 4 == 3;
            let count = 1 + next(match case % 4 {
                0 => 8,
                1 => 24,
                2 => 60,
                _ => 40,
            });
            let images: Vec<ImageData> = (0..count)
                .map(|_| {
                    let side = |next: &mut dyn FnMut(u32) -> u32| match next(4) {
                        _ if small => 1 + next(300),
                        0 => 1 + next(256),
                        1 => 1 + next(1500),
                        2 => 1 + next(4096),
                        _ => 3000 + next(1097),
                    };
                    image(side(&mut next), side(&mut next))
                })
                .collect();
            let (area, draws) = list(&images);
            let sizes: Vec<_> = images.iter().map(|i| (i.width, i.height)).collect();
            match atlas::place(&sizes) {
                Ok(_) => fit += 1,
                Err(_) => left_out += 1,
            }
            agree(
                &images,
                &scene(&draws, Affine::IDENTITY, area),
                &format!("case {case}"),
            );
        }
        // Both sides of the boundary, many times over.
        eprintln!("{fit} fit, {left_out} don't");
        assert!(fit > 500 && left_out > 500, "{fit} fit, {left_out} don't");
    }

    /// Files of the size their name says, `w`x`h`-n.png, made without
    /// decoding.
    struct Sized(HashMap<String, String>);

    impl devpage_design::Files for Sized {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).map(|s| s.clone().into_bytes())
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            self.0.list(path)
        }
        fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
            let (size, _) = path.strip_suffix(".png")?.split_once('-')?;
            let (w, h) = size.split_once('x')?;
            let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
            Some(Ok(Arc::new(ImageFile {
                id: ImageId::of(path.as_bytes()),
                bytes: Vec::new(),
                width: w,
                height: h,
                pixels: Arc::new(vec![0; 4 * w as usize * h as usize]),
                pixels_width: w,
                pixels_height: h,
            })))
        }
    }

    /// A real drawing — image files, one shown twice, and shadows in one
    /// image and in pieces — replayed as the canvas replays it: vello places
    /// its images where the engine says, and the drawing's images are
    /// exactly those its scene paints. One more large file and they no
    /// longer fit, for the engine and for vello alike.
    #[test]
    fn a_drawings_images_are_placed_by_vello_where_the_engine_says() {
        let design = |files: &[&str]| {
            let images: String = files
                .iter()
                .map(|f| format!("\n  <Image src=\"{f}\" width={{40}} height={{40}} />"))
                .collect();
            let source = format!(
                r##"<Frame layout="row" gap={{8}} padding={{40}}>{images}
  <Rect width={{60}} height={{60}} radius={{12}} shadow={{{{ blur: 8, color: "#000000" }}}} />
  <Rect width={{3000}} height={{40}} radius={{[2, 40, 8, 0]}} shadow={{{{ blur: 2, color: "#000000" }}}} />
  <Ellipse width={{900}} height={{400}} shadow={{{{ blur: 3, color: "#000000" }}}} />
</Frame>"##
            );
            let files = Sized([("a.design".to_string(), source)].into());
            resolve::resolve(&files, "a.design").unwrap()
        };
        let mut renderer = Renderer::new();
        let files = [
            "4096x2000-a.png",
            "3000x3000-b.png",
            "4096x2000-a.png",
            "1200x900-c.png",
            "4000x1000-d.png",
        ];
        let drawing = renderer
            .drawing(&design(&files), devpage_design::Theme::Light, None)
            .unwrap();
        let replayed = scene(&drawing.list, Affine::IDENTITY, drawing.area);
        let painted = vello_places(&replayed);
        assert_eq!(painted.len(), drawing.images.len());
        assert!(drawing.images.len() > 8, "{}", drawing.images.len());
        agree(&drawing.images, &replayed, "the drawing");

        let mut more = files.to_vec();
        more.extend(["4096x4096-e.png", "4096x4096-f.png"]);
        let more = design(&more);
        assert!(
            renderer
                .drawing(&more, devpage_design::Theme::Light, None)
                .is_err()
        );
        // The same list, painted anyway: vello leaves out the image the
        // engine said it would.
        let (area, list) = renderer.display_list(&more, devpage_design::Theme::Light);
        let mut images: Vec<ImageData> = Vec::new();
        for draw in &list {
            if let Draw::Fill(_, Paint::Image(i, _)) = draw
                && !images.iter().any(|j| j.data.id() == i.data.id())
            {
                images.push(i.clone());
            }
        }
        let sizes: Vec<_> = images.iter().map(|i| (i.width, i.height)).collect();
        assert!(atlas::place(&sizes).is_err());
        agree(&images, &scene(&list, Affine::IDENTITY, area), "too many");
    }
}
