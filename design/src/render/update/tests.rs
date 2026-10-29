use crate::cpu::*;
use std::collections::HashMap;

use crate::render::testing::*;
use crate::scene::{self, Layout, Node, Scene, Theme};

use super::*;

/// An edit to one row of a tall design touches only the tiles around it,
/// and the drawing it leaves paints as a fresh one.
#[test]
fn an_edit_to_one_node_damages_only_the_tiles_around_it() {
    let design = |label: &str| {
        let mut rows = String::new();
        for i in 0..1000 {
            let text = if i == 500 {
                label.to_string()
            } else {
                format!("Row {i}")
            };
            rows.push_str(&format!("<Frame width={{400}} height={{100}} fill=\"#eeeeee\"><Text size={{20}}>{text}</Text></Frame>\n"));
        }
        let files: HashMap<String, String> = [(
            "a.design".to_string(),
            format!("<Frame layout=\"column\" fill=\"#ffffff\">{rows}</Frame>"),
        )]
        .into();
        crate::resolve(&files, "a.design").unwrap()
    };
    let (old, new) = (design("Row 500"), design("Edited"));
    let mut r = Renderer::new();
    let mut live = r.drawing(&old, Theme::Light, None).unwrap();
    let mut held = old.clone();
    let mut uses = crate::scene::Uses::of(&held.root);
    let t = std::time::Instant::now();
    let applied = {
        let edits = crate::scene::Edits::between(&held, &new);
        crate::scene::apply(&mut held, &mut uses, &edits)
    }
    .unwrap();
    let damage = live.update(&mut r, &held, &applied).unwrap();
    eprintln!("update {:?}, damage {damage:?}", t.elapsed());
    assert!(!damage.all && !damage.rects.is_empty());
    for d in &damage.rects {
        assert!(d.y0 >= 50_000.0 - 21.0 && d.y1 <= 50_100.0 + 21.0, "{d:?}");
    }
    let fresh = r.drawing(&new, Theme::Light, None).unwrap();
    for row in [0, 195, 196, 390] {
        let a = crate::cpu::tile(&live.list, 1.0, 0, row);
        let b = crate::cpu::tile(&fresh.list, 1.0, 0, row);
        assert!(a == b, "tile row {row}");
    }
}

/// An update paints as a fresh drawing of the new scene does: a row
/// growing moves those after it, a clipping parent resizing keeps its
/// children under its clip, and a row added grows the area along its
/// bottom — damage there, not everywhere; a child past the top-left
/// moves the area's corner, which damages every tile.
#[test]
fn an_update_paints_as_a_fresh_drawing() {
    let design = |tall: u32, clip: u32, rows: usize, above: bool| {
        let mut body = String::new();
        for i in 0..rows {
            let h = if i == 3 { tall } else { 40 };
            body.push_str(&format!(
                "<Frame width={{200}} height={{{h}}} fill=\"#{:06x}\" layout=\"none\"><Frame width={{{clip}}} height={{30}} clip shadow={{{{ blur: 6, color: \"#00000080\" }}}}><Rect x={{20}} width={{100}} height={{30}} fill=\"#ff0000\" /></Frame></Frame>",
                i * 40503 % 0xffffff
            ));
        }
        let extra = if above {
            "<Rect x={-30} y={-30} width={10} height={10} fill=\"#00ff00\" />"
        } else {
            ""
        };
        let files: HashMap<String, String> = [(
            "a.design".to_string(),
            format!("<Frame layout=\"none\" fill=\"#ffffff\"><Frame layout=\"column\">{body}</Frame>{extra}</Frame>"),
        )]
        .into();
        crate::resolve(&files, "a.design").unwrap()
    };
    let mut r = Renderer::new();
    let paint = |d: &Drawing| {
        paint_area(&d.list, frame_rect(d.area()), 1.0)
            .data_as_u8_slice()
            .to_vec()
    };
    let steps = [
        design(40, 60, 8, false),
        design(90, 60, 8, false),
        design(90, 150, 8, false),
        design(90, 150, 9, false),
        design(90, 150, 9, true),
        design(40, 60, 6, false),
    ];
    let mut live = r.drawing(&steps[0], Theme::Light, None).unwrap();
    let mut held = steps[0].clone();
    let mut uses = crate::scene::Uses::of(&held.root);
    for (n, pair) in steps.windows(2).enumerate() {
        let applied = {
            let edits = crate::scene::Edits::between(&held, &pair[1]);
            crate::scene::apply(&mut held, &mut uses, &edits)
        }
        .unwrap();
        let damage = live.update(&mut r, &held, &applied).unwrap();
        let fresh = r.drawing(&pair[1], Theme::Light, None).unwrap();
        assert_eq!(live.area(), fresh.area(), "step {n}");
        assert_eq!(live.frames(), fresh.frames(), "step {n}");
        assert!(paint(&live) == paint(&fresh), "step {n}");
        match n {
            2 => {
                assert!(!damage.all);
                let bottom = live.area().y as f64 + live.area().height as f64;
                assert!(
                    damage.rects.iter().any(|d| d.y1 >= bottom - 1.0),
                    "{damage:?}"
                );
            }
            3 => assert!(damage.all, "{damage:?}"),
            _ => {}
        }
    }
}

