use super::*;
use crate::ids::testing::Saves;
use crate::scene::Edits;
use crate::wire::{decode_scene, scene as whole};

/// The scene a canvas holds, from a whole one on the wire.
fn held(s: &Scene) -> Scene {
    decode_scene(&whole(s), (&Default::default(), &Default::default())).unwrap()
}

/// Edits of every kind travel as a patch and come back the same.
#[test]
fn edits_come_back_from_their_patch() {
    let mut saves = Saves::default();
    saves.save(
        "a.design",
        r#"<Frame><Rect key="a" /><Rect key="b" /><Frame key="c"><Rect key="d" /></Frame><Rect key="e" /></Frame>"#,
    );
    let old = saves.resolve("a.design");
    saves.save(
        "a.design",
        r#"<Frame><Rect key="e" /><Rect key="a" x={3} /><Frame key="c" /><Rect key="d" /></Frame>"#,
    );
    let new = saves.resolve("a.design");
    let sent = Edits::between(&old, &new);
    let came = edits(&patch(&sent), &old).unwrap();
    assert_eq!(came, sent);
    let mut s = held(&old);
    let mut uses = Uses::of(&s.root);
    apply(&mut s, &mut uses, &patch(&sent)).unwrap();
    assert!(s.root == new.root);
}

/// A patch that isn't one — an empty operation, an insert with no node, a
/// node with no id — is refused before anything is applied.
#[test]
fn a_patch_that_isnt_one_is_refused() {
    let mut saves = Saves::default();
    saves.save("a.design", "<Frame><Rect /></Frame>");
    let s0 = saves.resolve("a.design");
    let insert = |node| {
        Kind::Insert(proto::Insert {
            parent: Some(node_id(&s0.root.id)),
            after: None,
            node,
        })
    };
    for ops in [
        vec![proto::Op { op: None }],
        vec![proto::Op {
            op: Some(insert(None)),
        }],
        vec![proto::Op {
            op: Some(insert(Some(proto::Node::default()))),
        }],
    ] {
        let p = proto::Patch {
            ops,
            ..Default::default()
        };
        let mut s = held(&s0);
        let mut uses = Uses::of(&s.root);
        assert!(apply(&mut s, &mut uses, &p).is_err());
        assert!(s.root == s0.root);
    }
}
