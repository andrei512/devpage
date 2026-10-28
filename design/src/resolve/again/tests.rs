use super::*;
use crate::ids::testing::Saves;

/// A design and the files it reads, as a test edits them.
#[derive(Clone)]
struct Model {
    /// Each row: a number naming it, whether it has a key, its text, and
    /// whether it is a `Card` instance rather than a frame.
    rows: Vec<(u32, bool, String, bool)>,
    next: u32,
    layout: &'static str,
    fill: &'static str,
    card_fill: &'static str,
    card_radius: u32,
    accent: &'static str,
    /// Whether a blank line sits at the top, moving every element down.
    blank: bool,
}

impl Model {
    fn new(n: u32) -> Model {
        Model {
            rows: (0..n)
                .map(|i| (i, i % 3 == 0, format!("Row {i}"), i % 4 == 1))
                .collect(),
            next: n,
            layout: "column",
            fill: "#ffffff",
            card_fill: "#eeeeee",
            card_radius: 4,
            accent: "#2563eb",
            blank: false,
        }
    }

    fn screen(&self) -> String {
        let rows: String = self
            .rows
            .iter()
            .map(|(n, keyed, text, card)| {
                let key = if *keyed {
                    format!(" key=\"r{n}\"")
                } else {
                    String::new()
                };
                if *card {
                    format!("  <Card{key} label=\"{text}\" />\n")
                } else {
                    format!(
                        "  <Frame{key} width={{200}} height={{20}} fill={{theme.accent}}><Text>{text}</Text></Frame>\n"
                    )
                }
            })
            .collect();
        format!(
            "{}import Card from \"./card.design\"\n<Frame layout=\"{}\" fill=\"{}\">\n{rows}</Frame>\n",
            if self.blank { "\n" } else { "" },
            self.layout,
            self.fill
        )
    }

    fn card(&self) -> String {
        format!(
            "<Frame props={{{{ label: \"Card\", tint: theme.accent }}}} width={{120}} height={{30}} fill=\"{}\" radius={{{}}} shadow={{{{ y: 2, blur: 4, color: \"#00000033\" }}}}>\n  <Text>{{label}}</Text>\n  <Rect x={{100}} y={{0}} width={{10}} height={{4}} fill={{tint}} />\n</Frame>\n",
            self.card_fill, self.card_radius
        )
    }

    fn themes(&self) -> String {
        format!(
            "kind: themes\nlight:\n  accent: \"{}\"\ndark:\n  accent: \"#000000\"\n",
            self.accent
        )
    }
}

/// A small, fixed random sequence.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One random edit to `m`, and the file it saves.
fn edit(m: &mut Model, rng: &mut Rng) -> &'static str {
    let n = m.rows.len();
    match rng.below(13) {
        0 => {
            let at = [0, n / 2, n][rng.below(3)];
            m.rows.insert(
                at,
                (
                    m.next,
                    rng.below(2) == 0,
                    format!("New {}", m.next),
                    rng.below(3) == 0,
                ),
            );
            m.next += 1;
        }
        1 if n > 1 => {
            m.rows.remove(rng.below(n));
        }
        2 if n > 1 => {
            let row = m.rows.remove(rng.below(n));
            let at = rng.below(m.rows.len() + 1);
            m.rows.insert(at, row);
        }
        3 if n > 0 => {
            let i = rng.below(n);
            m.rows[i].2 = format!("Edited {}", rng.below(1000));
        }
        4 if n > 0 => {
            let i = rng.below(n);
            m.rows[i].3 = !m.rows[i].3;
        }
        5 => m.layout = ["column", "row", "none"][rng.below(3)],
        6 => m.fill = ["#ffffff", "#f0f0f0"][rng.below(2)],
        7 => {
            m.card_fill = ["#eeeeee", "#dddddd"][rng.below(2)];
            return "card.design";
        }
        8 => {
            m.card_radius = rng.below(8) as u32;
            return "card.design";
        }
        9 => {
            m.accent = ["#2563eb", "#16a34a"][rng.below(2)];
            return "themes.yml";
        }
        10 => m.blank = !m.blank,
        11 if n > 0 => {
            let i = rng.below(n);
            m.rows[i].1 = !m.rows[i].1;
        }
        _ => {
            // Nothing: saved as it was.
        }
    }
    "screen.design"
}