/// A design of `rows` rows, each a frame with a text; row `edit` says
/// `label`.
fn tall(rows: usize, edit: usize, label: &str) -> Scene {
    let mut body = String::new();
    for i in 0..rows {
        let text = if i == edit {
            label.to_string()
        } else {
            format!("Row {i:05}")
        };
        body.push_str(&format!(
            "<Frame width={{400}} height={{100}} fill=\"#eeeeee\" layout=\"row\" padding={{10}}><Text size={{20}}>{text}</Text></Frame>\n"
        ));
    }
    let files: HashMap<String, String> = [(
        "a.design".to_string(),
        format!("<Frame layout=\"column\" fill=\"#ffffff\">{body}</Frame>"),
    )]
    .into();
    crate::resolve(&files, "a.design").unwrap()
}

/// The fastest of several updates of row `rows / 2`'s text, back and
/// forth, on a design of `rows` rows, and how many nodes it visited
/// after laying out.
fn edit_cost(rows: usize) -> (std::time::Duration, usize) {
    let (a, b) = (
        tall(rows, rows / 2, "Row 00000"),
        tall(rows, rows / 2, "Edited!!!"),
    );
    let mut r = Renderer::new();
    let mut d = r.drawing(&a, Theme::Light, None).unwrap();
    let mut held = a.clone();
    let mut uses = crate::scene::Uses::of(&held.root);
    let mut best = std::time::Duration::MAX;
    for n in 0..12 {
        let new = if n % 2 == 0 { &b } else { &a };
        let t = std::time::Instant::now();
        let applied = {
            let edits = crate::scene::Edits::between(&held, new);
            crate::scene::apply(&mut held, &mut uses, &edits)
        }
        .unwrap();
        let damage = d.update(&mut r, &held, &applied).unwrap();
        best = best.min(t.elapsed());
        assert!(!damage.all && damage.rects.len() <= 4, "{damage:?}");
    }
    (best, d.visited)
}

/// Updating one row's text visits only the nodes on its path — the
/// same few on a design ten times the size — so making chunks, moving
/// index entries and marking damage don't grow with the design. What
/// still does, on a flat list: taffy laying out the row's parent again over
/// all its children.
#[test]
fn an_edit_visits_only_its_path() {
    edit_cost(100);
    let ((small, a), (big, b)) = (edit_cost(1_000), edit_cost(10_000));
    eprintln!(
        "one row's text: {small:?} on 1,000 rows, {big:?} on 10,000; {a} and {b} nodes visited"
    );
    assert_eq!((a, b), (3, 3));
}

/// A row that grows moves the rows after it, and only those, and the
/// drawing paints as a fresh one.
#[test]
fn a_row_that_grows_moves_the_rows_after_it() {
    let a = tall(50, 20, "Row 00020");
    let mut b = a.clone();
    b.root.children.nth_mut(20).unwrap().height = scene::Size::Px(160.0);
    let mut r = Renderer::new();
    let mut d = r.drawing(&a, Theme::Light, None).unwrap();
    let mut held = a.clone();
    let mut uses = crate::scene::Uses::of(&held.root);
    let applied = {
        let edits = crate::scene::Edits::between(&held, &b);
        crate::scene::apply(&mut held, &mut uses, &edits)
    }
    .unwrap();
    let damage = d.update(&mut r, &held, &applied).unwrap();
    assert!(!damage.all);
    assert!(
        damage.rects.iter().all(|r| r.y1 > 2000.0 - 1.0),
        "{damage:?}"
    );
    let fresh = r.drawing(&b, Theme::Light, None).unwrap();
    assert_eq!(d.frames(), fresh.frames());
    assert_eq!(d.area(), fresh.area());
    for row in 0..d.area().height as u32 / 256 + 1 {
        assert!(
            crate::cpu::tile(&d.list, 1.0, 0, row) == crate::cpu::tile(&fresh.list, 1.0, 0, row),
            "row {row}"
        );
    }
}

