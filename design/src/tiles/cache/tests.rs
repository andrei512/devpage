use crate::resolve::Theme;

use super::*;

fn key(level: f64, col: u32, row: u32) -> Key {
    Key {
        theme: Theme::Light,
        level: Level::of(level),
        col,
        row,
    }
}

#[test]
fn the_cache_holds_its_cap_and_drops_the_least_recently_used_first() {
    let mut c = Cache::new(3);
    for col in 0..3 {
        assert!(c.insert(key(1.0, col, 0), col, 1).is_empty());
    }
    // Using the first makes the second the oldest.
    assert_eq!(c.get(&key(1.0, 0, 0)), Some((&0, false)));
    let dropped = c.insert(key(1.0, 3, 0), 3, 1);
    assert_eq!(dropped, vec![(key(1.0, 1, 0), 1)]);
    assert_eq!((c.len(), c.cost()), (3, 3));
    // Replacing a tile costs nothing more.
    assert!(c.insert(key(1.0, 3, 0), 30, 1).is_empty());
    assert_eq!(c.peek(&key(1.0, 3, 0)), Some((&30, false)));
    // A tile costing more than the rest drops as many as it takes.
    let dropped = c.insert(key(1.0, 9, 9), 9, 3);
    assert_eq!(dropped.len(), 3);
    assert_eq!((c.len(), c.cost()), (1, 3));
}

#[test]
fn damage_marks_exactly_the_tiles_it_overlaps_stale_at_every_level() {
    let mut c = Cache::new(usize::MAX);
    for level in [0.5, 1.0, 2.0] {
        for col in 0..4 {
            for row in 0..4 {
                c.insert(key(level, col, row), (), 1);
            }
        }
    }
    let mut dark = key(1.0, 0, 0);
    dark.theme = Theme::Dark;
    c.insert(dark, (), 1);
    // The drawing's top-left at (-10, 20); a rect inside tile (1, 0) at
    // 1×, in tile (0, 0) at ½×, across tiles (2..=3, 0..=1) at 2×.
    let origin = (-10.0, 20.0);
    let damage = [KRect::new(250.0, 30.0, 300.0, 200.0)];
    c.damage(Theme::Light, origin, &damage);
    let mut stale: Vec<(i64, u32, u32)> = c
        .iter()
        .filter(|(_, _, s)| *s)
        .map(|(k, _, _)| (k.level.0, k.col, k.row))
        .collect();
    stale.sort();
    assert_eq!(
        stale,
        vec![(512, 0, 0), (1024, 1, 0), (2048, 2, 0), (2048, 2, 1)]
    );
    // A stale tile is still returned, as stale, until replaced.
    assert_eq!(c.get(&key(1.0, 1, 0)), Some((&(), true)));
    c.insert(key(1.0, 1, 0), (), 1);
    assert_eq!(c.get(&key(1.0, 1, 0)), Some((&(), false)));
    assert_eq!(c.peek(&dark), Some((&(), false)));
    // An edge that only touches a tile doesn't make it stale.
    c.damage(
        Theme::Light,
        origin,
        &[KRect::new(246.0, 30.0, 246.0, 40.0)],
    );
    assert_eq!(c.peek(&key(1.0, 0, 0)), Some((&(), false)));
}
