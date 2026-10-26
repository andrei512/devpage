//! Whether a drawing's images fit the GPU, and where. `vello` copies every
//! image a frame paints into one atlas texture, at most [`SIDE`] pixels a
//! side, and an image that finds no room there is silently left out of the
//! frame. So the engine asks this model first, and a drawing whose images
//! don't fit is the design's problem rather than a picture with holes.
//!
//! The model is `vello_encoding`'s own image cache (0.11, `image_cache.rs` and
//! `Resolver::resolve_pending_images`) run on a fresh cache, with the same
//! `guillotiere` allocator: images placed in the order the frame first uses
//! them, the atlas starting at [`START`] and doubling, everything placed so far
//! repacked in blob order at each size, until [`SIDE`]. The canvas keeps a
//! vello renderer — and so its atlas — only while the drawings it renders
//! paint the very same blobs, in the same order: the atlas then holds those
//! images and nothing else, placed as a fresh one would place them. Any other
//! drawing gets a fresh renderer. The engine makes each drawing's new blobs in
//! the order it first uses them, so blob order is that order on every machine.
//! A test in the module runs vello's own resolver on the same images and
//! holds the two to the same fit and the same places.

use guillotiere::{AtlasAllocator, size2};

/// The atlas's largest side, `vello_encoding`'s `MAX_ATLAS_SIZE`.
pub const SIDE: u32 = 8192;
/// The atlas's first side, `vello_encoding`'s `DEFAULT_ATLAS_SIZE`.
const START: i32 = 1024;

/// Places `images` — each a `(width, height)`, distinct, in the order a frame
/// first paints them — as `vello` does in a fresh atlas, and gives each one's
/// top-left corner there. `Err` is the index of the first image it leaves out.
pub fn place(images: &[(u32, u32)]) -> Result<Vec<(u32, u32)>, usize> {
    let mut side = START;
    let mut atlas = AtlasAllocator::new(size2(side, side));
    let mut at = Vec::with_capacity(images.len());
    let corner = |a: guillotiere::Allocation| (a.rectangle.min.x as u32, a.rectangle.min.y as u32);
    while at.len() < images.len() {
        let (w, h) = images[at.len()];
        if let Some(a) = atlas.allocate(size2(w as i32, h as i32)) {
            at.push(corner(a));
            continue;
        }
        // No room: a larger atlas, with everything placed so far repacked
        // into it in blob order — which is first-use order.
        loop {
            side *= 2;
            if side > SIDE as i32 {
                return Err(at.len());
            }
            let mut bigger = AtlasAllocator::new(size2(side, side));
            let repacked: Option<Vec<_>> = images[..at.len()]
                .iter()
                .map(|&(w, h)| bigger.allocate(size2(w as i32, h as i32)).map(corner))
                .collect();
            if let Some(repacked) = repacked {
                atlas = bigger;
                at = repacked;
                break;
            }
        }
    }
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_quarters_fill_the_atlas_and_a_fifth_does_not_fit() {
        assert_eq!(
            place(&[(4096, 4096); 4]),
            Ok(vec![(0, 0), (0, 4096), (4096, 0), (4096, 4096)])
        );
        assert_eq!(place(&[(4096, 4096); 5]), Err(4));
        assert_eq!(place(&[(8192, 8192)]), Ok(vec![(0, 0)]));
        assert_eq!(place(&[(8193, 1)]), Err(0));
        // Sixteen 2000-pixel squares pack four by four.
        assert!(place(&[(2000, 2000); 16]).is_ok());
        assert_eq!(place(&[]), Ok(vec![]));
    }
}