/// A small deterministic random number source.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// Every node of `n`'s subtree, by its path of child positions.
fn paths(n: &Node, at: Vec<usize>, out: &mut Vec<Vec<usize>>) {
    out.push(at.clone());
    for (i, c) in n.children.iter().enumerate() {
        let mut p = at.clone();
        p.push(i);
        paths(c, p, out);
    }
}

/// Gives `n` and every node under it ids no other node has.
fn renumber(n: &mut Node, next: &mut u32) {
    n.id = crate::tree::NodeId(vec![*next]);
    *next += 1;
    for c in n.children.iter_mut() {
        renumber(c, next);
    }
}

fn node_at<'a>(root: &'a mut Node, path: &[usize]) -> &'a mut Node {
    let mut at = root;
    for &i in path {
        at = at.children.nth_mut(i).unwrap();
    }
    at
}

/// Edits of every kind to a real design — attributes, text, sizes,
/// layouts, clips and opacities, children added, removed, inserted at
/// the front and replaced, the root replaced — each leave a drawing that
/// lays out, paints and holds property-tree nodes as a fresh drawing of
/// the same scene does.
#[test]
fn random_edits_leave_a_drawing_as_a_fresh_one() {
    const SEED: u64 = 7;
    const STEPS: usize = 120;
    let path = "docs/designs/checkout/checkout.design";
    let start = crate::resolve(&repo(), path).unwrap();
    let mut r = Renderer::new();
    let mut rng = Lcg(SEED);
    let mut fresh_ids = 1_000_000;
    for theme in [Theme::Light, Theme::Dark] {
        let mut scene = start.clone();
        // The canvas's side: the scene as the stream carries it, patched,
        // and the drawing made from it, whose places are its.
        let mut held = crate::wire::decode_scene(
            &crate::wire::scene(&scene),
            (&Default::default(), &Default::default()),
        )
        .unwrap();
        let mut d = r.drawing(&held, theme, None).unwrap();
        let mut uses = crate::scene::Uses::of(&held.root);
        for step in 0..STEPS {
            let mut next = scene.clone();
            let mut all = Vec::new();
            paths(&next.root, Vec::new(), &mut all);
            let at = all[rng.next(all.len())].clone();
            let kind = rng.next(10);
            {
                let n = node_at(&mut next.root, &at);
                match kind {
                    0 => n.opacity = if n.opacity < 1.0 { 1.0 } else { 0.5 },
                    1 => n.clip = !n.clip,
                    2 => n.width = scene::Size::Px(40.0 + rng.next(300) as f32),
                    3 => n.x += 7.0,
                    4 => {
                        if let Some(t) = &mut n.text {
                            t.content.push_str(" more words");
                        } else {
                            n.gap += 3.0;
                        }
                    }
                    5 if !n.children.is_empty() => {
                        let last = n.children.last_key().cloned().unwrap();
                        n.children.remove(&last);
                    }
                    6 if !n.children.is_empty() => {
                        // A copy is another node, with ids of its own.
                        let mut c = n.children[0].clone();
                        renumber(&mut c, &mut fresh_ids);
                        n.children.push(c);
                    }
                    7 if n.children.len() > 1 => {
                        let last = n.children.last_key().cloned().unwrap();
                        let c = n.children.remove(&last).unwrap();
                        n.children.insert_after(None, c);
                    }
                    8 => {
                        n.layout = match n.layout {
                            Layout::Row => Layout::Column,
                            Layout::Column => Layout::None,
                            Layout::None => Layout::Row,
                        }
                    }
                    _ => n.padding = [3.0, 5.0, 7.0, 9.0],
                }
            }
            if step == STEPS / 2 {
                next.root = start.root.clone();
            }
            // Only what the resolver lets through: no `"fill"` on a
            // child of a frame with no layout.
            fn valid(n: &Node) -> bool {
                let fills =
                    |c: &Node| c.width == scene::Size::Fill || c.height == scene::Size::Fill;
                !(n.layout == Layout::None && n.children.iter().any(fills))
                    && n.children.iter().all(valid)
            }
            if !valid(&next.root) {
                continue;
            }
            // Edited by hand, so indexed again.
            next.ids = crate::scene::Ids::of(&next.root);
            // By the very patch the canvas gets, applied as the canvas
            // applies it.
            let patch = crate::wire::patch(&crate::scene::Edits::between(&scene, &next));
            let replaced = crate::wire::apply(&mut held, &mut uses, &patch).unwrap_or_else(|e| {
                panic!(
                    "{theme:?} step {step} kind {kind} at {at:?}: {e}\n{:?}",
                    patch
                        .ops
                        .iter()
                        .map(|o| format!(
                            "{:?}",
                            o.op.as_ref().map(|o| match o {
                                crate::proto::op::Op::Change(n) => format!("change {:?}", n.id),
                                crate::proto::op::Op::Insert(i) => format!(
                                    "insert {:?} after {:?} under {:?}",
                                    i.node.as_ref().unwrap().id,
                                    i.after,
                                    i.parent
                                ),
                                crate::proto::op::Op::Remove(r) => format!("remove {r:?}"),
                                crate::proto::op::Op::Move(m) => format!("move {:?}", m.id),
                            })
                        ))
                        .collect::<Vec<_>>()
                )
            });
            assert!(held.root == next.root, "the patch carries the scene");
            assert_eq!(uses, crate::scene::Uses::of(&held.root));
            d.update(&mut r, &held, &replaced).unwrap();
            // What the update moved by a fraction of a pixel rounds when read.
            d.prepare_all(&mut r, &held);
            let fresh = r.drawing(&next, theme, None).unwrap();
            let case = format!("{theme:?} step {step}: kind {kind} at {at:?}");
            assert_eq!(d.frames(), fresh.frames(), "{case}");
            assert_eq!(d.area(), fresh.area(), "{case}");
            let paint = |d: &Drawing| {
                paint_area(&d.list, frame_rect(d.area()), 1.0)
                    .data_as_u8_slice()
                    .to_vec()
            };
            assert!(paint(&d) == paint(&fresh), "{case}");
            let live = |d: &Drawing| {
                let t = &d.list.trees;
                (
                    t.transforms.live(),
                    t.clips.live(),
                    t.effects.live(),
                    d.images.len(),
                )
            };
            assert_eq!(live(&d), live(&fresh), "{case}");
            scene = next;
        }
    }
}

