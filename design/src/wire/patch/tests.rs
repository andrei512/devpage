use super::*;
use crate::fonts::FontFiles;
use crate::ids::testing::Saves;
use crate::resolve::ImageFiles;
use crate::wire::{Uses, decode_scene, scene as whole};

fn rows(labels: &[String]) -> String {
    let rows: String = labels
        .iter()
        .map(|l| format!(r##"<Frame name="{l}" width={{40}} height={{10}} fill="#336699" />"##))
        .collect();
    format!(r#"<Frame layout="column">{rows}</Frame>"#)
}

/// The scene a canvas holds, from a whole one on the wire.
fn held(s: &Scene) -> Scene {
    decode_scene(&whole(s), (&ImageFiles::new(), &FontFiles::new())).unwrap()
}

/// Applies `p` to `s` as a canvas does, checking the counts follow.
fn applied(s: &mut Scene, p: &proto::Patch) -> Result<Applied, String> {
    let mut uses = Uses::of(&s.root);
    let done = apply(s, &mut uses, p)?;
    assert_eq!(uses, Uses::of(&s.root), "the counts follow the patch");
    assert_eq!(s.ids.len(), count(&s.root), "every node indexed");
    Ok(done)
}

fn kinds(p: &proto::Patch) -> Vec<&'static str> {
    p.ops
        .iter()
        .map(|o| match o.op.as_ref().unwrap() {
            Kind::Change(_) => "change",
            Kind::Insert(_) => "insert",
            Kind::Remove(_) => "remove",
            Kind::Move(_) => "move",
        })
        .collect()
}

#[test]
fn an_insert_before_the_first_of_a_hundred_thousand_is_one_insert() {
    let labels: Vec<String> = (0..100_000).map(|i| format!("r{i}")).collect();
    let mut saves = Saves::default();
    saves.save("a.design", rows(&labels));
    let old = saves.resolve("a.design");
    let mut more = vec!["new".to_string()];
    more.extend(labels);
    saves.save("a.design", rows(&more));
    let new = saves.resolve("a.design");
    let p = patch(&old, &new);
    assert_eq!(kinds(&p), ["insert"]);
    let mut s = held(&old);
    let done = applied(&mut s, &p).unwrap();
    assert_eq!(done.placed.len(), 1);
    assert!(s.root == new.root);
}

#[test]
fn moves_removes_and_changes_are_found_by_id() {
    let mut saves = Saves::default();
    saves.save(
        "a.design",
        r#"<Frame><Rect key="a" /><Rect key="b" /><Frame key="c"><Rect key="d" /></Frame><Rect key="e" /></Frame>"#,
    );
    let old = saves.resolve("a.design");
    // `e` to the front, `b` gone, `a` changed, and `d` out of `c` to the
    // end: a list is matched within its parent, so `d` is a new element and
    // the old one goes.
    saves.save(
        "a.design",
        r#"<Frame><Rect key="e" /><Rect key="a" x={3} /><Frame key="c" /><Rect key="d" /></Frame>"#,
    );
    let new = saves.resolve("a.design");
    let p = patch(&old, &new);
    let mut k = kinds(&p);
    k.sort();
    assert_eq!(k, ["change", "insert", "move", "remove", "remove"]);
    let mut s = held(&old);
    applied(&mut s, &p).unwrap();
    assert!(s.root == new.root);
}

#[test]
fn a_new_root_replaces_the_scene() {
    let mut saves = Saves::default();
    saves.save("a.design", "<Frame><Rect /></Frame>");
    let old = saves.resolve("a.design");
    let mut new = old.clone();
    new.root.id = NodeId(vec![99]);
    new.ids = crate::tree::Ids::of(&new.root);
    let p = patch(&old, &new);
    assert_eq!(kinds(&p), ["insert"]);
    let mut s = held(&old);
    let done = applied(&mut s, &p).unwrap();
    assert!(s.root == new.root);
    assert_eq!(done.placed, vec![new.root.id.clone()]);
}

/// A patch that can't be applied whole — an id the scene hasn't, a sibling
/// under another parent, a node it has already, a move under itself —
/// changes nothing, even after the operations before it did.
#[test]
fn a_patch_that_cant_be_applied_changes_nothing() {
    let mut saves = Saves::default();
    saves.save(
        "a.design",
        r#"<Frame><Frame><Rect /></Frame><Rect /></Frame>"#,
    );
    let s0 = saves.resolve("a.design");
    let root = &s0.root;
    let inner = &root.children[0];
    let leaf = &inner.children[0];
    let other = &root.children[1];
    let id = |n: &Node| Some(crate::wire::node_id(&n.id));
    let good = op(Kind::Remove(crate::wire::node_id(&other.id)));
    let bad = [
        op(Kind::Remove(proto::NodeId { path: vec![99] })),
        op(Kind::Insert(proto::Insert {
            parent: id(root),
            after: id(leaf),
            node: Some(crate::wire::encode(other)),
        })),
        op(Kind::Insert(proto::Insert {
            parent: id(root),
            after: None,
            node: Some(crate::wire::encode(leaf)),
        })),
        op(Kind::Move(proto::Move {
            id: id(inner),
            parent: id(leaf),
            after: None,
        })),
    ];
    for (n, b) in bad.into_iter().enumerate() {
        let mut s = held(&s0);
        let p = proto::Patch {
            ops: vec![good.clone(), b],
            ..Default::default()
        };
        assert!(applied(&mut s, &p).is_err());
        assert!(s.root == s0.root, "a refused patch leaves the scene: {n}");
        assert_eq!(s.ids.len(), count(&s.root));
    }
}

#[test]
fn the_longest_run_stays() {
    let stay = longest_run(&[Some(3), Some(0), None, Some(1), Some(2)]);
    assert_eq!(stay, [false, true, false, true, true]);
}