fn save(saves: &mut Saves, m: &Model) {
    saves.save("screen.design", m.screen());
    saves.save("card.design", m.card());
    saves.save("themes.yml", m.themes());
}

/// The same scene, node for node, as the wire carries it and as the store
/// keeps it.
fn same(a: &Scene, b: &Scene) {
    assert_eq!(a.root, b.root);
    assert_eq!(a.images, b.images);
    assert_eq!(a.fonts, b.fonts);
    assert_eq!(a.themed, b.themed);
    assert_eq!(a.ids.len(), b.ids.len());
}

/// Every kind of save — rows inserted at the front, middle and end, removed,
/// moved, keyed and not, made instances and frames again, their text
/// edited, the layout and fill of their frame, a component's attributes, a
/// theme's color, a line added above everything — resolved again only as
/// far as it reaches: the scene kept is a fresh resolve's every time, the
/// patch brings the browser's copy there, and a problem is every problem.
#[test]
fn every_save_resolved_again_is_a_fresh_resolve() {
    for seed in 1..=12u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut m = Model::new(6 + seed as u32);
        let mut saves = Saves::default();
        save(&mut saves, &m);
        let mut res = Resolution::new(&saves, "screen.design").unwrap_or_else(|p| panic!("{p:?}"));
        let mut browser = wire::decode_scene(
            &wire::scene(res.scene()),
            (&Default::default(), &Default::default()),
        )
        .unwrap();
        let mut uses = Uses::of(&browser.root);
        for step in 0..80 {
            let file = edit(&mut m, &mut rng);
            save(&mut saves, &m);
            let outcome = res.update(&saves, &[file.to_string()]);
            let fresh = crate::resolve(&saves, "screen.design");
            match (fresh, outcome) {
                (Ok(fresh), Outcome::Same) => same(res.scene(), &fresh),
                (Ok(fresh), Outcome::Changed(patch, _)) => {
                    same(res.scene(), &fresh);
                    wire::apply(&mut browser, &mut uses, &patch)
                        .unwrap_or_else(|e| panic!("seed {seed} step {step}: {e}"));
                    assert_eq!(
                        wire::encode(&browser.root),
                        wire::encode(&fresh.root),
                        "seed {seed} step {step}"
                    );
                }
                (Err(want), Outcome::Problems(got)) => assert_eq!(want, got),
                (fresh, outcome) => panic!(
                    "seed {seed} step {step}: fresh {:?}, but {outcome:?}",
                    fresh.map(|_| "a scene")
                ),
            }
            assert_eq!(
                res.wholes, 0,
                "seed {seed} step {step}: brought across whole"
            );
        }
    }
}

/// The rows of a list of `n`, the `edited`th's text set apart.
fn list(n: usize, edited: Option<usize>, front: bool) -> String {
    let mut body = String::new();
    if front {
        body.push_str("  <Frame width={400} height={20}><Text>Front</Text></Frame>\n");
    }
    for i in 0..n {
        let t = if Some(i) == edited {
            "Edited".to_string()
        } else {
            format!("Row {i}")
        };
        body.push_str(&format!(
            "  <Frame width={{400}} height={{20}}><Text>{t}</Text></Frame>\n"
        ));
    }
    format!("<Frame layout=\"column\">\n{body}</Frame>\n")
}