/// A balanced tree, ten children a frame, `depth` levels of frames above
/// its rows, alternately rows and columns; row `edit` says `label`.
fn balanced(depth: u32, edit: usize, label: &str) -> Scene {
    fn go(depth: u32, base: usize, edit: usize, label: &str, out: &mut String) {
        if depth == 0 {
            let text = if base == edit {
                label.to_string()
            } else {
                format!("Row {base:06}")
            };
            out.push_str(&format!(
                "<Frame width={{400}} height={{40}} fill=\"#eeeeee\" layout=\"row\"><Text size={{20}}>{text}</Text></Frame>"
            ));
            return;
        }
        let dir = if depth.is_multiple_of(2) {
            "column"
        } else {
            "row"
        };
        out.push_str(&format!("<Frame layout=\"{dir}\">"));
        let span = 10usize.pow(depth - 1);
        for i in 0..10 {
            go(depth - 1, base + i * span, edit, label, out);
        }
        out.push_str("</Frame>");
    }
    let mut s = String::new();
    go(depth, 0, edit, label, &mut s);
    let files: HashMap<String, String> = [("a.design".to_string(), s)].into();
    crate::resolve(&files, "a.design").unwrap()
}

/// The fastest of several edits of one row's text, back and forth, on a
/// balanced design of `10^depth` rows, applied as a canvas applies them: the
/// patch onto the scene it holds, then the drawing by what it replaced.
fn patch_cost(depth: u32) -> std::time::Duration {
    let n = 10usize.pow(depth);
    let a = balanced(depth, n / 2, &format!("Row {:06}", n / 2));
    let b = balanced(depth, n / 2, "Edited!!!!");
    let (there, back) = (
        crate::wire::patch(&crate::scene::Edits::between(&a, &b)),
        crate::wire::patch(&crate::scene::Edits::between(&b, &a)),
    );
    let mut held = crate::wire::decode_scene(
        &crate::wire::scene(&a),
        (&Default::default(), &Default::default()),
    )
    .unwrap();
    let mut uses = crate::scene::Uses::of(&held.root);
    let mut r = Renderer::new();
    let mut d = r.drawing(&held, Theme::Light, None).unwrap();
    let mut best = std::time::Duration::MAX;
    for k in 0..10 {
        let p = if k % 2 == 0 { &there } else { &back };
        let t = std::time::Instant::now();
        let replaced = crate::wire::apply(&mut held, &mut uses, p).unwrap();
        let damage = d.update(&mut r, &held, &replaced).unwrap();
        best = best.min(t.elapsed());
        assert!(!damage.all && damage.rects.len() <= 4, "{damage:?}");
    }
    best
}

