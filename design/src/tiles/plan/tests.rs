use super::*;

/// A drawing 1,000 × 100,000 at its top-left, and a view of it 600 × 400
/// device pixels at 1×, scrolled 300 down: it needs six tiles, (1, 1) the
/// nearest its middle.
fn area() -> KRect {
    KRect::new(0.0, 0.0, 1000.0, 100_000.0)
}

fn view() -> View {
    View {
        width: 600.0,
        height: 400.0,
        zoom: 1.0,
        origin: (0.0, -300.0),
    }
}

fn needed() -> Vec<(u32, u32)> {
    view().tiles(area(), Level::of(1.0))
}

fn key(theme: Theme, level: f64, col: u32, row: u32) -> Key {
    Key {
        theme,
        level: Level::of(level),
        col,
        row,
    }
}

fn light(col: u32, row: u32) -> Key {
    key(Theme::Light, 1.0, col, row)
}

/// A settled frame of the view in the light theme.
fn frame(plan: &mut Plan) -> Work {
    plan.frame(Theme::Light, area(), &view(), &needed(), true)
}

/// Every tile `frame` asked for, painted and arrived: their tickets.
fn arrive(plan: &mut Plan, frame: &Work) -> Vec<Ticket> {
    frame
        .request
        .iter()
        .map(|&(ticket, _)| {
            assert_eq!(plan.arrived(ticket), Arrival::Current);
            ticket
        })
        .collect()
}

/// The budget is the tiles in flight, not the tiles of a frame: a frame
/// asks for the missing tiles nearest the middle first until that many are
/// asked and not yet arrived, and the next asks only for as many as have
/// arrived since.
#[test]
fn a_frame_asks_for_the_missing_tiles_middle_first_while_fewer_than_the_budget_are_in_flight() {
    let mut plan = Plan::new(CAP, 4);
    let first = frame(&mut plan);
    let asked: Vec<Key> = first.request.iter().map(|&(_, k)| k).collect();
    let middle_first: Vec<Key> = needed()[..4].iter().map(|&(c, r)| light(c, r)).collect();
    assert_eq!(asked, middle_first);
    assert_eq!(asked[0], light(1, 1));
    assert_eq!(first.missing, 6);
    assert!(first.draw.is_empty());

    // Four in flight: the next frame asks for none, and still lacks six.
    let second = frame(&mut plan);
    assert!(second.request.is_empty());
    assert_eq!(second.missing, 6);

    // Two arrive: two more are asked for, the last two the view needs.
    for &(ticket, _) in &first.request[..2] {
        assert_eq!(plan.arrived(ticket), Arrival::Current);
    }
    let third = frame(&mut plan);
    let asked: Vec<Key> = third.request.iter().map(|&(_, k)| k).collect();
    let rest: Vec<Key> = needed()[4..].iter().map(|&(c, r)| light(c, r)).collect();
    assert_eq!(asked, rest);
    assert_eq!(third.missing, 4);
    assert_eq!(third.draw.len(), 2);

    // Everything arrives: the view is whole, and nothing is asked again.
    arrive(&mut plan, &third);
    for &(ticket, _) in &first.request[2..] {
        plan.arrived(ticket);
    }
    let whole = frame(&mut plan);
    assert_eq!(
        (whole.missing, whole.request.len(), whole.draw.len()),
        (0, 0, 6)
    );
}

/// While a zoom is still moving, a frame shows what is held and asks for
/// nothing, but says how much it lacks.
#[test]
fn an_unsettled_frame_asks_for_nothing_and_counts_what_it_lacks() {
    let mut plan = Plan::new(CAP, BUDGET);
    let moving = plan.frame(Theme::Light, area(), &view(), &needed(), false);
    assert!(moving.request.is_empty());
    assert_eq!(moving.missing, 6);
    assert_eq!(frame(&mut plan).request.len(), BUDGET);
}

/// Only the answer to a ticket still asked is kept: a ticket never given, a
/// ticket answered already, and a ticket another plan gave — the design a
/// canvas showed before — are dropped. A tile asked for and painted as
/// nothing is asked for again, under a new ticket.
#[test]
fn an_answer_to_a_ticket_not_asked_is_dropped() {
    let mut before = Plan::new(CAP, BUDGET);
    let old = frame(&mut before).request[0].0;
    let mut plan = Plan::new(CAP, BUDGET);
    let asked = frame(&mut plan);
    let (ticket, key) = asked.request[0];
    assert_ne!(ticket, old, "tickets are never given twice");
    assert_eq!(plan.arrived(old), Arrival::Dropped);
    assert_eq!(plan.arrived(0), Arrival::Dropped);
    assert_eq!(plan.arrived(ticket), Arrival::Current);
    assert_eq!(plan.arrived(ticket), Arrival::Dropped);

    let (lost, lost_key) = asked.request[1];
    plan.missed(lost);
    assert_eq!(plan.arrived(lost), Arrival::Dropped);
    let again = frame(&mut plan);
    let (anew, again_key) = again.request[0];
    assert_eq!(again_key, lost_key);
    assert_ne!(anew, lost);
    assert_eq!(
        again.draw.iter().map(|&(t, _)| t).collect::<Vec<_>>(),
        vec![ticket]
    );
    assert_ne!(key, lost_key);
}

