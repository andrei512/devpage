//! Whether a drawing's images fit the GPU. `vello` copies every image a frame
//! paints into one atlas texture, at most [`SIDE`] pixels a side, and an image
//! that finds no room there is silently left out of the frame. So the engine
//! asks this model first, and a drawing whose images don't fit is the design's
//! problem rather than a picture with holes.
//!
//! The model is `vello_encoding`'s own image cache (0.11, `image_cache.rs` and
//! `Resolver::resolve_pending_images`) run on a fresh cache, with the same
//! `guillotiere` allocator: images placed in the order the frame first uses
//! them, the atlas starting at [`START`] and doubling, everything placed so far
//! repacked in blob order at each size, until [`SIDE`]. The canvas renders
//! each drawing with a renderer of its own, so its cache is fresh, and the
//! engine makes each drawing's images in the order it first uses them, so blob
//! order is that order on every machine.

use guillotiere::{AtlasAllocator, size2};

/// The atlas's largest side, `vello_encoding`'s `MAX_ATLAS_SIZE`.
pub const SIDE: u32 = 8192;
/// The atlas's first side, `vello_encoding`'s `DEFAULT_ATLAS_SIZE`.
const START: i32 = 1024;

/// Places `images` — each a `(width, height)`, distinct, in the order a frame
/// first paints them — as `vello` does in a fresh atlas. `Err` is the index of
/// the first image it leaves out.
pub fn fits(images: &[(u32, u32)]) -> Result<(), usize> {
    let mut side = START;
    let mut atlas = AtlasAllocator::new(size2(side, side));
    // How many of `images`, from the first, are placed.
    let mut placed = 0;
    while placed < images.len() {
        let (w, h) = images[placed];
        if atlas.allocate(size2(w as i32, h as i32)).is_some() {
            placed += 1;
            continue;
        }
        // No room: a larger atlas, with everything placed so far repacked
        // into it in blob order — which is first-use order.
        loop {
            side *= 2;
            if side > SIDE as i32 {
                return Err(placed);
            }
            let mut bigger = AtlasAllocator::new(size2(side, side));
            if images[..placed]
                .iter()
                .all(|&(w, h)| bigger.allocate(size2(w as i32, h as i32)).is_some())
            {
                atlas = bigger;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_quarters_fill_the_atlas_and_a_fifth_does_not_fit() {
        assert_eq!(fits(&[(4096, 4096); 4]), Ok(()));
        assert_eq!(fits(&[(4096, 4096); 5]), Err(4));
        assert_eq!(fits(&[(8192, 8192)]), Ok(()));
        assert_eq!(fits(&[(8193, 1)]), Err(0));
        // Sixteen 2000-pixel squares pack four by four.
        assert_eq!(fits(&[(2000, 2000); 16]), Ok(()));
        assert_eq!(fits(&[]), Ok(()));
    }
}
