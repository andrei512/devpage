use std::path::{Path, PathBuf};

use super::*;
use crate::document::Document;

/// A served folder: a site whose Topics list a guide, a doc of two
/// chapters and a deck with notes; a doc of one chapter whose chapter
/// the site lists as a page too; a page no nav lists; and a page in a
/// folder `.gitignore` excludes.
pub(crate) fn served(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for folder in ["docs/guide", "docs/build", "docs/features", "plan", "talk"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    let write = |path: &str, text: &str| std::fs::write(dir.join(path), text).unwrap();
    write(
        "docs/devpage.yml",
        "kind: site\ntitle: Handbook\nroot: README.md\n",
    );
    write(
        "docs/README.md",
        "# Handbook\n\nWelcome.\n\n## Topics\n\n- [Guide](guide/README.md)\n- [The plan](../plan/plan.yml)\n- [The talk](../talk/talk.yml)\n- [Features](features/features.yml)\n- [Tour](features/tour.md)\n",
    );
    write(
        "docs/guide/README.md",
        "# Guide\n\nHow to read it.\n\n## Topics\n\n- [Layout](layout.md)\n",
    );
    write(
        "docs/guide/layout.md",
        "# Layout\n\nThe page has a header.\n\n## The sidebar\n\nThe sidebar lists every page, and the reader opens a branch to see deeper.\n\n## Navigation keys\n\nArrows move between pages.\n",
    );
    write(
        "docs/features/features.yml",
        "kind: doc\ntitle: Features\nchapters: [tour.md]\n",
    );
    write(
        "docs/features/tour.md",
        "# Tour\n\nA walrus tours it all.\n",
    );
    write("docs/orphan.md", "# Orphan\n\nA lonely heron.\n");
    write("docs/.gitignore", "build/\n");
    write("docs/build/out.md", "# Out\n\nGenerated zebra text.\n");
    write(
        "plan/plan.yml",
        "kind: doc\ntitle: The plan\nchapters: [why.md, how.md]\n",
    );
    write(
        "plan/why.md",
        "# Why\n\n## Context\n\nUsers scroll forever.\n",
    );
    write(
        "plan/how.md",
        "# How\n\n## Context\n\nA quokka index answers.\n",
    );
    write(
        "talk/talk.yml",
        "kind: slides\ntitle: The talk\nslides: [one.md, two.md]\n",
    );
    write("talk/one.md", "# Hello\n\nWelcome, everyone.\n");
    write(
        "talk/two.md",
        "<Slide title=\"Results\">\n\nNumbers.\n\n</Slide>\n\n<Notes>\nSay that the pelican flew home.\n</Notes>\n",
    );
    dir.canonicalize().unwrap()
}

fn index(dir: &Path) -> Index {
    Index::build(dir).with_search(&dir.join("docs/devpage.yml"))
}

fn find(index: &Index, query: &str) -> Answer {
    index.search().unwrap().search(query)
}

fn urls(answer: &Answer) -> Vec<&str> {
    answer.pages.iter().map(|page| page.url.as_str()).collect()
}

fn section_urls(answer: &Answer) -> Vec<&str> {
    answer
        .pages
        .iter()
        .flat_map(|page| &page.sections)
        .map(|section| section.url.as_str())
        .collect()
}

#[test]
fn a_section_is_found_by_a_word_in_its_body() {
    let dir = served("body");
    let index = index(&dir);
    let answer = find(&index, "branch");
    assert_eq!(urls(&answer), ["/docs/guide/layout.md"]);
    let page = &answer.pages[0];
    assert_eq!(page.title, "Layout");
    assert_eq!(page.kind, PageKind::Page);
    assert!(!page.title_match);
    assert_eq!(page.lede, "The page has a header.");
    assert_eq!(
        page.document,
        Owner {
            title: "Handbook".into(),
            path: "/docs/devpage.yml".into()
        }
    );
    assert_eq!(page.trail, ["Guide", "Layout"]);
    assert_eq!(page.more, 0);
    let section = &page.sections[0];
    assert_eq!(section.url, "/docs/guide/layout.md#the-sidebar");
    assert_eq!(section.heading, "The sidebar");
    assert_eq!(section.path, ["Layout", "The sidebar"]);
    assert_eq!(section.words, ["branch"]);
    assert_eq!(
        section.snippet,
        "The sidebar lists every page, and the reader opens a <mark>branch</mark> to see deeper."
    );
}

#[test]
fn only_what_is_published_is_searched() {
    let dir = served("scope");
    let index = index(&dir);
    // A page no nav lists, and a page out of sight.
    assert!(find(&index, "heron").pages.is_empty());
    assert!(find(&index, "zebra").pages.is_empty());
    // Front matter puts an unlisted page in, and takes a listed one out.
    std::fs::write(
        dir.join("docs/orphan.md"),
        "---\nsearch: true\n---\n\n# Orphan\n\nA lonely heron.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("docs/guide/layout.md"),
        "---\nsearch: false\n---\n# Layout\n\nA branch.\n",
    )
    .unwrap();
    let index = super::tests::index(&dir);
    let answer = find(&index, "heron");
    assert_eq!(urls(&answer), ["/docs/orphan.md"]);
    assert_eq!(answer.pages[0].title, "Orphan");
    assert_eq!(answer.pages[0].document.path, "/docs/devpage.yml");
    assert!(find(&index, "branch").pages.is_empty());
}

#[test]
fn a_chapter_listed_as_a_page_too_is_found_once_as_a_chapter() {
    let dir = served("once");
    let index = index(&dir);
    let answer = find(&index, "walrus");
    assert_eq!(urls(&answer), ["/docs/features/features.yml"]);
    assert_eq!(answer.pages[0].kind, PageKind::Chapter);
    assert_eq!(section_urls(&answer), ["/docs/features/features.yml#tour"]);
}

#[test]
fn a_doc_and_a_deck_are_searched_at_their_own_addresses() {
    let dir = served("kinds");
    let index = index(&dir);
    // Two chapters: each heading's id carries its chapter's name, as rendered.
    let answer = find(&index, "quokka");
    assert_eq!(urls(&answer), ["/plan/plan.yml#how"]);
    assert_eq!(section_urls(&answer), ["/plan/plan.yml#how--context"]);
    let page = &answer.pages[0];
    assert_eq!(page.kind, PageKind::Chapter);
    assert_eq!(page.trail, ["How"]);
    assert_eq!(page.document.path, "/plan/plan.yml");
    let plan = Document::load(&dir.join("plan/plan.yml"), &dir).unwrap();
    let html = crate::render::doc(
        &plan,
        &crate::widgets::Widgets::of(&plan, &Default::default()),
        &[dir.join("plan/why.md"), dir.join("plan/how.md")],
        &Default::default(),
        false,
        true,
        &[],
    );
    assert!(html.contains("id=\"how--context\""), "{html}");
    assert!(html.contains("id=\"how\""), "{html}");
    // A slide's notes are searchable, and lead to its slide.
    let answer = find(&index, "pelican");
    assert_eq!(urls(&answer), ["/talk/talk.yml#2"]);
    assert_eq!(section_urls(&answer), ["/talk/talk.yml#2"]);
    let page = &answer.pages[0];
    assert_eq!(page.kind, PageKind::Slide);
    assert_eq!(page.title, "Results");
    assert_eq!(page.document.title, "The talk");
}

#[test]
fn a_page_whose_title_is_the_query_comes_first() {
    let dir = served("title");
    std::fs::write(
        dir.join("docs/guide/README.md"),
        "# Guide\n\nThe layout, the layout and the layout of it.\n\n## Topics\n\n- [Layout](layout.md)\n",
    )
    .unwrap();
    let index = index(&dir);
    for query in ["layout", "Layout", "layo"] {
        let answer = find(&index, query);
        assert_eq!(answer.pages[0].url, "/docs/guide/layout.md", "{query}");
        assert!(answer.pages[0].title_match, "{query}");
        assert!(urls(&answer).contains(&"/docs/guide/README.md"));
    }
    assert!(!find(&index, "layout keys").pages[0].title_match);
}

#[test]
fn a_typo_still_finds_a_word_the_index_lacks() {
    let dir = served("typo");
    let index = index(&dir);
    let answer = find(&index, "sidbar");
    assert!(
        section_urls(&answer).contains(&"/docs/guide/layout.md#the-sidebar"),
        "{answer:#?}"
    );
    let section = &answer.pages[0].sections[0];
    assert!(
        section.snippet.contains("<mark>sidebar</mark>"),
        "{section:#?}"
    );
    assert_eq!(section.words, ["sidebar"]);
    // Short words must be exact: three letters may not be one off.
    assert!(find(&index, "hox").pages.is_empty());
    // A word the index holds is never taken for a near one.
    assert!(
        find(&index, "pages")
            .pages
            .iter()
            .all(|page| page.url != "/talk/talk.yml#1")
    );
}

#[test]
fn the_last_word_is_matched_while_it_is_typed() {
    let dir = served("prefix");
    let index = index(&dir);
    let answer = find(&index, "navig");
    assert_eq!(
        section_urls(&answer),
        ["/docs/guide/layout.md#navigation-keys"]
    );
    // Matched by its heading: the snippet is the first sentence, nothing marked.
    assert_eq!(
        answer.pages[0].sections[0].snippet,
        "Arrows move between pages."
    );
    assert_eq!(answer.pages[0].sections[0].words, ["Navigation"]);
    assert_eq!(
        section_urls(&find(&index, "arrows mov")),
        ["/docs/guide/layout.md#navigation-keys"]
    );
    // A word followed by a space is whole.
    assert!(!find(&index, "mov").pages.is_empty());
    assert!(find(&index, "mov ").pages.is_empty());
}

#[test]
fn every_word_must_match_unless_none_do_together() {
    let dir = served("words");
    let index = index(&dir);
    // Both words are in one section only.
    assert_eq!(
        section_urls(&find(&index, "reader branch")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
    // No section holds both: any word, the more of them the better.
    let answer = find(&index, "quokka pelican");
    assert_eq!(answer.pages.len(), 2, "{answer:#?}");
    // A stopword need not match; a query of only stopwords finds them.
    assert_eq!(
        section_urls(&find(&index, "the branch")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
    assert!(!find(&index, "the").pages.is_empty());
}

#[test]
fn a_quoted_phrase_must_match_in_a_row_and_a_minus_word_never() {
    let dir = served("phrase");
    let index = index(&dir);
    assert_eq!(
        section_urls(&find(&index, "\"reader opens\"")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
    assert!(find(&index, "\"opens reader\"").pages.is_empty());
    assert!(
        section_urls(&find(&index, "pages")).contains(&"/docs/guide/layout.md#navigation-keys")
    );
    let without = find(&index, "pages -arrows");
    assert!(
        !section_urls(&without).contains(&"/docs/guide/layout.md#navigation-keys"),
        "{without:#?}"
    );
    assert!(find(&index, "-arrows").pages.is_empty());
}

#[test]
fn a_page_shows_three_sections_and_counts_the_rest() {
    let dir = served("group");
    let mut page = String::from("# Otters\n\nAn otter page.\n");
    for n in 1..=5 {
        page.push_str(&format!("\n## Part {n}\n\nAn otter swims.\n"));
    }
    std::fs::write(dir.join("docs/guide/layout.md"), page).unwrap();
    let index = index(&dir);
    let answer = find(&index, "otter");
    assert_eq!(answer.pages.len(), 1);
    assert_eq!(answer.pages[0].sections.len(), 3);
    assert_eq!(answer.pages[0].more, 3);
    assert!(answer.pages[0].title_match, "Otters is the title");
}

#[test]
fn at_most_ten_pages_answer() {
    let dir = served("ten");
    let mut topics = String::from("# Guide\n\n## Topics\n\n");
    for n in 0..12 {
        topics.push_str(&format!("- [P{n}](p{n}.md)\n"));
        std::fs::write(
            dir.join(format!("docs/guide/p{n}.md")),
            format!("# P{n}\n\nA marmot.\n"),
        )
        .unwrap();
    }
    std::fs::write(dir.join("docs/guide/README.md"), topics).unwrap();
    let index = index(&dir);
    assert_eq!(find(&index, "marmot").pages.len(), MAX_PAGES);
}

#[test]
fn code_and_diagrams_are_their_own_kinds_of_text() {
    let dir = served("code");
    std::fs::write(
        dir.join("docs/guide/layout.md"),
        "# Layout\n\nThe engine draws it.\n```mermaid\nflowchart LR\n  a[\"Quick fox\"] -->|jumps| lazyDog\n  click a href \"x.md\"\n```\n\n## Calls\n\nCall `Index::with_search` once.\n\n```rust\nlet tweakScore = build_index();\n```\n\n<Callout title=\"Heads up\" icon=\"rocket\">\nMind the step.\n</Callout>\n",
    )
    .unwrap();
    let index = index(&dir);
    // A diagram's labels are prose; its source is not text at all.
    let answer = find(&index, "fox");
    let section = &answer.pages[0].sections[0];
    assert!(
        section.snippet.contains("Quick <mark>fox</mark>"),
        "{section:#?}"
    );
    assert!(!section.snippet.contains("flowchart"), "{section:#?}");
    assert!(!section.snippet.contains("draws it.Quick"), "{section:#?}");
    assert!(find(&index, "lazydog").pages.is_empty());
    assert!(find(&index, "flowchart").pages.is_empty());
    // Code is found by its identifiers whole and by their parts, and
    // never shows in a snippet.
    for query in [
        "Index::with_search",
        "with_search",
        "tweakScore",
        "tweak",
        "build_index",
    ] {
        assert_eq!(
            section_urls(&find(&index, query)),
            ["/docs/guide/layout.md#calls"],
            "{query}"
        );
    }
    let answer = find(&index, "tweak");
    assert!(!answer.pages[0].sections[0].snippet.contains("tweak"));
    // As the text spells it: the word the query's start matched.
    assert_eq!(answer.pages[0].sections[0].words, ["tweakScore"]);
    // A widget's text attributes are prose; its others are not.
    assert_eq!(
        section_urls(&find(&index, "heads")),
        ["/docs/guide/layout.md#calls"]
    );
    assert!(find(&index, "rocket").pages.is_empty());
}

#[test]
fn front_matter_is_read_and_never_shown() {
    let source = "---\nsearch: true\ndescription: All about otters.\nkeywords: [river, swim]\n---\n# Otters\n\nText.\n";
    let front = FrontMatter::read(source);
    assert_eq!(front.search, Some(true));
    assert_eq!(front.meta, "All about otters. river swim");
    assert_eq!(crate::markup::title(source, Path::new("x.md")), "Otters");
    let html = crate::render::markdown(source);
    assert!(!html.contains("search"), "{html}");
    assert!(html.contains("<h1"), "{html}");
    let dir = served("meta");
    std::fs::write(
        dir.join("docs/guide/layout.md"),
        format!("{}\n## More\n\nNothing here.\n", source),
    )
    .unwrap();
    let index = index(&dir);
    assert_eq!(urls(&find(&index, "river")), ["/docs/guide/layout.md"]);
}

#[test]
fn an_empty_query_finds_nothing() {
    let dir = served("empty");
    let index = index(&dir);
    assert!(find(&index, "").pages.is_empty());
    assert!(find(&index, "  \t ").pages.is_empty());
    assert!(find(&index, "\"\"").pages.is_empty());
}

#[test]
fn an_odd_query_never_fails() {
    let dir = served("odd");
    let index = index(&dir);
    let long = "sidebar ".repeat(100);
    for query in [
        "\"",
        "-",
        "--",
        "\"unclosed phrase",
        "::",
        "_",
        "a_",
        "ÄÖü ß",
        "-sidebar",
        "\"the\"",
        "sidebar\"branch\"",
        "a::b::",
        "😀 sidebar",
        &long,
    ] {
        let answer = find(&index, query);
        assert!(answer.pages.len() <= MAX_PAGES, "{query}");
    }
    assert_eq!(
        section_urls(&find(&index, "sidebar\"branch\"")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
}

#[test]
fn changes_keep_it_current() {
    let dir = served("changes");
    let index = index(&dir);
    let search = index.search().unwrap();
    let changed = |path: &Path| {
        index.changed(path);
        search.changed(&index, &[path.to_path_buf()]);
    };
    let layout = dir.join("docs/guide/layout.md");
    std::fs::write(
        &layout,
        "# Layout\n\n## The sidebar\n\nA heron stands still.\n",
    )
    .unwrap();
    changed(&layout);
    assert_eq!(
        section_urls(&search.search("heron")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
    assert!(search.search("branch").pages.is_empty());

    // The guide no longer lists the page: it leaves the index.
    let guide = dir.join("docs/guide/README.md");
    std::fs::write(&guide, "# Guide\n\nHow to read it.\n").unwrap();
    changed(&guide);
    assert!(search.search("heron").pages.is_empty());

    // A chapter of the doc goes away: the doc is invalid, with no entries.
    let how = dir.join("plan/how.md");
    std::fs::remove_file(&how).unwrap();
    changed(&how);
    assert!(search.search("quokka").pages.is_empty());
    assert!(search.search("forever").pages.is_empty());

    // A new rule excludes the talk: its slides leave the index.
    let rule = dir.join(".gitignore");
    std::fs::write(&rule, "talk/\n").unwrap();
    changed(&rule);
    assert!(search.search("pelican").pages.is_empty());
    assert_eq!(urls(&search.search("welcome")), ["/docs/README.md"]);
}

#[test]
fn a_section_split_keeps_rendered_ids_and_sorts_text() {
    let source = "Intro text.\n\n# Comments\n\nx\n\n## Two `words`\n\ny\n\n## Two words\n\nBefore.\n```\ncode here\n```\n";
    let found = read(source).sections;
    let ids: Vec<_> = found.iter().map(|s| s.id.clone()).collect();
    let html = crate::render::markdown(source);
    for id in ids.iter().flatten() {
        assert!(html.contains(&format!("id=\"{id}\"")), "{id} in {html}");
    }
    assert_eq!(found[0].prose, "Intro text.");
    assert_eq!(found[2].heading, "Two words");
    assert_eq!(found[2].level, 2);
    assert_eq!(found[3].prose, "Before.");
    assert_eq!(found[3].code, "code here");
}

#[test]
fn a_snippet_is_cut_at_sentences() {
    let dir = served("snippet");
    let index = index(&dir);
    let search = index.search().unwrap();
    let wanted = search.parse("otter");
    let words = wanted.matched();
    let prose = "Rivers run. An otter swims fast. Fish hide.";
    assert_eq!(
        search.snippet(prose, &words),
        "An <mark>otter</mark> swims fast."
    );
    assert_eq!(
        search.snippet("No match here. None.", &words),
        "No match here."
    );
    let long = format!("{} otter {}.", "word ".repeat(60), "word ".repeat(60));
    let snippet = search.snippet(&long, &words);
    assert!(
        snippet.starts_with('…') && snippet.ends_with('…'),
        "{snippet}"
    );
    assert!(snippet.contains("<mark>otter</mark>"), "{snippet}");
}

#[test]
fn code_splits_into_identifiers_and_their_parts() {
    assert_eq!(
        expand_code("Search::changed(tweakScore, HTMLParser) x_1"),
        "search::changed search changed tweakscore tweak score htmlparser html parser x_1 x 1"
    );
    assert!(is_identifier("with_search"));
    assert!(is_identifier("Index::build"));
    assert!(is_identifier("camelCase"));
    assert!(!is_identifier("plain"));
    assert!(!is_identifier("x-forwarded-email"));
}

#[test]
fn near_matches_count_a_swap_as_one_edit() {
    assert_eq!(distance("sidebar", "sidbar"), 1);
    assert_eq!(distance("sidebar", "sideabr"), 1);
    assert_eq!(distance("layout", "layuot"), 1);
    assert_eq!(distance("", "abc"), 3);
}
