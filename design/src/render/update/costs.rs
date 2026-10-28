//! What an edit costs by the kind of frame it is in: a list's widest row
//! made wider, a list's own fill changed, a child of a frame with no layout
//! resized or inserted first, a sibling of a `"fill"` child resized, and a
//! child of a `"space-between"` frame resized — each at 1,000, 10,000 and
//! 100,000 siblings, the save resolved again and the drawing brought across.

use crate::render::Drawing;
use crate::resolve::Theme;
use crate::resolve::{Outcome, Resolution};

use super::*;

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
        Kind::NoLayoutResize | Kind::NoLayoutFront => "<Frame>".to_string(),
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
}

pub(super) const KINDS: [Kind; 6] = [
    Kind::Widest,
    Kind::ParentFill,
    Kind::NoLayoutResize,
    Kind::NoLayoutFront,
    Kind::FillSibling,
    Kind::SpaceBetween,
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
    for kind in KINDS {
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