/// An edit to one row's text, and a row inserted before the first, are
/// patches of one operation, and resolving them again costs no more on
/// 100,000 rows than on 1,000, apart from reading the saved file.
#[test]
fn an_edit_resolves_again_only_what_it_reaches() {
    let mut cost = Vec::new();
    for n in [1_000usize, 100_000] {
        let mut saves = Saves::default();
        saves.save("a.design", list(n, None, false));
        let mut res = Resolution::new(&saves, "a.design").unwrap();
        saves.save("a.design", list(n, Some(n / 2), false));
        let outcome = res.update(&saves, &["a.design".to_string()]);
        let Outcome::Changed(patch, _) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(patch.ops.len(), 1, "{n}");
        saves.save("a.design", list(n, Some(n / 2), true));
        let Outcome::Changed(patch, applied) = res.update(&saves, &["a.design".to_string()]) else {
            panic!()
        };
        assert_eq!(patch.ops.len(), 1, "{n}");
        assert_eq!(applied.placed.len(), 1);
        same(res.scene(), &crate::resolve(&saves, "a.design").unwrap());
        // What resolving again costs beyond reading the file: the update,
        // less a parse and match of the same text.
        saves.save("a.design", list(n, Some(n / 3), true));
        let t = std::time::Instant::now();
        let outcome = res.update(&saves, &["a.design".to_string()]);
        let update = t.elapsed();
        assert!(matches!(outcome, Outcome::Changed(..)));
        let t = std::time::Instant::now();
        let text = list(n, Some(n / 3), true);
        let _ = saves.parse("a.design", &text).unwrap();
        let read = t.elapsed();
        cost.push((n, update, read));
    }
    eprintln!("{cost:?}");
    // The update is reading the file and a little more, at every size.
    for &(n, update, read) in &cost {
        assert!(
            update < read * 4 + std::time::Duration::from_millis(5),
            "{n}: update {update:?}, reading the file {read:?}"
        );
    }
}

/// A save to a component resolves again only its instances' nodes; a change
/// of theme only the nodes that read the theme.
#[test]
fn a_component_or_a_theme_reaches_only_what_uses_it() {
    let mut m = Model::new(10);
    let mut saves = Saves::default();
    save(&mut saves, &m);
    let mut res = Resolution::new(&saves, "screen.design").unwrap();
    m.card_fill = "#dddddd";
    save(&mut saves, &m);
    let Outcome::Changed(_, applied) = res.update(&saves, &["card.design".to_string()]) else {
        panic!()
    };
    let cards = m.rows.iter().filter(|r| r.3).count();
    assert_eq!(applied.changed.len(), cards);
    for (id, _) in &applied.changed {
        assert_eq!(id.0.len(), 2, "an instance's root: {id}");
    }
    m.accent = "#16a34a";
    save(&mut saves, &m);
    let Outcome::Changed(_, applied) = res.update(&saves, &["themes.yml".to_string()]) else {
        panic!()
    };
    // Every plain row's fill, and each card's tint: a theme color among its
    // design's props defaults, which the theme change reaches through it.
    assert_eq!(applied.changed.len(), m.rows.len());
    same(
        res.scene(),
        &crate::resolve(&saves, "screen.design").unwrap(),
    );
}

/// A save that breaks the design keeps its last scene; saves in the meantime
/// are kept, and the save that fixes it brings the scene across all of
/// them, against that last scene.
#[test]
fn saves_while_a_design_has_problems_arrive_with_its_fix() {
    let mut m = Model::new(5);
    let mut saves = Saves::default();
    save(&mut saves, &m);
    let mut res = Resolution::new(&saves, "screen.design").unwrap();
    let before = res.scene().clone();
    saves.save("card.design", "<Frame fill={theme.nothing} />");
    assert!(matches!(
        res.update(&saves, &["card.design".to_string()]),
        Outcome::Problems(_)
    ));
    m.rows[0].2 = "While broken".into();
    m.rows.push((99, false, "Added while broken".into(), false));
    saves.save("screen.design", m.screen());
    assert!(matches!(
        res.update(&saves, &["screen.design".to_string()]),
        Outcome::Problems(_)
    ));
    same(res.scene(), &before);
    let mut browser = wire::decode_scene(
        &wire::scene(&before),
        (&Default::default(), &Default::default()),
    )
    .unwrap();
    let mut uses = Uses::of(&browser.root);
    saves.save("card.design", m.card());
    let Outcome::Changed(patch, _) = res.update(&saves, &["card.design".to_string()]) else {
        panic!()
    };
    let fresh = crate::resolve(&saves, "screen.design").unwrap();
    same(res.scene(), &fresh);
    wire::apply(&mut browser, &mut uses, &patch).unwrap();
    assert_eq!(wire::encode(&browser.root), wire::encode(&fresh.root));
}
