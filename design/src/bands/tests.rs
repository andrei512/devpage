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
    bands(w, h, &mut png, |col, row| {
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
    let stopped = bands(w, h, Vec::new(), |_, row| {
        if row == 1 {
            Err("no tile")
        } else {
            Ok(vec![0u8; (TILE * TILE * 4) as usize])
        }
    });
    assert!(matches!(stopped, Err(Band::Tile("no tile"))));
}
