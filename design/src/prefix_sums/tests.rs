use super::*;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Total(i64, usize);

impl Summary for Total {
    fn then(&self, next: &Self) -> Self {
        Total(self.0 + next.0, self.1 + next.1)
    }
}

impl Item for i64 {
    type Sum = Total;
    fn summary(&self) -> Total {
        Total(*self, 1)
    }
}

/// Random inserts, removes and replacements agree with a sorted map on
/// every prefix, every search and every walk.
#[test]
fn sums_agree_with_a_sorted_map() {
    let mut s: Sums<u32, i64> = Sums::new();
    let mut m = std::collections::BTreeMap::new();
    let mut seed = 7u64;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    for step in 0..4000 {
        let k = next(500) as u32;
        match next(3) {
            0 | 1 => {
                let v = next(100) as i64;
                assert_eq!(s.insert(k, v), m.insert(k, v));
            }
            _ => assert_eq!(s.remove(&k), m.remove(&k)),
        }
        if step % 97 == 0 {
            assert_eq!(s.len(), m.len());
            let all: Vec<_> = s.iter().map(|(k, v)| (*k, *v)).collect();
            let want: Vec<_> = m.iter().map(|(k, v)| (*k, *v)).collect();
            assert_eq!(all, want);
            for q in [0, 17, 250, 499, 600] {
                let before: i64 = m.range(..q).map(|(_, v)| v).sum();
                assert_eq!(s.before(&q).0, before);
                let from: Vec<_> = s.from(&q).map(|(k, _)| *k).collect();
                let want: Vec<_> = m.range(q..).map(|(k, _)| *k).collect();
                assert_eq!(from, want);
            }
            let total: i64 = m.values().sum();
            for target in [0, total / 3, total] {
                let mut acc = 0;
                let want = m.iter().find(|(_, v)| {
                    acc += **v;
                    acc > target
                });
                let got = s.seek(|t| t.0 > target).map(|(k, _, b)| (*k, b.0));
                let mut before = 0;
                let want = want.map(|(k, _)| {
                    before = m.range(..*k).map(|(_, v)| v).sum();
                    (*k, before)
                });
                assert_eq!(got, want);
            }
        }
    }
}

/// 100,000 items: an insert before the first and a prefix at the end take
/// log time, not a walk.
#[test]
fn a_hundred_thousand_items_are_cheap_to_change() {
    let mut s: Sums<u32, i64> = Sums::new();
    for k in 1..=100_000u32 {
        s.insert(k * 2, 1);
    }
    let t = std::time::Instant::now();
    for i in 0..1000 {
        s.insert(1, i);
        assert_eq!(s.before(&200_000).1, 100_000);
    }
    assert!(t.elapsed().as_millis() < 200, "{:?}", t.elapsed());
}
