use super::*;
use crate::parse::parse;

/// The ids of the root's children, in order, after `after` is matched
/// against `before`.
fn ids(before: &str, after: &str) -> (Vec<u32>, File) {
    let last = parse(before).unwrap();
    let new = matched(parse(after).unwrap(), &last);
    let ids = elements(&new.root.children).map(|e| e.id).collect();
    (ids, new)
}

fn rows(labels: &[&str]) -> String {
    let rows: String = labels
        .iter()
        .map(|l| format!(r#"<Rect name="{l}" />"#))
        .collect();
    format!("<Frame>{rows}</Frame>")
}

#[test]
fn a_first_parse_numbers_elements_in_order() {
    let f = parse(&rows(&["a", "b"])).unwrap();
    assert_eq!(f.root.id, 0);
    assert_eq!(
        elements(&f.root.children).map(|e| e.id).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(f.next_id, 3);
}

#[test]
fn an_insert_at_the_front_keeps_every_other_id() {
    let (ids, f) = ids(&rows(&["a", "b", "c"]), &rows(&["new", "a", "b", "c"]));
    assert_eq!(ids, [4, 1, 2, 3]);
    assert_eq!(f.next_id, 5);
}

#[test]
fn a_removal_and_an_edit_keep_the_rest() {
    let (ids, _) = ids(&rows(&["a", "b", "c", "d"]), &rows(&["a", "c", "dd"]));
    // `b` gone; `d` renamed in place is an edit of `d`.
    assert_eq!(ids, [1, 3, 4]);
}

#[test]
fn keyed_siblings_keep_their_ids_when_reordered() {
    let before = r#"<Frame><Rect key="x" /><Rect key="y" /><Rect key="z" /></Frame>"#;
    let after = r#"<Frame><Rect key="z" /><Rect key="x" /><Rect key="y" /></Frame>"#;
    let (ids, _) = ids(before, after);
    assert_eq!(ids, [3, 1, 2]);
}

#[test]
fn children_of_a_matched_element_are_matched_too_and_new_ones_numbered() {
    let before = "<Frame><Frame><Rect /></Frame></Frame>";
    let after = "<Frame><Frame><Ellipse /><Rect /></Frame></Frame>";
    let last = parse(before).unwrap();
    let new = matched(parse(after).unwrap(), &last);
    let inner = elements(&new.root.children).next().unwrap();
    assert_eq!(inner.id, 1);
    let kids: Vec<u32> = elements(&inner.children).map(|e| e.id).collect();
    assert_eq!(kids, [3, 2]);
    // A different root is a new root.
    let other = matched(parse("<Rect />").unwrap(), &last);
    assert_eq!(other.root.id, 3);
}

#[test]
fn a_hundred_thousand_siblings_match_in_a_pass() {
    let labels: Vec<String> = (0..100_000).map(|i| format!("r{i}")).collect();
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let before = rows(&refs);
    let mut after_refs = vec!["new"];
    after_refs.extend(&refs);
    let after = rows(&after_refs);
    let t = std::time::Instant::now();
    let (ids, _) = ids(&before, &after);
    assert_eq!(ids[0], 100_001);
    assert_eq!(ids[1..], (1..=100_000).collect::<Vec<u32>>()[..]);
    // Far below what quadratic matching would take.
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
}

#[test]
fn myers_finds_a_longest_common_subsequence() {
    let a = b"ABCABBA";
    let b = b"CBABAC";
    let pairs = myers(a.len(), b.len(), &|i, j| a[i] == b[j]);
    assert_eq!(pairs.len(), 4);
    assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
    assert!(pairs.iter().all(|&(i, j)| a[i] == b[j]));
}
