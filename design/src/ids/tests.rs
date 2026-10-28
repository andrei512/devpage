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
    let pairs = myers(a.len(), b.len(), &|i, j| a[i] == b[j]).unwrap();
    assert_eq!(pairs.len(), 4);
    assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
    assert!(pairs.iter().all(|&(i, j)| a[i] == b[j]));
}

/// The rows `r{i}` for `order`, each distinct, as a design's children.
fn numbered(order: &[usize]) -> String {
    let rows: String = order
        .iter()
        .map(|i| format!(r#"<Rect name="r{i}" />"#))
        .collect();
    format!("<Frame>{rows}</Frame>")
}

/// How many of `after`'s rows kept the id their row had in `before`, and how
/// many took another row's id — an edit where there was none.
fn kept(before: &[usize], after: &[usize]) -> (usize, usize, std::time::Duration) {
    let last = parse(&numbered(before)).unwrap();
    let new = parse(&numbered(after)).unwrap();
    let t = std::time::Instant::now();
    let new = matched(new, &last);
    let took = t.elapsed();
    let id_of: HashMap<usize, u32> = before
        .iter()
        .zip(elements(&last.root.children))
        .map(|(&r, e)| (r, e.id))
        .collect();
    let by_id: HashMap<u32, usize> = id_of.iter().map(|(&r, &id)| (id, r)).collect();
    let (mut same, mut wrong) = (0, 0);
    for (&r, e) in after.iter().zip(elements(&new.root.children)) {
        match by_id.get(&e.id) {
            Some(&was) if was == r => same += 1,
            Some(_) => wrong += 1,
            None => {}
        }
    }
    (same, wrong, took)
}

/// A shuffle that is the same on every run.
fn shuffled(n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    for i in (1..n).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        v.swap(i, (s % (i as u64 + 1)) as usize);
    }
    v
}

/// Rewrites past the edit cap keep every surviving row's id and give no row
/// another's: every third row removed, a shuffle, a reversal, half the rows
/// replaced.
#[test]
fn a_list_rewritten_past_the_edit_cap_keeps_the_ids_of_what_survived() {
    let n = 1_000;
    let all: Vec<usize> = (0..n).collect();
    let thirds: Vec<usize> = all.iter().copied().filter(|i| i % 3 != 1).collect();
    let (same, wrong, _) = kept(&all, &thirds);
    assert_eq!((same, wrong), (thirds.len(), 0));
    for after in [shuffled(n), all.iter().rev().copied().collect()] {
        let (same, wrong, _) = kept(&all, &after);
        assert_eq!((same, wrong), (n, 0));
    }
    // Half replaced by new rows, interleaved: the old half keeps its ids.
    let half: Vec<usize> = all
        .iter()
        .map(|&i| if i % 2 == 0 { i } else { n + i })
        .collect();
    let (same, _, _) = kept(&all, &half);
    assert_eq!(same, n / 2);
}

/// Matching a rewritten list stays near-linear: run with `--release
/// --ignored` to print the times at 1,000, 10,000 and 100,000 rows.
#[test]
#[ignore]
fn rewritten_lists_match_in_near_linear_time() {
    for n in [1_000usize, 10_000, 100_000] {
        let all: Vec<usize> = (0..n).collect();
        let cases: [(&str, Vec<usize>); 4] = [
            (
                "every third removed",
                all.iter().copied().filter(|i| i % 3 != 1).collect(),
            ),
            ("shuffled", shuffled(n)),
            ("reversed", all.iter().rev().copied().collect()),
            (
                "half replaced",
                all.iter()
                    .map(|&i| if i % 2 == 0 { i } else { n + i })
                    .collect(),
            ),
        ];
        for (name, after) in cases {
            let (same, wrong, took) = kept(&all, &after);
            eprintln!("{n} {name}: {same} kept, {wrong} wrong, {took:?}");
        }
    }
}

/// A list whose elements are unique only inside ever smaller gaps — each
/// level's halves apart from each other but alike across — recurses no
/// deeper than the cap and still matches what it can.
#[test]
fn nested_gaps_stop_at_the_depth_cap() {
    // r0 r1 … repeated so that most rows occur twice, then reversed: Myers
    // can't, and uniqueness is scarce.
    let n = 4_000;
    let before: Vec<usize> = (0..n).map(|i| i / 2).collect();
    let after: Vec<usize> = before.iter().rev().copied().collect();
    let last = parse(&numbered(&before)).unwrap();
    let new = matched(parse(&numbered(&after)).unwrap(), &last);
    // Every row has an id, unique among its siblings.
    let ids: std::collections::HashSet<u32> = elements(&new.root.children).map(|e| e.id).collect();
    assert_eq!(ids.len(), n);
}
