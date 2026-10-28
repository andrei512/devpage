use super::*;

#[test]
fn the_extent_is_the_least_and_greatest_edge_of_what_is_in_it() {
    let mut e = Extent::default();
    let (a, b) = (
        KRect::new(0.0, 5.0, 10.0, 20.0),
        KRect::new(-3.0, 7.0, 4.0, 30.0),
    );
    e.add(a);
    e.add(b);
    e.add(b);
    assert_eq!(e.rect(), Some(KRect::new(-3.0, 5.0, 10.0, 30.0)));
    e.remove(b);
    assert_eq!(e.rect(), Some(KRect::new(-3.0, 5.0, 10.0, 30.0)));
    e.remove(b);
    assert_eq!(e.rect(), Some(a));
    e.remove(a);
    assert_eq!(e.rect(), None);
}