/// An edit costs the patch, not the design: on a balanced design, one row's
/// text costs about the same on 10,000 rows as on 100 — the path to it
/// grows by two levels, nothing walks the rest.
#[test]
fn an_edit_costs_the_patch_not_the_design() {
    patch_cost(2);
    let (small, big) = (patch_cost(2), patch_cost(4));
    eprintln!("one row's text by its patch: {small:?} on 100 rows, {big:?} on 10,000");
    assert!(
        big < small * 4,
        "{big:?} on 10,000 rows against {small:?} on 100"
    );
}

/// A design of `n` rows in a frame with no layout, as saved: with `front`,
/// one more before the first.
fn rows_saved(saves: &mut crate::ids::testing::Saves, n: usize, front: bool) -> Scene {
    let mut rows = String::new();
    if front {
        rows.push_str("<Frame width={400} height={10} fill=\"#ff0000\" />");
    }
    for i in 0..n {
        rows.push_str(&format!(
            "<Frame y={{{}}} width={{400}} height={{10}} fill=\"#{:06x}\" />",
            i * 10,
            i * 7919 % 0xffffff
        ));
    }
    saves.save(
        "a.design",
        format!("<Frame layout=\"none\" fill=\"#ffffff\">{rows}</Frame>"),
    );
    saves.resolve("a.design")
}

/// A row inserted before the first of 1,000 or 10,000 is one insert in the
/// patch and one chunk made: the drawing visits the new row and its parent,
/// whatever the number of siblings, and paints as a fresh drawing does.
#[test]
fn an_insert_before_the_first_is_one_insert_and_one_chunk() {
    for n in [1_000, 10_000] {
        let mut saves = crate::ids::testing::Saves::default();
        let old = rows_saved(&mut saves, n, false);
        let new = rows_saved(&mut saves, n, true);
        let p = crate::wire::patch(&crate::scene::Edits::between(&old, &new));
        assert_eq!(p.ops.len(), 1, "{n}");
        let mut held = crate::wire::decode_scene(
            &crate::wire::scene(&old),
            (&Default::default(), &Default::default()),
        )
        .unwrap();
        let mut uses = crate::scene::Uses::of(&held.root);
        let mut r = Renderer::new();
        let mut d = r.drawing(&held, Theme::Light, None).unwrap();
        let applied = crate::wire::apply(&mut held, &mut uses, &p).unwrap();
        d.update(&mut r, &held, &applied).unwrap();
        assert_eq!(d.visited, 2, "{n}");
        let fresh = r.drawing(&new, Theme::Light, None).unwrap();
        assert_eq!(d.frames(), fresh.frames());
        for row in [0, 1, 2] {
            assert!(
                crate::cpu::tile(&d.list, 1.0, 0, row)
                    == crate::cpu::tile(&fresh.list, 1.0, 0, row),
                "{n} row {row}"
            );
        }
    }
}