/// Damage that reaches a tile while it is being painted makes it arrive
/// stale: it is kept and shown, and asked for again. Damage elsewhere, or in
/// the other theme, leaves a tile in flight arriving current.
#[test]
fn a_tile_damaged_in_flight_arrives_stale_and_is_asked_for_again() {
    let mut plan = Plan::new(CAP, BUDGET);
    let asked = frame(&mut plan);
    let ticket = |k: Key| asked.request.iter().find(|&&(_, at)| at == k).unwrap().0;
    // Inside tile (1, 1) only.
    plan.damage(
        Theme::Light,
        (0.0, 0.0),
        &[KRect::new(300.0, 300.0, 310.0, 310.0)],
    );
    plan.damage_all(Theme::Dark);
    assert_eq!(plan.arrived(ticket(light(1, 1))), Arrival::Stale);
    assert_eq!(plan.arrived(ticket(light(0, 1))), Arrival::Current);
    let next = frame(&mut plan);
    assert!(next.draw.iter().any(|&(t, _)| t == ticket(light(1, 1))));
    assert_eq!(next.request.first().map(|&(_, k)| k), Some(light(1, 1)));

    // Damage to a tile held marks it stale too: asked for again, still shown.
    let mut plan = Plan::new(CAP, BUDGET);
    let first = frame(&mut plan);
    arrive(&mut plan, &first);
    plan.damage_all(Theme::Light);
    let next = frame(&mut plan);
    assert_eq!(next.draw.len(), BUDGET);
    assert_eq!(next.request.len(), BUDGET);
    assert_eq!(next.missing, 6);
}

/// A tile painted again replaces the one it was, and a plan past its cap
/// drops the least recently used: each tile let go is in the next frame's
/// evictions, once, for its host to free.
#[test]
fn every_tile_let_go_is_evicted_once() {
    let mut plan = Plan::new(4, BUDGET);
    let first = frame(&mut plan);
    let tickets = arrive(&mut plan, &first);
    // Six arrived into a cap of four: the first two let go.
    let next = frame(&mut plan);
    assert_eq!(next.evict, tickets[..2].to_vec());
    assert_eq!(next.draw.len(), 4);
    // They are asked for again; the next frame evicts nothing new.
    assert_eq!(next.request.len(), 2);
    assert!(frame(&mut plan).evict.is_empty());

    // A stale tile painted again: the old painting is let go.
    let mut plan = Plan::new(CAP, BUDGET);
    let first = frame(&mut plan);
    arrive(&mut plan, &first);
    plan.damage(
        Theme::Light,
        (0.0, 0.0),
        &[KRect::new(300.0, 300.0, 310.0, 310.0)],
    );
    let again = frame(&mut plan);
    let old = first
        .request
        .iter()
        .find(|&&(_, k)| k == light(1, 1))
        .unwrap()
        .0;
    assert_eq!(again.request.len(), 1);
    arrive(&mut plan, &again);
    let after = frame(&mut plan);
    assert_eq!(after.evict, vec![old]);
    assert!(after.draw.iter().all(|&(t, _)| t != old));
    assert!(frame(&mut plan).evict.is_empty());
}

/// A frame draws every held tile of its theme the view shows, at any level,
/// where it lands — coarser levels under finer ones and the view's own level
/// on top, so a stretched stand-in never covers a tile at the right scale.
#[test]
fn a_frame_draws_coarse_under_fine_and_the_views_own_level_on_top() {
    let mut plan = Plan::new(CAP, usize::MAX);
    let at = |level: f64| View {
        zoom: level,
        ..view()
    };
    let mut held = Vec::new();
    for (theme, level) in [
        (Theme::Light, 2.0),
        (Theme::Dark, 1.0),
        (Theme::Light, 1.0),
        (Theme::Light, 0.5),
    ] {
        let view = at(level);
        let needed = view.tiles(area(), Level::of(level));
        let asked = plan.frame(theme, area(), &view, &needed, true);
        for &(ticket, k) in &asked.request {
            plan.arrived(ticket);
            held.push((ticket, k));
        }
    }
    let shown = frame(&mut plan);
    let levels: Vec<Level> = shown
        .draw
        .iter()
        .map(|&(t, _)| held.iter().find(|&&(h, _)| h == t).unwrap().1)
        .inspect(|k| assert_eq!(k.theme, Theme::Light))
        .map(|k| k.level)
        .collect();
    let mut expected = levels.clone();
    expected.sort_by_key(|l| (*l == Level::of(1.0), l.0));
    assert_eq!(levels, expected);
    assert_eq!(levels.first(), Some(&Level::of(0.5)));
    assert_eq!(levels.last(), Some(&Level::of(1.0)));
    for &(ticket, placed) in &shown.draw {
        let k = held.iter().find(|&&(h, _)| h == ticket).unwrap().1;
        assert_eq!(Some(placed), view().place(area(), &k));
    }
}
