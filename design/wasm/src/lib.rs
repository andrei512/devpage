//! The design engine for the browser: raw `extern "C"` exports over linear
//! memory, so building it needs nothing but `cargo`.

use std::collections::HashMap;

use devpage_design::Renderer;

use devpage_design::proto::{Update, update::Body};
use devpage_design::resolve::Node;
use devpage_design::{Scene, wire};
use prost::Message;

thread_local! {
    static RENDERER: std::cell::RefCell<Option<Renderer>> = const { std::cell::RefCell::new(None) };
    static SCENE: std::cell::RefCell<Option<Scene>> = const { std::cell::RefCell::new(None) };
}

/// Applies one watch-stream message: a whole scene or a patch. Returns 1 when
/// the scene changed, 2 for problems (the scene is kept), 0 otherwise.
///
/// # Safety
/// `ptr` must point at `len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn apply(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(update) = Update::decode(bytes) else {
        return 0;
    };
    SCENE.with(|s| {
        let mut s = s.borrow_mut();
        match update.body {
            Some(Body::Scene(root)) => {
                *s = Some(Scene {
                    root: wire::decode(&root),
                    files: Vec::new(),
                });
                1
            }
            Some(Body::Patch(patch)) => match s.as_mut() {
                Some(scene) => {
                    wire::apply(&mut scene.root, &patch.replace);
                    1
                }
                None => 0,
            },
            Some(Body::Problems(_)) => 2,
            None => 0,
        }
    })
}

/// Paints the scene the stream built, like `paint`.
#[unsafe(no_mangle)]
pub extern "C" fn paint_scene(scale: f32) -> *mut u8 {
    SCENE.with(|s| match s.borrow().as_ref() {
        Some(scene) => pixels(scene, scale),
        None => std::ptr::null_mut(),
    })
}

/// The interaction of the topmost node under `(x, y)` that has one, as
/// `[length u32 LE, UTF-8…]`, or null.
#[unsafe(no_mangle)]
pub extern "C" fn click(x: f32, y: f32) -> *mut u8 {
    SCENE.with(|s| {
        let s = s.borrow();
        let Some(scene) = s.as_ref() else {
            return std::ptr::null_mut();
        };
        let frames = RENDERER.with(|r| {
            r.borrow_mut()
                .get_or_insert_with(Renderer::new)
                .layout(scene)
        });
        let mut found = None;
        let mut at = 0;
        hit(&scene.root, &frames, &mut at, x, y, &mut found);
        match found {
            Some(action) => {
                let mut out = (action.len() as u32).to_le_bytes().to_vec();
                out.extend_from_slice(action.as_bytes());
                let ptr = out.as_mut_ptr();
                std::mem::forget(out);
                ptr
            }
            None => std::ptr::null_mut(),
        }
    })
}

/// Walks in paint order, so the last hit is the topmost.
fn hit(
    node: &Node,
    frames: &[devpage_design::render::Frame],
    at: &mut usize,
    x: f32,
    y: f32,
    found: &mut Option<String>,
) {
    let f = frames[*at];
    *at += 1;
    if let Some(action) = &node.on_click
        && x >= f.x
        && y >= f.y
        && x < f.x + f.width
        && y < f.y + f.height
    {
        *found = Some(action.clone());
    }
    for child in &node.children {
        hit(child, frames, at, x, y, found);
    }
}

fn pixels(scene: &Scene, scale: f32) -> *mut u8 {
    let pixmap = RENDERER.with(|r| {
        r.borrow_mut()
            .get_or_insert_with(Renderer::new)
            .paint(scene, scale)
    });
    let mut out = Vec::with_capacity(8 + pixmap.data().len());
    out.extend_from_slice(&pixmap.width().to_le_bytes());
    out.extend_from_slice(&pixmap.height().to_le_bytes());
    out.extend_from_slice(pixmap.data());
    let ptr = out.as_mut_ptr();
    std::mem::forget(out);
    ptr
}

/// Room for `len` bytes the page writes into before a call.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Frees what `alloc` or `paint` returned.
///
/// # Safety
/// `ptr` and `len` must come from one `alloc(len)` or one `paint` result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Paints the design named first in `files` at `scale`. `files` is
/// `path\n<byte length>\n<source>` repeated. Returns a pointer to
/// `[width u32 LE, height u32 LE, RGBA premultiplied…]`, or null on problems.
///
/// # Safety
/// `ptr` must point at `len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn paint(ptr: *const u8, len: usize, scale: f32) -> *mut u8 {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Some((first, files)) = decode(input) else {
        return std::ptr::null_mut();
    };
    let Ok(scene) = devpage_design::resolve(&files, &first) else {
        return std::ptr::null_mut();
    };
    pixels(&scene, scale)
}

fn decode(mut input: &[u8]) -> Option<(String, HashMap<String, String>)> {
    let mut files = HashMap::new();
    let mut first = None;
    while !input.is_empty() {
        let nl = input.iter().position(|&b| b == b'\n')?;
        let path = std::str::from_utf8(&input[..nl]).ok()?.to_string();
        input = &input[nl + 1..];
        let nl = input.iter().position(|&b| b == b'\n')?;
        let n: usize = std::str::from_utf8(&input[..nl]).ok()?.parse().ok()?;
        input = &input[nl + 1..];
        let source = std::str::from_utf8(input.get(..n)?).ok()?.to_string();
        input = &input[n..];
        first.get_or_insert_with(|| path.clone());
        files.insert(path, source);
    }
    Some((first?, files))
}

#[cfg(test)]
mod tests {
    /// Writes what `paint` gives natively, for comparing with the module's
    /// output: `DESIGN_RAW_IN=<input> DESIGN_RAW_OUT=<file> cargo test`.
    #[test]
    fn paint_natively() {
        let (Ok(input), Ok(out)) = (
            std::env::var("DESIGN_RAW_IN"),
            std::env::var("DESIGN_RAW_OUT"),
        ) else {
            return;
        };
        let input = std::fs::read(input).unwrap();
        let ptr = unsafe { super::paint(input.as_ptr(), input.len(), 1.0) };
        let (w, h) = unsafe { (*(ptr as *const u32), *(ptr.add(4) as *const u32)) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, 8 + (w * h * 4) as usize) };
        std::fs::write(out, bytes).unwrap();
    }
}
