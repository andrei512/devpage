use super::*;

/// The band writer holds one band of tiles at a time: every tile of a
/// band is let go before the next band's first is asked for, and the
/// rows it writes are the tiles' rows, in order.
#[test]
fn bands_hold_one_row_of_tiles_at_a_time() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Held(Vec<u8>, Rc<Cell<usize>>);
    impl AsRef<[u8]> for Held {
        fn as_ref(&self) -> &[u8] {
            &self.0
        }
    }
    impl Drop for Held {
        fn drop(&mut self) {
            self.1.set(self.1.get() - 1);
        }
    }
    let (alive, most) = (Rc::new(Cell::new(0)), Cell::new(0));
    let (w, h) = (600u32, 1000u32);
    let mut png = Vec::new();
    bands(w, h, &mut png, |piece| {
        let Piece::Tile { col, row } = piece else {
            panic!("a drawing 600 wide is banded in whole tiles: {piece:?}");
        };
        alive.set(alive.get() + 1);
        most.set(most.get().max(alive.get()));
        // Each pixel says which tile it is in.
        let px = [col as u8, row as u8, 0, 255];
        Ok::<_, ()>(Held(px.repeat((TILE * TILE) as usize), alive.clone()))
    })
    .unwrap();
    assert_eq!((alive.get(), most.get()), (0, 3));
    let image = crate::resolve::decode(&png).unwrap();
    assert_eq!((image.width, image.height), (w, h));
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        image.data.data()[i..i + 2].to_vec()
    };
    assert_eq!(at(599, 999), vec![2, 3]);
    assert_eq!(at(256, 255), vec![1, 0]);
    assert_eq!(at(255, 256), vec![0, 1]);
    // A tile that can't be had stops the PNG there.
    let stopped = bands(w, h, Vec::new(), |piece| {
        if matches!(piece, Piece::Tile { row: 1, .. }) {
            Err("no tile")
        } else {
            Ok(vec![0u8; (TILE * TILE * 4) as usize])
        }
    });
    assert!(matches!(stopped, Err(Band::Tile("no tile"))));
}

/// Up to 16,384 pixels wide a band is a whole row of tiles; wider, it holds
/// as many rows as fit in `BAND_BYTES`, never less than one.
#[test]
fn a_band_holds_at_most_its_bytes() {
    assert_eq!(rows_per_band(1), TILE);
    assert_eq!(rows_per_band(16_384), TILE);
    assert!(rows_per_band(16_385) < TILE);
    assert_eq!(rows_per_band(1_000_000), 4);
    assert_eq!(rows_per_band(u32::MAX), 1);
    for w in [16_385u32, 100_000, 1_000_000, 4_000_000] {
        let held = w.div_ceil(TILE) as usize * (TILE * rows_per_band(w)) as usize * 4;
        assert!(held <= BAND_BYTES, "{w}: {held}");
    }
}

/// A drawing 1,000,000 pixels wide has its PNG made in strips, holding at
/// most `BAND_BYTES` of them at once, with its pixels where they belong: at
/// both ends, across a strip's seam and a band's.
#[test]
fn a_drawing_a_million_pixels_wide_has_its_png_within_a_band() {
    use std::cell::Cell;
    let src = r##"<Frame width={1000000} height={12} fill="#ffffff">
  <Rect x={0} y={0} width={100} height={2} fill="#ff0000" />
  <Rect x={500000} y={2} width={512} height={4} fill="#00ff00" />
  <Rect x={999900} y={8} width={100} height={4} fill="#0000ff" />
</Frame>"##;
    let files: std::collections::HashMap<String, String> =
        [("a.design".to_string(), src.to_string())].into();
    let scene = crate::resolve::resolve(&files, "a.design").unwrap();
    let d = crate::Renderer::new()
        .drawing(&scene, crate::Theme::Light, None)
        .unwrap();
    struct Held<'a>(Vec<u8>, &'a Cell<usize>);
    impl AsRef<[u8]> for Held<'_> {
        fn as_ref(&self) -> &[u8] {
            &self.0
        }
    }
    impl Drop for Held<'_> {
        fn drop(&mut self) {
            self.1.set(self.1.get() - self.0.len());
        }
    }
    let (alive, most) = (Cell::new(0usize), Cell::new(0usize));
    let mut png = Vec::new();
    bands(1_000_000, 12, &mut png, |piece| {
        let Piece::Strip { col, y, rows } = piece else {
            panic!("a drawing a million wide is banded in strips: {piece:?}");
        };
        let px = crate::cpu::strip(&d.list, col, y, rows);
        assert_eq!(px.len(), (TILE * rows * 4) as usize);
        alive.set(alive.get() + px.len());
        most.set(most.get().max(alive.get()));
        Ok::<_, ()>(Held(px, &alive))
    })
    .unwrap();
    assert!(most.get() <= BAND_BYTES, "{}", most.get());
    let image = crate::resolve::decode(&png).unwrap();
    assert_eq!((image.width, image.height), (1_000_000, 12));
    let at = |x: u32, y: u32| {
        let i = ((y as usize * 1_000_000) + x as usize) * 4;
        image.data.data()[i..i + 4].to_vec()
    };
    assert_eq!(at(0, 0), vec![255, 0, 0, 255]);
    assert_eq!(at(99, 1), vec![255, 0, 0, 255]);
    assert_eq!(at(100, 0), vec![255, 255, 255, 255]);
    assert_eq!(at(999_999, 11), vec![0, 0, 255, 255]);
    // The green bar crosses a strip's seam (500,224 is col 1953's edge) and,
    // rows 2 to 5, a band's (bands of 4 rows).
    for (x, y) in [(500_000, 2), (500_223, 3), (500_224, 4), (500_511, 5)] {
        assert_eq!(at(x, y), vec![0, 255, 0, 255], "{x},{y}");
    }
    assert_eq!(at(500_000, 6), vec![255, 255, 255, 255]);
}

/// A band of strips is the band of tiles it replaces, pixel for pixel: the
/// same drawing banded both ways.
#[test]
fn strips_paint_as_tiles() {
    let scene = crate::resolve(
        &crate::render::testing::repo(),
        "docs/designs/checkout/checkout.design",
    )
    .unwrap();
    let d = crate::Renderer::new()
        .drawing(&scene, crate::Theme::Light, None)
        .unwrap();
    let w = d.area().width as u32;
    for col in 0..w.div_ceil(TILE) {
        let tile = crate::cpu::tile(&d.list, 1.0, col, 0);
        for (y, rows) in [(0, 4), (4, 3), (7, 64), (71, TILE - 71)] {
            let strip = crate::cpu::strip(&d.list, col, y, rows);
            let start = (y * TILE * 4) as usize;
            let want = &tile[start..start + strip.len()];
            let d = crate::compare::compare(&strip, want, TILE as usize).unwrap();
            assert!(d.within_tolerance() && d.max <= 2, "col {col} y {y}: {d:?}");
        }
    }
}
