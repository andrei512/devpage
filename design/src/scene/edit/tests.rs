use super::*;
use crate::ids::Remembering;
use std::collections::HashMap;

fn rows(labels: &[String]) -> String {
    let rows: String = labels
        .iter()
        .map(|l| format!(r##"<Frame name="{l}" width={{40}} height={{10}} fill="#336699" />"##))
        .collect();
    format!(r#"<Frame layout="column">{rows}</Frame>"#)
}

/// Applies `edits` to `s`, checking the counts and the index follow.
fn applied(s: &mut Scene, edits: &Edits) -> Result<Applied, String> {
    let mut uses = Uses::of(&s.root);
    let done = apply(s, &mut uses, edits)?;
    assert_eq!(uses, Uses::of(&s.root), "the counts follow the edits");
    assert_eq!(s.ids.len(), count(&s.root), "every node indexed");
    Ok(done)
}

fn kinds(edits: &Edits) -> Vec<&'static str> {
    edits
        .list
        .iter()
        .map(|e| match e {
            Edit::Change(_) => "change",
            Edit::Insert { .. } => "insert",
            Edit::Remove(_) => "remove",
            Edit::Move { .. } => "move",
        })
        .collect()
}

#[test]
fn an_insert_before_the_first_of_a_hundred_thousand_is_one_insert() {
    let labels: Vec<String> = (0..100_000).map(|i| format!("r{i}")).collect();
    let mut saves = Remembering::new(HashMap::<String, String>::new());
    saves.files.insert("a.design".into(), rows(&labels));
    let old = crate::resolve(&saves, "a.design").unwrap();
    let mut more = vec!["new".to_string()];
    more.extend(labels);
    saves.files.insert("a.design".into(), rows(&more));
    let new = crate::resolve(&saves, "a.design").unwrap();
    let edits = Edits::between(&old, &new);
    assert_eq!(kinds(&edits), ["insert"]);
    let mut s = old.clone();
    let done = applied(&mut s, &edits).unwrap();
    assert_eq!(done.placed.len(), 1);
    assert!(s.root == new.root);
}

#[test]
fn moves_removes_and_changes_are_found_by_id() {
    let mut saves = Remembering::new(HashMap::<String, String>::new());
    saves.files.insert("a.design".into(), r#"<Frame><Rect key="a" /><Rect key="b" /><Frame key="c"><Rect key="d" /></Frame><Rect key="e" /></Frame>"#.into());
    let old = crate::resolve(&saves, "a.design").unwrap();
    // `e` to the front, `b` gone, `a` changed, and `d` out of `c` to the
    // end: a list is matched within its parent, so `d` is a new element and
    // the old one goes.
    saves.files.insert(
        "a.design".into(),
        r#"<Frame><Rect key="e" /><Rect key="a" x={3} /><Frame key="c" /><Rect key="d" /></Frame>"#
            .into(),
    );
    let new = crate::resolve(&saves, "a.design").unwrap();
    let edits = Edits::between(&old, &new);
    let mut k = kinds(&edits);
    k.sort();
    assert_eq!(k, ["change", "insert", "move", "remove", "remove"]);
    let mut s = old.clone();
    applied(&mut s, &edits).unwrap();
    assert!(s.root == new.root);
}

#[test]
fn a_new_root_replaces_the_scene() {
    let mut saves = Remembering::new(HashMap::<String, String>::new());
    saves
        .files
        .insert("a.design".into(), "<Frame><Rect /></Frame>".into());
    let old = crate::resolve(&saves, "a.design").unwrap();
    let mut new = old.clone();
    new.root.id = NodeId(vec![99]);
    new.ids = Ids::of(&new.root);
    let edits = Edits::between(&old, &new);
    assert_eq!(kinds(&edits), ["insert"]);
    let mut s = old.clone();
    let done = applied(&mut s, &edits).unwrap();
    assert!(s.root == new.root);
    assert_eq!(done.placed, vec![new.root.id.clone()]);
}

/// Edits that can't be applied whole — an id the scene hasn't, a sibling
/// under another parent, a node it has already, a move under itself — change
/// nothing, even after the edits before them did.
#[test]
fn edits_that_cant_be_applied_change_nothing() {
    let mut saves = Remembering::new(HashMap::<String, String>::new());
    saves.files.insert(
        "a.design".into(),
        r#"<Frame><Frame><Rect /></Frame><Rect /></Frame>"#.into(),
    );
    let s0 = crate::resolve(&saves, "a.design").unwrap();
    let root = &s0.root;
    let inner = &root.children[0];
    let leaf = &inner.children[0];
    let other = &root.children[1];
    let good = Edit::Remove(other.id.clone());
    let bad = [
        Edit::Remove(NodeId(vec![99])),
        Edit::Insert {
            parent: Some(root.id.clone()),
            after: Some(leaf.id.clone()),
            node: other.clone(),
        },
        Edit::Insert {
            parent: Some(root.id.clone()),
            after: None,
            node: leaf.clone(),
        },
        Edit::Move {
            id: inner.id.clone(),
            parent: leaf.id.clone(),
            after: None,
        },
    ];
    for (n, b) in bad.into_iter().enumerate() {
        let mut s = s0.clone();
        let edits = Edits {
            list: vec![good.clone(), b],
            ..Default::default()
        };
        assert!(applied(&mut s, &edits).is_err());
        assert!(s.root == s0.root, "refused edits leave the scene: {n}");
        assert_eq!(s.ids.len(), count(&s.root));
    }
}

/// Edits carry only the files the scene before didn't hold, and a scene
/// keeps only the files its nodes still use.
#[test]
fn edits_carry_only_the_files_the_scene_lacks() {
    let png = |rgba: [u8; 4]| {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 1, 1);
        e.set_color(png::ColorType::Rgba);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&rgba).unwrap();
        drop(w);
        out
    };
    struct Bytes(HashMap<String, Vec<u8>>);
    impl crate::Files for Bytes {
        fn bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
        fn list(&self, path: &str) -> Option<Vec<String>> {
            crate::scene::listed(self.0.keys(), path)
        }
    }
    let design = |text: &str| {
        let files = Bytes(HashMap::from([
            ("a.png".to_string(), png([255, 0, 0, 255])),
            ("b.png".to_string(), png([0, 0, 255, 255])),
            ("a.design".to_string(), text.as_bytes().to_vec()),
        ]));
        crate::resolve(&files, "a.design").unwrap()
    };
    let old = design(r#"<Frame><Image src="./a.png" /></Frame>"#);
    let both = design(r#"<Frame><Image src="./a.png" /><Image src="./b.png" /></Frame>"#);
    let edits = Edits::between(&old, &both);
    let b = both.root.children[1].image.as_ref().unwrap().id;
    assert_eq!(edits.images.keys().collect::<Vec<_>>(), [&b], "only b");
    let mut s = old.clone();
    applied(&mut s, &edits).unwrap();
    assert_eq!(s.images.len(), 2);
    let only_b = design(r#"<Frame><Image src="./b.png" /></Frame>"#);
    let edits = Edits::between(&s, &only_b);
    assert!(edits.images.is_empty(), "b is held already");
    applied(&mut s, &edits).unwrap();
    assert_eq!(s.images.keys().collect::<Vec<_>>(), [&b], "a is dropped");
}

#[test]
fn the_longest_run_stays() {
    let stay = longest_run(&[Some(3), Some(0), None, Some(1), Some(2)]);
    assert_eq!(stay, [false, true, false, true, true]);
}