/// Random saves of a list — rows inserted at the front, middle and end,
/// removed, moved, resized, keyed and not — each resolved from the saved
/// text, patched onto the canvas's scene and its drawing: the drawing paints
/// as a fresh one, and every row the save didn't touch keeps its node's id.
#[test]
fn random_saves_keep_ids_and_leave_a_drawing_as_a_fresh_one() {
    #[derive(Clone)]
    struct Row {
        label: String,
        keyed: bool,
        width: u32,
    }
    let text = |rows: &[Row]| {
        let body: String = rows
            .iter()
            .map(|r| {
                let key = if r.keyed {
                    format!(r#" key="{}""#, r.label)
                } else {
                    String::new()
                };
                format!(
                    r##"<Frame name="{l}"{key} width={{{w}}} height={{20}} fill="#88aacc"><Text size={{12}}>{l}</Text></Frame>"##,
                    l = r.label,
                    w = r.width
                )
            })
            .collect();
        format!(r##"<Frame layout="column" gap={{2}} fill="#ffffff">{body}</Frame>"##)
    };
    let ids = |s: &Scene| -> HashMap<String, crate::tree::NodeId> {
        s.root
            .children
            .iter()
            .map(|c| (c.name.clone(), c.id.clone()))
            .collect()
    };
    let mut rng = Lcg(11);
    let mut rows: Vec<Row> = (0..12)
        .map(|i| Row {
            label: format!("r{i}"),
            keyed: i % 3 == 0,
            width: 100,
        })
        .collect();
    let mut saves = crate::ids::testing::Saves::default();
    saves.save("a.design", text(&rows));
    let mut sent = saves.resolve("a.design");
    let mut held = crate::wire::decode_scene(
        &crate::wire::scene(&sent),
        (&Default::default(), &Default::default()),
    )
    .unwrap();
    let mut uses = crate::scene::Uses::of(&held.root);
    let mut r = Renderer::new();
    let mut d = r.drawing(&held, Theme::Light, None).unwrap();
    let mut next_label = 100;
    for step in 0..80 {
        let mut touched: Vec<String> = Vec::new();
        let at = rng.next(rows.len() + 1);
        match rng.next(5) {
            0 => {
                let place = [0, rows.len() / 2, rows.len()][rng.next(3)];
                let label = format!("n{next_label}");
                next_label += 1;
                touched.push(label.clone());
                rows.insert(
                    place,
                    Row {
                        label,
                        keyed: rng.next(2) == 0,
                        width: 80,
                    },
                );
            }
            1 if rows.len() > 2 => {
                let gone = rows.remove(at.min(rows.len() - 1));
                touched.push(gone.label);
            }
            2 if rows.len() > 2 => {
                let row = rows.remove(at.min(rows.len() - 1));
                touched.push(row.label.clone());
                let to = rng.next(rows.len() + 1);
                rows.insert(to, row);
            }
            _ if !rows.is_empty() => {
                let i = at.min(rows.len() - 1);
                rows[i].width = 60 + rng.next(200) as u32;
                touched.push(rows[i].label.clone());
            }
            _ => continue,
        }
        let before = ids(&sent);
        saves.save("a.design", text(&rows));
        let next = saves.resolve("a.design");
        let after = ids(&next);
        for (label, id) in &before {
            if !touched.contains(label) {
                assert_eq!(
                    after.get(label),
                    Some(id),
                    "step {step}: {label} kept its id"
                );
            }
        }
        // A keyed row moved keeps its id too.
        for row in rows
            .iter()
            .filter(|r| r.keyed && touched.contains(&r.label))
        {
            if let Some(id) = before.get(&row.label) {
                assert_eq!(
                    after.get(&row.label),
                    Some(id),
                    "step {step}: keyed {}",
                    row.label
                );
            }
        }
        let p = crate::wire::patch(&crate::scene::Edits::between(&sent, &next));
        assert!(p.ops.len() <= 4, "step {step}: {} operations", p.ops.len());
        let applied = crate::wire::apply(&mut held, &mut uses, &p).unwrap();
        assert!(held.root == next.root, "step {step}");
        d.update(&mut r, &held, &applied).unwrap();
        let fresh = r.drawing(&next, Theme::Light, None).unwrap();
        assert_eq!(d.frames(), fresh.frames(), "step {step}");
        let paint = |d: &Drawing| {
            paint_area(&d.list, frame_rect(d.area()), 1.0)
                .data_as_u8_slice()
                .to_vec()
        };
        assert!(paint(&d) == paint(&fresh), "step {step}");
        sent = next;
    }
}

/// A column of `n` rows, each `heights(i)` tall with a line of text in it,
/// saved through `saves`, and the row at `edit` given `label`, the row at
/// `grow` 8 px taller, and a row before the first when `front`.
fn column_saved(
    saves: &mut crate::ids::testing::Saves,
    n: usize,
    label: (usize, &str),
    grow: Option<usize>,
    front: bool,
    fractional: bool,
) -> Scene {
    let mut rows = String::new();
    let height = |i: usize| {
        let h = if Some(i) == grow { 28 } else { 20 };
        if fractional {
            String::new()
        } else {
            format!(" height={{{h}}}")
        }
    };
    if front {
        rows.push_str(&format!(
            "<Frame width={{400}}{}><Text>first</Text></Frame>",
            height(usize::MAX)
        ));
    }
    for i in 0..n {
        let text = if i == label.0 {
            label.1.to_string()
        } else {
            format!("Row {i}")
        };
        let pad = if Some(i) == grow && fractional {
            " padding={4}"
        } else {
            ""
        };
        rows.push_str(&format!(
            "<Frame width={{400}}{}{pad}><Text>{text}</Text></Frame>",
            height(i)
        ));
    }
    saves.save(
        "a.design",
        format!("<Frame layout=\"column\" fill=\"#ffffff\">{rows}</Frame>"),
    );
    saves.resolve("a.design")
}

/// What one edit to a column of `n` rows costs the drawing, in release:
/// the update's time, and how many nodes it visited.
fn column_edit(
    n: usize,
    edit: &dyn Fn(&mut crate::ids::testing::Saves) -> Scene,
) -> (std::time::Duration, usize) {
    let mut saves = crate::ids::testing::Saves::default();
    let old = column_saved(&mut saves, n, (usize::MAX, ""), None, false, false);
    let old = if edit_fractional() {
        column_saved(&mut saves, n, (usize::MAX, ""), None, false, true)
    } else {
        old
    };
    let new = edit(&mut saves);
    let p = crate::wire::patch(&crate::scene::Edits::between(&old, &new));
    let mut held = crate::wire::decode_scene(
        &crate::wire::scene(&old),
        (&Default::default(), &Default::default()),
    )
    .unwrap();
    let mut uses = crate::scene::Uses::of(&held.root);
    let mut r = Renderer::new();
    let mut d = r.drawing(&held, Theme::Light, None).unwrap();
    let applied = crate::wire::apply(&mut held, &mut uses, &p).unwrap();
    let t = std::time::Instant::now();
    d.update(&mut r, &held, &applied).unwrap();
    let took = t.elapsed();
    (took, d.visited)
}

fn edit_fractional() -> bool {
    std::env::var_os("DEVPAGE_FRACTIONAL").is_some()
}

/// The numbers for a column of 1,000, 10,000 and 100,000 rows: an edit that
/// keeps a row's size, one that grows it, and a row inserted before the
/// first. Run in release: `cargo test --release -p devpage-design
/// column_edit_costs -- --ignored --nocapture`.
#[test]
#[ignore]
fn column_edit_costs() {
    let f = edit_fractional();
    for n in [1_000, 10_000, 100_000] {
        let text = column_edit(n, &|s| {
            column_saved(s, n, (n / 2, "Edited"), None, false, f)
        });
        let grow = column_edit(n, &|s| {
            column_saved(s, n, (usize::MAX, ""), Some(n / 2), false, f)
        });
        let front = column_edit(n, &|s| column_saved(s, n, (usize::MAX, ""), None, true, f));
        eprintln!(
            "{n:>7} rows: text {:?} ({} visited), grow {:?} ({} visited), front {:?} ({} visited)",
            text.0, text.1, grow.0, grow.1, front.0, front.1
        );
    }
}

/// One row of a column edited — its text, its height, a row before the
/// first — visits the same nodes at 1,000 and 10,000 rows: the rows after it
/// move by the run's sums. The drawing paints, hit-tests and finds its
/// view's tiles as a fresh one does, and the damage runs from the edited row
/// to the column's end.
#[test]
fn a_column_edit_visits_the_same_nodes_at_any_length() {
    for n in [1_000, 10_000] {
        let edits: [&dyn Fn(&mut crate::ids::testing::Saves) -> Scene; 3] = [
            &|s| column_saved(s, n, (n / 2, "Edited"), None, false, false),
            &|s| column_saved(s, n, (usize::MAX, ""), Some(n / 2), false, false),
            &|s| column_saved(s, n, (usize::MAX, ""), None, true, false),
        ];
        for (e, edit) in edits.iter().enumerate() {
            let mut saves = crate::ids::testing::Saves::default();
            let old = column_saved(&mut saves, n, (usize::MAX, ""), None, false, false);
            let new = edit(&mut saves);
            let p = crate::wire::patch(&crate::scene::Edits::between(&old, &new));
            let mut held = crate::wire::decode_scene(
                &crate::wire::scene(&old),
                (&Default::default(), &Default::default()),
            )
            .unwrap();
            let mut uses = crate::scene::Uses::of(&held.root);
            let mut r = Renderer::new();
            let mut d = r.drawing(&held, Theme::Light, None).unwrap();
            let applied = crate::wire::apply(&mut held, &mut uses, &p).unwrap();
            let damage = d.update(&mut r, &held, &applied).unwrap();
            assert!(d.visited <= 3, "{n} rows, edit {e}: {} visited", d.visited);
            let fresh = r.drawing(&new, Theme::Light, None).unwrap();
            assert_eq!(d.area(), fresh.area(), "{n} {e}");
            assert_eq!(d.frames(), fresh.frames(), "{n} {e}");
            let edited = if e == 2 { 0.0 } else { (n / 2) as f64 * 20.0 };
            for rect in &damage.rects {
                assert!(
                    rect.y1 >= edited - 1.0,
                    "{n} {e}: damage {rect:?} above the edit"
                );
            }
            let rows = (fresh.area().height as u32).div_ceil(crate::tiles::TILE);
            for row in [0, rows / 2, rows - 1] {
                assert!(
                    crate::cpu::tile(&d.list, 1.0, 0, row)
                        == crate::cpu::tile(&fresh.list, 1.0, 0, row),
                    "{n} {e}: tile row {row}"
                );
            }
            for y in [5.0, edited as f32 + 5.0, fresh.area().height - 5.0] {
                assert_eq!(d.action_at(10.0, y), fresh.action_at(10.0, y));
            }
        }
    }
}

/// A row of fractional height inserted before the first of 10,000 moves
/// every row after it by a fraction of a pixel: the update visits only the
/// new row, and rounding the rest anew costs only what is read — a tile
/// rounds the rows it shows — and leaves a drawing a fresh one's, box for
/// box and pixel for pixel.
#[test]
fn rows_moved_by_a_fraction_round_when_read() {
    let n = 10_000;
    let mut saves = crate::ids::testing::Saves::default();
    let old = column_saved(&mut saves, n, (usize::MAX, ""), None, false, true);
    let new = column_saved(&mut saves, n, (usize::MAX, ""), None, true, true);
    let p = crate::wire::patch(&crate::scene::Edits::between(&old, &new));
    let mut held = crate::wire::decode_scene(
        &crate::wire::scene(&old),
        (&Default::default(), &Default::default()),
    )
    .unwrap();
    let mut uses = crate::scene::Uses::of(&held.root);
    let mut r = Renderer::new();
    let mut d = r.drawing(&held, Theme::Light, None).unwrap();
    let applied = crate::wire::apply(&mut held, &mut uses, &p).unwrap();
    d.update(&mut r, &held, &applied).unwrap();
    assert!(d.visited <= 3, "{} visited", d.visited);
    let fresh = r.drawing(&new, Theme::Light, None).unwrap();
    let rows = (fresh.area().height as u32).div_ceil(crate::tiles::TILE);
    for row in [rows / 2, rows - 1, 0] {
        d.visited = 0;
        let rect = crate::tiles::tile_rect(&d.list, 1.0, 0, row);
        d.prepare(&mut r, &held, rect);
        assert!(d.visited < 64, "tile row {row}: {} visited", d.visited);
        assert!(
            crate::cpu::tile(&d.list, 1.0, 0, row) == crate::cpu::tile(&fresh.list, 1.0, 0, row),
            "tile row {row}"
        );
    }
    d.prepare_all(&mut r, &held);
    assert_eq!(d.frames(), fresh.frames());
    assert_eq!(d.area(), fresh.area());
}
