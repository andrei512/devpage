//! What an edit costs by the kind of frame it is in: a list's widest row
//! made wider, a list's own fill changed, a child of a frame with no layout
//! resized or inserted first, a sibling of a `"fill"` child resized, and a
//! child of a `"space-between"` frame resized — each at 1,000, 10,000 and
//! 100,000 siblings, the save resolved again and the drawing brought across.

use crate::render::Drawing;
use crate::resolve::Theme;
use crate::resolve::{Outcome, Resolution};

use super::super::*;

/// A design of `n` siblings as `kind` lays them out, the `edit`ed one at
/// `n / 2` (or a new one first) changed.
pub(super) fn source(kind: Kind, n: usize, edited: bool) -> String {
    let mut rows = String::new();
    if edited && kind == Kind::NoLayoutFront {
        rows.push_str("<Rect x={0} y={0} width={50} height={10} fill=\"#000000\" />");
    }
    for i in 0..n {
        let e = edited && i == n / 2;
        rows.push_str(&match kind {
            Kind::Widest | Kind::ParentFill => format!(
                "<Frame layout=\"row\" padding={{4}}><Text>{}</Text></Frame>",
                if e && kind == Kind::Widest {
                    "Edited and a good deal longer".to_string()
                } else {
                    format!("Row {i}")
                }
            ),
            Kind::NoLayoutResize | Kind::NoLayoutFront => format!(
                "<Rect x={{0}} y={{{}}} width={{{}}} height={{8}} fill=\"#336699\" />",
                i * 9,
                if e && kind == Kind::NoLayoutResize {
                    150
                } else {
                    100
                }
            ),
            Kind::FixedFree => format!(
                "<Frame x={{0}} y={{{}}} layout=\"column\" width={{{}}}><Text width=\"fill\">Row {i}</Text></Frame>",
                i * 9,
                if e { 300 } else { 200 }
            ),
            Kind::Align => format!(
                "<Frame layout=\"row\" padding={{4}}><Text>{}</Text></Frame>",
                "Row ".repeat(1 + i % 3)
            ),
            Kind::FillSibling | Kind::SpaceBetween => format!(
                "<Rect width={{100}} height={{{}}} fill=\"#336699\" />",
                if e { 3 } else { 2 }
            ),
        });
    }
    let fill_child = if kind == Kind::FillSibling {
        "<Rect width={100} height=\"fill\" fill=\"#000000\" />"
    } else {
        ""
    };
    let root = match kind {
        Kind::Widest => "<Frame layout=\"column\">".to_string(),
        Kind::ParentFill => format!(
            "<Frame layout=\"column\"{}>",
            if edited { " fill=\"#ff0000\"" } else { "" }
        ),
        Kind::NoLayoutResize | Kind::NoLayoutFront | Kind::FixedFree => "<Frame>".to_string(),
        Kind::Align => format!(
            "<Frame layout=\"column\"{}>",
            if edited { " align=\"center\"" } else { "" }
        ),
        Kind::FillSibling => format!("<Frame layout=\"column\" height={{{}}}>", n * 3 + 100),
        Kind::SpaceBetween => format!(
            "<Frame layout=\"column\" justify=\"space-between\" height={{{}}}>",
            n * 3 + 100
        ),
    };
    format!("{root}{rows}{fill_child}</Frame>")
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Kind {
    Widest,
    ParentFill,
    NoLayoutResize,
    NoLayoutFront,
    FillSibling,
    SpaceBetween,
    /// A child of a frame with no layout, of a fixed width holding wrapping
    /// text, made wider: no sibling depends on the frame's new room.
    FixedFree,
    /// A list of rows of three widths centred: every row's box moves across,
    /// so every row is placed again — and only the rows, not what is in them.
    Align,
}

/// The kinds whose edit costs the nodes it changes and log time.
pub(super) const KINDS: [Kind; 7] = [
    Kind::Widest,
    Kind::ParentFill,
    Kind::NoLayoutResize,
    Kind::NoLayoutFront,
    Kind::FillSibling,
    Kind::SpaceBetween,
    Kind::FixedFree,
];

/// The save of `kind`'s edit at `n`: resolving it again, and the drawing's
/// update, each timed, and how many nodes the update visited. The drawing
/// it leaves, and a fresh one of the edited design.
pub(super) fn edit(
    kind: Kind,
    n: usize,
) -> (
    std::time::Duration,
    std::time::Duration,
    usize,
    Drawing,
    Drawing,
    Renderer,
) {
    let mut saves = crate::ids::testing::Saves::default();
    saves.save("a.design", source(kind, n, false));
    let mut res = Resolution::new(&saves, "a.design").unwrap();
    let mut r = Renderer::new();
    let mut d = r.drawing(res.scene(), Theme::Light, None).unwrap();
    saves.save("a.design", source(kind, n, true));
    let t = std::time::Instant::now();
    let Outcome::Changed(_, applied) = res.update(&saves, &["a.design".to_string()]) else {
        panic!("{kind:?}: the edit changed nothing");
    };
    let resolved = t.elapsed();
    let t = std::time::Instant::now();
    d.update(&mut r, res.scene(), &applied).unwrap();
    let updated = t.elapsed();
    let fresh = r.drawing(res.scene(), Theme::Light, None).unwrap();
    (resolved, updated, d.visited, d, fresh, r)
}

/// The numbers: `cargo test --release -p devpage-design frame_edit_costs --
/// --ignored --nocapture`.
#[test]
#[ignore]
fn frame_edit_costs() {
    let only = std::env::var("DEVPAGE_KINDS").unwrap_or_default();
    for kind in KINDS.into_iter().chain([Kind::Align]) {
        if !only.is_empty() && !only.contains(&format!("{kind:?}")) {
            continue;
        }
        for n in [1_000, 10_000, 100_000] {
            let (resolved, updated, visited, ..) = edit(kind, n);
            eprintln!(
                "{kind:?} {n:>7}: resolve again {resolved:?}, update {updated:?} ({visited} visited)"
            );
        }
    }
}

/// Every kind of edit visits the same nodes among 1,000 siblings as among
/// 10,000 — none of the siblings beside what it changed — and leaves a
/// drawing that lays out, paints and hit-tests as a fresh one.
#[test]
fn an_edit_costs_what_it_changed_in_every_kind_of_frame() {
    for kind in KINDS {
        let mut seen = None;
        for n in [1_000, 10_000] {
            let (_, _, visited, mut d, fresh, mut r) = edit(kind, n);
            assert!(visited <= 3, "{kind:?} at {n}: {visited} visited");
            if let Some(v) = seen {
                assert_eq!(visited, v, "{kind:?}: visits grow with the siblings");
            }
            seen = Some(visited);
            let scene = {
                let mut saves = crate::ids::testing::Saves::default();
                saves.save("a.design", source(kind, n, false));
                let _ = saves.resolve("a.design");
                saves.save("a.design", source(kind, n, true));
                saves.resolve("a.design")
            };
            d.prepare_all(&mut r, &scene);
            assert_eq!(d.area(), fresh.area(), "{kind:?} at {n}");
            assert_eq!(d.frames(), fresh.frames(), "{kind:?} at {n}");
            let rows = (fresh.area().height as u32).div_ceil(crate::tiles::TILE);
            for row in [0, rows / 2, rows - 1] {
                assert!(
                    crate::cpu::tile(&d.list, 1.0, 0, row)
                        == crate::cpu::tile(&fresh.list, 1.0, 0, row),
                    "{kind:?} at {n}: tile row {row}"
                );
            }
        }
    }
}

/// A frame with no layout that hugs a wrapping text: a sibling that widens
/// it lays the text out again against the wider box, and the drawing shows
/// the text as a fresh one does.
#[test]
fn a_frame_that_grows_lays_out_again_what_its_box_reaches() {
    let design = |w: u32| {
        format!(
            "<Frame layout=\"column\"><Frame><Rect x={{0}} y={{0}} width={{{w}}} height={{10}} /><Frame x={{0}} y={{20}} layout=\"column\"><Text width=\"fill\">a few words that wrap at the frame</Text></Frame></Frame></Frame>"
        )
    };
    let mut saves = crate::ids::testing::Saves::default();
    saves.save("a.design", design(60));
    let mut res = Resolution::new(&saves, "a.design").unwrap();
    let mut r = Renderer::new();
    let mut d = r.drawing(res.scene(), Theme::Light, None).unwrap();
    saves.save("a.design", design(300));
    let Outcome::Changed(_, applied) = res.update(&saves, &["a.design".to_string()]) else {
        panic!("the edit changed nothing");
    };
    d.update(&mut r, res.scene(), &applied).unwrap();
    d.prepare_all(&mut r, res.scene());
    let fresh = r.drawing(res.scene(), Theme::Light, None).unwrap();
    assert_eq!(d.frames(), fresh.frames());
}

/// A list's new `align` moves every row that doesn't fill its line: each is
/// placed again, its contents — in its own coordinates — are not visited,
/// and the drawing equals a fresh one in its boxes and its pixels.
#[test]
fn a_new_align_places_every_row_again_and_nothing_in_them() {
    for n in [1_000, 10_000] {
        let (_, _, visited, mut d, fresh, mut r) = edit(Kind::Align, n);
        assert!(visited <= n + 3, "at {n}: {visited} visited");
        let scene = {
            let mut saves = crate::ids::testing::Saves::default();
            saves.save("a.design", source(Kind::Align, n, false));
            let _ = saves.resolve("a.design");
            saves.save("a.design", source(Kind::Align, n, true));
            saves.resolve("a.design")
        };
        d.prepare_all(&mut r, &scene);
        assert_eq!(d.area(), fresh.area(), "at {n}");
        assert_eq!(d.frames(), fresh.frames(), "at {n}");
        let rows = (fresh.area().height as u32).div_ceil(crate::tiles::TILE);
        for row in 0..rows.min(40) {
            assert!(
                crate::cpu::tile(&d.list, 1.0, 0, row)
                    == crate::cpu::tile(&fresh.list, 1.0, 0, row),
                "at {n}: tile row {row}"
            );
        }
    }
}

/// Every attribute of a list that places its children — `align`,
/// `justify`, `padding`, `gap`, its direction — changed on 1,000 rows of
/// three widths in a fixed box, each from every other value: the drawing
/// equals a fresh one in its boxes and its pixels.
#[test]
fn every_attribute_that_places_a_lists_children_redraws_as_a_fresh_drawing() {
    let attrs: &[&[&str]] = &[
        &["", " align=\"center\"", " align=\"end\""],
        &[
            "",
            " justify=\"center\"",
            " justify=\"end\"",
            " justify=\"space-between\"",
        ],
        &["", " padding={7}", " padding={[3, 11]}"],
        &["", " gap={5}"],
        &[" layout=\"column\"", " layout=\"row\""],
    ];
    let design = |extra: &str, layout: &str| {
        let rows: String = (0..1_000)
            .map(|i| {
                format!(
                    "<Frame layout=\"row\" padding={{2}}><Text>{}</Text></Frame>",
                    "Row ".repeat(1 + i % 3)
                )
            })
            .collect();
        format!("<Frame{layout} width={{60000}} height={{60000}}{extra}>{rows}</Frame>")
    };
    for values in attrs {
        for a in values.iter() {
            for b in values.iter().filter(|b| *b != a) {
                let (from, to, layout) = if values[0].contains("layout") {
                    ("", "", (*a, *b))
                } else {
                    (*a, *b, (" layout=\"column\"", " layout=\"column\""))
                };
                let mut saves = crate::ids::testing::Saves::default();
                saves.save("a.design", design(from, layout.0));
                let mut res = Resolution::new(&saves, "a.design").unwrap();
                let mut r = Renderer::new();
                let mut d = r.drawing(res.scene(), Theme::Light, None).unwrap();
                saves.save("a.design", design(to, layout.1));
                let Outcome::Changed(_, applied) = res.update(&saves, &["a.design".to_string()])
                else {
                    panic!("{a} → {b}: nothing changed");
                };
                d.update(&mut r, res.scene(), &applied).unwrap();
                d.prepare_all(&mut r, res.scene());
                let fresh = r.drawing(res.scene(), Theme::Light, None).unwrap();
                assert_eq!(d.frames(), fresh.frames(), "{a} → {b}");
                for (col, row) in [(0, 0), (0, 2), (1, 1), (3, 3)] {
                    assert!(
                        crate::cpu::tile(&d.list, 1.0, col, row)
                            == crate::cpu::tile(&fresh.list, 1.0, col, row),
                        "{a} → {b}: tile {col},{row}"
                    );
                }
            }
        }
    }
}

/// How an edit's update grows with the siblings beside it, as the median of
/// 20 alternating saves at 10,000, 100,000 and 200,000: `cargo test --release
/// -p devpage-design edit_scaling -- --ignored --nocapture`. `DEVPAGE_LOOP`
/// repeats the largest, to profile it.
#[test]
#[ignore]
fn edit_scaling() {
    let row =
        |i: usize| format!("<Frame layout=\"row\" padding={{4}}><Text>Row {i}</Text></Frame>");
    type Source = Box<dyn Fn(usize, bool) -> String>;
    let cases: Vec<(&str, Source)> = vec![
        (
            "a middle row deleted",
            Box::new(move |n, e| {
                let rows: String = (0..n)
                    .map(|i| {
                        if e && i == n / 2 {
                            String::new()
                        } else {
                            row(i)
                        }
                    })
                    .collect();
                format!("<Frame layout=\"column\">{rows}</Frame>")
            }),
        ),
        (
            "the widest row made narrower",
            Box::new(move |n, e| {
                let rows: String = (0..n)
                    .map(|i| {
                        if i == n / 2 {
                            format!(
                                "<Frame layout=\"row\" padding={{4}}><Text>{}</Text></Frame>",
                                if e {
                                    "x"
                                } else {
                                    "The widest row of them all by far"
                                }
                            )
                        } else {
                            row(i)
                        }
                    })
                    .collect();
                format!("<Frame layout=\"column\">{rows}</Frame>")
            }),
        ),
        (
            "a child of a frame with no layout resized",
            Box::new(|n, e| source(Kind::NoLayoutResize, n, e)),
        ),
    ];
    let repeat: usize = std::env::var("DEVPAGE_LOOP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let only = std::env::var("DEVPAGE_CASE").unwrap_or_default();
    for (name, src) in cases
        .iter()
        .filter(|(name, _)| name.contains(only.as_str()))
    {
        // A frame with no layout places its 200,000th child past the
        // largest number a design may write: it stops at 100,000.
        let lengths: &[usize] = if name.contains("no layout") {
            &[10_000, 50_000, 100_000]
        } else {
            &[10_000, 100_000, 200_000]
        };
        for &n in lengths {
            let mut saves = crate::ids::testing::Saves::default();
            saves.save("a.design", src(n, false));
            let mut res = Resolution::new(&saves, "a.design").unwrap();
            let mut r = Renderer::new();
            let mut d = r.drawing(res.scene(), Theme::Light, None).unwrap();
            let rounds = if n == *lengths.last().expect("a length") {
                20 + repeat
            } else {
                20
            };
            let mut times = vec![];
            for i in 0..rounds {
                saves.save("a.design", src(n, i % 2 == 0));
                let Outcome::Changed(_, applied) = res.update(&saves, &["a.design".to_string()])
                else {
                    panic!("{name}: nothing changed")
                };
                let t = std::time::Instant::now();
                d.update(&mut r, res.scene(), &applied).unwrap();
                times.push(t.elapsed());
            }
            times.sort();
            eprintln!(
                "{name} {n:>7}: median {:?}, least {:?} ({} visited)",
                times[times.len() / 2],
                times[0],
                d.visited
            );
        }
    }
}

/// One update and its undo, applied to the same drawing over and over with
/// nothing between, to profile: `DEVPAGE_LOOP` rounds of a middle row
/// deleted and put back among 200,000.
#[test]
#[ignore]
fn profile_update() {
    let n = 200_000;
    let src = |e: bool| {
        let rows: String = (0..n)
            .map(|i| {
                if e && i == n / 2 {
                    String::new()
                } else {
                    format!("<Frame layout=\"row\" padding={{4}}><Text>Row {i}</Text></Frame>")
                }
            })
            .collect();
        format!("<Frame layout=\"column\">{rows}</Frame>")
    };
    let mut saves = crate::ids::testing::Saves::default();
    saves.save("a.design", src(false));
    let mut res = Resolution::new(&saves, "a.design").unwrap();
    let a = res.scene().clone();
    saves.save("a.design", src(true));
    let Outcome::Changed(_, there) = res.update(&saves, &["a.design".to_string()]) else {
        panic!()
    };
    let b = res.scene().clone();
    saves.save("a.design", src(false));
    let Outcome::Changed(_, back) = res.update(&saves, &["a.design".to_string()]) else {
        panic!()
    };
    let a2 = res.scene().clone();
    let mut r = Renderer::new();
    let mut d = r.drawing(&a, Theme::Light, None).unwrap();
    let rounds: usize = std::env::var("DEVPAGE_LOOP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    eprintln!("drawn");
    let t = std::time::Instant::now();
    for _ in 0..rounds {
        d.update(&mut r, &b, &there).unwrap();
        d.update(&mut r, &a2, &back).unwrap();
    }
    eprintln!("{:?} a round", t.elapsed() / rounds as u32);
}
