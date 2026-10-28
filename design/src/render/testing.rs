//! What the render tests share: designs from text and from the repository,
//! pixels read back, PNGs made.

use std::path::PathBuf;

use std::collections::HashMap;

use vello_cpu::Pixmap;

use crate::Files;
use crate::render::*;
use crate::resolve::{self, Scene};

pub(crate) fn scene(source: &str) -> Scene {
    let files: HashMap<String, String> = [("a.design".to_string(), source.to_string())].into();
    resolve::resolve(&files, "a.design").unwrap()
}

pub(crate) fn at(x: f32, y: f32, width: f32, height: f32) -> Frame {
    Frame {
        x,
        y,
        width,
        height,
    }
}

pub(crate) fn rgba(p: &Pixmap, x: u16, y: u16) -> [u8; 4] {
    let c = p.sample(x, y).to_u8_array();
    // Unpremultiply, to compare with the colors a design names.
    if c[3] == 0 {
        return [0; 4];
    }
    let u = |v: u8| ((v as u32 * 255 + c[3] as u32 / 2) / c[3] as u32) as u8;
    [u(c[0]), u(c[1]), u(c[2]), c[3]]
}

pub(crate) fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

pub(crate) fn png_bytes(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut w_ = e.write_header().unwrap();
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            data.extend(px(x, y));
        }
    }
    w_.write_image_data(&data).unwrap();
    drop(w_);
    out
}

/// The repository's files, read from disk as the server reads them.
pub(crate) struct Repo(pub(crate) PathBuf);

impl Files for Repo {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(path)).ok()
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(self.0.join(path))
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Some(names)
    }
}

pub(crate) fn repo() -> Repo {
    Repo(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
}
