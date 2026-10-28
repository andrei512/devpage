use super::*;

#[test]
fn keys_spread_evenly_sort_in_order() {
    for n in [1, 2, 254, 255, 256, 70_000] {
        let keys: Vec<OrderKey> = (0..n).map(|i| OrderKey::nth(i, n)).collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{n}");
    }
}

#[test]
fn a_key_between_two_sorts_between_them_forever() {
    let mut keys = vec![OrderKey::nth(0, 2), OrderKey::nth(1, 2)];
    // Always before the first, always after the last, always in the middle.
    for round in 0..2000 {
        let k = match round % 3 {
            0 => OrderKey::between(None, keys.first()),
            1 => OrderKey::between(keys.last(), None),
            _ => {
                let at = keys.len() / 2;
                OrderKey::between(Some(&keys[at - 1]), Some(&keys[at]))
            }
        };
        assert!(k.0.last().is_some_and(|d| *d != 0));
        keys.push(k);
        keys.sort();
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
    }
    // Inserting before the first again and again grows a key by a byte per
    // eight or so.
    assert!(keys.iter().map(|k| k.0.len()).max().unwrap() < 200);
}

#[test]
fn children_insert_anywhere_and_keep_order() {
    let mut c: Children<u32> = Children::from_vec(vec![1, 2, 3]);
    let k = c.insert_after(None, 0);
    assert_eq!(c.first_key(), Some(&k));
    let two = c.entries().find(|(_, v)| **v == 2).unwrap().0.clone();
    c.insert_after(Some(&two), 25);
    c.push(4);
    assert_eq!(
        c.iter().copied().collect::<Vec<_>>(),
        vec![0, 1, 2, 25, 3, 4]
    );
    assert_eq!(c.rank(&two), 2);
    assert_eq!(c[3], 25);
    // The same children placed otherwise are the same.
    assert_eq!(c, Children::from_vec(vec![0, 1, 2, 25, 3, 4]));
}
