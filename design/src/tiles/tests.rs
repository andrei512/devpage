use super::*;

/// A view of a drawing 1,000 × 100,000 at 1×, 600 × 400 device pixels,
/// scrolled 300 down: the tiles it needs are the rows it crosses, the middle
/// first, and a tile below it lands nowhere.
#[test]
fn a_view_needs_the_tiles_it_crosses_its_middle_first() {
    let area = KRect::new(0.0, 0.0, 1000.0, 100_000.0);
    let view = View {
        width: 600.0,
        height: 400.0,
        zoom: 1.0,
        origin: (0.0, -300.0),
    };
    let level = Level::of(1.0);
    let tiles = view.tiles(area, level);
    let mut sorted = tiles.clone();
    sorted.sort();
    assert_eq!(sorted, vec![(0, 1), (0, 2), (1, 1), (1, 2), (2, 1), (2, 2)]);
    assert_eq!(tiles[0], (1, 1));
    let key = |row| Key {
        theme: Theme::Light,
        level,
        col: 0,
        row,
    };
    assert_eq!(
        view.place(area, &key(1)),
        Some(KRect::new(0.0, -44.0, 256.0, 212.0))
    );
    assert_eq!(view.place(area, &key(9)), None);
}
