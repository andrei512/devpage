use std::path::{Path, PathBuf};

use super::*;
use crate::document::Document;
use crate::folder::{Folder, ServedFolder};

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

/// `source`'s content as a page with no widgets shows it.
fn unwidgeted(source: &str) -> crate::page::Rendered {
    crate::page::content(
        &crate::widgets::Widgets::default(),
        Path::new("page.md"),
        source.to_string(),
    )
}

/// The folder opened on `docs/`, and the search index of what it publishes.
struct Indexed {
    folder: Folder,
    search: Search,
}

fn index(dir: &Path) -> Indexed {
    let entry = dir.join("docs/devpage.yml");
    let document = Document::load(&entry, &ServedFolder::new(dir).unwrap()).unwrap();
    let folder = Folder::new(std::sync::Arc::new(document));
    let search = Search::build(&folder, &entry);
    Indexed { folder, search }
}

fn find(index: &Indexed, query: &str) -> Answer {
    index.search.search(query)
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
    let plan = Document::load(
        &dir.join("plan/plan.yml"),
        &ServedFolder::new(&dir).unwrap(),
    )
    .unwrap();
    let ids = crate::page::ids(
        &plan.path(),
        &index.folder,
        &crate::widgets::Modules::default(),
    );
    assert!(ids.contains("how--context"), "{ids:?}");
    assert!(ids.contains("how"), "{ids:?}");
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
    let html = crate::page::markdown::markdown(source);
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
    let search = &index.search;
    let changed = |path: &Path| {
        index.folder.changed(&[path.to_path_buf()]);
        search.changed(&index.folder, &[path.to_path_buf()]);
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
    let found = read(&unwidgeted(source)).sections;
    let ids: Vec<_> = found.iter().map(|s| s.id.clone()).collect();
    let html = crate::page::markdown::markdown(source);
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
    let search = &index.search;
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

#[test]
fn a_save_reads_again_only_the_pages_it_touches() {
    let dir = served("touches");
    let index = index(&dir);
    let search = &index.search;
    // The tour changes on disk unheard, so whatever reads it again shows.
    let tour = dir.join("docs/features/tour.md");
    std::fs::write(&tour, "# Tour\n\nA narwhal tours it all.\n").unwrap();
    let layout = dir.join("docs/guide/layout.md");
    std::fs::write(
        &layout,
        "# Layout\n\n## The sidebar\n\nA heron stands still.\n",
    )
    .unwrap();
    index.folder.changed(std::slice::from_ref(&layout));
    search.changed(&index.folder, std::slice::from_ref(&layout));
    assert_eq!(
        section_urls(&search.search("heron")),
        ["/docs/guide/layout.md#the-sidebar"]
    );
    assert!(search.search("narwhal").pages.is_empty());
    assert_eq!(
        urls(&search.search("walrus")),
        ["/docs/features/features.yml"]
    );
}

/// The queries an index kept current must answer as one built fresh does:
/// words, a typo, a word being typed, a phrase, a title, code.
const ALIKE: &[&str] = &[
    "sidebar",
    "heron",
    "context",
    "welcome",
    "pelican",
    "quokka",
    "walrus",
    "tour",
    "guide",
    "layout",
    "plan",
    "results",
    "sidbar",
    "navig",
    "\"the sidebar\"",
    "handbook",
    "the",
    "lonely",
    "kingfisher",
    "otter",
    "zebra",
    "forever",
    "arrows",
    "numbers",
    "hello",
];

fn alike(index: &Indexed, dir: &Path, queries: &[&str], when: &str) {
    let fresh = Search::build(&index.folder, &dir.join("docs/devpage.yml"));
    for query in queries {
        assert_eq!(
            index.search.search(query),
            fresh.search(query),
            "{query:?} after {when}"
        );
    }
}

#[test]
fn an_index_kept_current_answers_as_one_built_fresh() {
    let dir = served("alike");
    let index = index(&dir);
    let write = |path: &str, text: &str| {
        let path = dir.join(path);
        std::fs::write(&path, text).unwrap();
        path
    };
    let changed = |paths: &[PathBuf], when: &str| {
        index.folder.changed(paths);
        index.search.changed(&index.folder, paths);
        alike(&index, &dir, ALIKE, when);
    };
    alike(&index, &dir, ALIKE, "the build");
    let layout = write(
        "docs/guide/layout.md",
        "# Layout\n\n## The sidebar\n\nA heron stands still in the sidebar.\n",
    );
    changed(&[layout], "a page's text changed");
    // A page first in the Topics moves every page after it in the nav, and
    // the guide's label changes its trail.
    let otter = write(
        "docs/otter.md",
        "# Otter\n\nAn otter floats in the sidebar.\n",
    );
    let readme = write(
        "docs/README.md",
        "# Handbook\n\nWelcome.\n\n## Topics\n\n- [Otter](otter.md)\n- [The guide](guide/README.md)\n- [The plan](../plan/plan.yml)\n- [The talk](../talk/talk.yml)\n- [Features](features/features.yml)\n- [Tour](features/tour.md)\n",
    );
    changed(&[otter, readme], "a page added first");
    let why = write(
        "plan/why.md",
        "# Reasons\n\n## Context\n\nUsers scroll forever.\n",
    );
    changed(&[why], "a chapter's title changed");
    let orphan = write(
        "docs/orphan.md",
        "---\nsearch: true\n---\n# Orphan\n\nA lonely heron.\n",
    );
    let tour = write(
        "docs/features/tour.md",
        "---\nsearch: false\n---\n# Tour\n\nA walrus tours it all.\n",
    );
    changed(&[orphan, tour], "front matter changed");
    let rule = write(".gitignore", "talk/\n");
    changed(std::slice::from_ref(&rule), "a rule excluded the talk");
    let how = dir.join("plan/how.md");
    std::fs::remove_file(&how).unwrap();
    changed(std::slice::from_ref(&how), "a chapter was deleted");
    write(
        "plan/how.md",
        "# How\n\n## Context\n\nA kingfisher index answers.\n",
    );
    std::fs::remove_file(&rule).unwrap();
    changed(&[how, rule], "a chapter and the talk came back");
    let guide = dir.join("docs/guide");
    std::fs::remove_dir_all(&guide).unwrap();
    changed(&[guide], "a folder was deleted");
}

/// Every page of the frozen docs saved in turn, unchanged: each is indexed
/// again behind every other, and every query answers as before.
#[test]
fn the_frozen_docs_answer_alike_after_every_page_is_saved() {
    const QUERIES: &[&str] = &[
        "design",
        "search",
        "glossary",
        "live updates",
        "comment store",
        "outbox",
        "sidebar",
        "front matter",
        "Search::changed",
        "desgin",
        "wid",
        "\"page script\"",
        "chrome -pdf",
        "how a page renders",
        "slides",
        "the",
        "version",
        "publish",
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/search-docs")
        .canonicalize()
        .unwrap();
    let entry = root.join("docs/devpage.yml");
    let document = Document::load(&entry, &ServedFolder::new(&root).unwrap()).unwrap();
    let folder = Folder::new(std::sync::Arc::new(document));
    let fresh = Search::build(&folder, &entry);
    let kept = Search::build(&folder, &entry);
    for page in markdown_files(&folder) {
        kept.changed(&folder, &[page]);
    }
    for query in QUERIES {
        assert_eq!(kept.search(query), fresh.search(query), "{query:?}");
    }
}

#[test]
fn a_section_is_at_an_id_the_check_lets_a_link_land_on() {
    let dir = std::env::temp_dir().join(format!("devpage-search-ids-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let served = ServedFolder::new(&dir).unwrap();
    let page = dir.join("page.md");
    let source = "# Use `code` *here*\n\n## Same\n\n## Same\n\n## devpage-page\n\n\
        ## [A link](x.md) & more\n\n<h2 id=\"raw\">Raw</h2>\n\n## Title[^1]\n\n[^1]: A note.\n\n\
        <Steps>\n## One\n## Two\n</Steps>\n\n<Nope>\n## Hidden\n</Nope>\n";
    std::fs::write(&page, source).unwrap();
    let document = Document::load(&page, &served).unwrap();
    let folder = Folder::new(std::sync::Arc::new(Document::load(&page, &served).unwrap()));
    let modules = crate::widgets::Modules::default();
    let rendered = crate::page::read(&crate::widgets::Widgets::of(&document, &modules), &page);
    let sections: Vec<String> = gather::read(&rendered)
        .sections
        .into_iter()
        .filter_map(|section| section.id)
        .collect();
    // A heading inside a widget is a section, as its page shows it; one
    // inside an unknown tag, which shows only its error, is not.
    assert_eq!(
        sections,
        [
            "use-code-here",
            "same",
            "same-1",
            "devpage-page-heading",
            "a-link--more",
            "title",
            "one",
            "two"
        ]
    );
    let landed = crate::page::ids(&page, &folder, &modules);
    for id in &sections {
        assert!(landed.contains(id), "{id} isn't an id of {landed:?}");
    }
    assert!(!landed.contains("hidden"), "{landed:?}");
}

#[test]
fn a_heading_inside_a_widget_is_found_and_one_inside_an_unknown_tag_is_not() {
    let dir = served("widget-headings");
    std::fs::write(
        dir.join("docs/guide/layout.md"),
        "# Layout\n\n<Steps>\n## Install the flamingo\nRun it.\n</Steps>\n\n\
         <Callout title=\"Heads up\" values={[\"rocket\"]} open>\nMind the step.\n</Callout>\n\n\
         <Nope>\n## Secret heron\nHidden capybara.\n</Nope>\n",
    )
    .unwrap();
    let index = index(&dir);
    assert_eq!(
        section_urls(&find(&index, "flamingo")),
        ["/docs/guide/layout.md#install-the-flamingo"]
    );
    // A widget's text props are prose; an expression's strings are not.
    assert!(!find(&index, "heads").pages.is_empty());
    assert!(find(&index, "rocket").pages.is_empty());
    // An unknown tag's content isn't shown, so it isn't searched, nor is
    // its error.
    assert!(find(&index, "heron").pages.is_empty());
    assert!(find(&index, "capybara").pages.is_empty());
    assert!(find(&index, "widget").pages.is_empty());
}

#[test]
fn a_widget_added_reads_again_the_pages_whose_tags_it_makes_widgets() {
    let dir = served("widget-added");
    std::fs::write(
        dir.join("docs/devpage.yml"),
        "kind: site\ntitle: Handbook\nroot: README.md\nwidgets: widgets\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
    std::fs::write(
        dir.join("docs/guide/layout.md"),
        "# Layout\n\n<Box>\n## The boxed wombat\n</Box>\n",
    )
    .unwrap();
    let index = index(&dir);
    assert!(find(&index, "wombat").pages.is_empty());
    let widget = dir.join("docs/widgets/Box.tsx");
    std::fs::write(
        &widget,
        "export default function Box({ children }) { return children; }\n",
    )
    .unwrap();
    index.search.changed(&index.folder, &[widget]);
    assert_eq!(
        section_urls(&find(&index, "wombat")),
        ["/docs/guide/layout.md#the-boxed-wombat"]
    );
}

/// What `pass` loads on this thread: each document file loaded, and each
/// document whose widgets were read, as often as it was, sorted.
fn loads(pass: impl FnOnce()) -> (Vec<PathBuf>, Vec<PathBuf>) {
    crate::document::LOADED.take();
    crate::widgets::READ.take();
    pass();
    let mut documents = crate::document::LOADED.take();
    let mut widgets = crate::widgets::READ.take();
    documents.sort();
    widgets.sort();
    (documents, widgets)
}

#[test]
fn a_rescan_loads_each_document_once_however_many_pages_it_shows() {
    let dir = served("loads-once");
    // The site shows more pages than the walk needs it for.
    for n in 0..5 {
        std::fs::write(
            dir.join(format!("docs/guide/extra-{n}.md")),
            format!("# Extra {n}\n\nMore.\n"),
        )
        .unwrap();
    }
    std::fs::write(
        dir.join("docs/guide/README.md"),
        "# Guide\n\nHow to read it.\n\n## Topics\n\n- [Layout](layout.md)\n- [0](extra-0.md)\n- [1](extra-1.md)\n- [2](extra-2.md)\n- [3](extra-3.md)\n- [4](extra-4.md)\n",
    )
    .unwrap();
    let documents: Vec<PathBuf> = [
        "docs/devpage.yml",
        "docs/features/features.yml",
        "plan/plan.yml",
        "talk/talk.yml",
    ]
    .iter()
    .map(|file| dir.join(file))
    .collect();
    let mut sorted = documents.clone();
    sorted.sort();
    let (loaded, read) = loads(|| drop(index(&dir)));
    // The test's own load of the entry, then the build's: each once.
    let mut expected = vec![dir.join("docs/devpage.yml")];
    expected.extend(sorted.clone());
    expected.sort();
    assert_eq!(loaded, expected, "a build loads each document once");
    assert_eq!(read, sorted, "a build reads each document's widgets once");

    let index = index(&dir);
    // A page's Topics are its footprint: what is published is found again.
    let layout = dir.join("docs/guide/layout.md");
    std::fs::write(
        &layout,
        "# Layout\n\nA heron stands still.\n\n## Topics\n\n- [Extra](extra-0.md)\n",
    )
    .unwrap();
    index.folder.changed(std::slice::from_ref(&layout));
    let (loaded, read) = loads(|| index.search.changed(&index.folder, &[layout]));
    assert_eq!(loaded, sorted, "a rescan loads each document once");
    assert_eq!(
        read,
        [dir.join("docs/devpage.yml")],
        "a page read again reads only its own document's widgets"
    );
    assert_eq!(
        section_urls(&find(&index, "heron")),
        ["/docs/guide/layout.md#layout"]
    );
}

/// What `pass` reads on this thread to find what is published: each
/// document file loaded, each page whose Topics it read, and each other
/// Markdown file it read, sorted.
fn walks(pass: impl FnOnce()) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    crate::document::LOADED.take();
    crate::nav::READ.take();
    WALKED.take();
    pass();
    let sorted = |mut files: Vec<PathBuf>| {
        files.sort();
        files
    };
    (
        sorted(crate::document::LOADED.take()),
        sorted(crate::nav::READ.take()),
        sorted(WALKED.take()),
    )
}

#[test]
fn a_save_that_leaves_what_is_published_walks_only_its_own_page() {
    let dir = served("walks");
    let index = index(&dir);
    let changed = |path: &Path| {
        index.folder.changed(&[path.to_path_buf()]);
        walks(|| index.search.changed(&index.folder, &[path.to_path_buf()]))
    };
    let layout = dir.join("docs/guide/layout.md");
    std::fs::write(
        &layout,
        "# Layout\n\nA heron stands still.\n\n## Navigation keys\n\nArrows move.\n",
    )
    .unwrap();
    let (loaded, nav, walked) = changed(&layout);
    assert_eq!(
        loaded,
        Vec::<PathBuf>::new(),
        "a body save loads no document"
    );
    assert_eq!(
        nav,
        Vec::<PathBuf>::new(),
        "a body save reads no page's Topics"
    );
    assert_eq!(
        walked,
        std::slice::from_ref(&layout),
        "a body save reads only its own page"
    );
    assert_eq!(
        section_urls(&find(&index, "heron")),
        ["/docs/guide/layout.md#layout"]
    );

    // A chapter's body: the same, its title read to see its trail stands.
    let how = dir.join("plan/how.md");
    std::fs::write(&how, "# How\n\n## Context\n\nA kingfisher index answers.\n").unwrap();
    let (loaded, nav, walked) = changed(&how);
    assert_eq!((loaded.len(), nav.len()), (0, 0));
    assert_eq!(walked, std::slice::from_ref(&how));
    assert_eq!(urls(&find(&index, "kingfisher")), ["/plan/plan.yml#how"]);

    // Nothing in a slide decides what is published, and finding that reads
    // nothing.
    let one = dir.join("talk/one.md");
    std::fs::write(&one, "# Hello\n\nWelcome, a narwhal.\n").unwrap();
    assert_eq!(changed(&one), (vec![], vec![], vec![]));
    assert_eq!(urls(&find(&index, "narwhal")), ["/talk/talk.yml#1"]);

    // A page out of the nav whose front matter now says `search: true` is
    // published: its footprint changed, and what is published is found again.
    let orphan = dir.join("docs/orphan.md");
    std::fs::write(
        &orphan,
        "---\nsearch: true\n---\n# Orphan\n\nA lonely heron.\n",
    )
    .unwrap();
    let (loaded, _, walked) = changed(&orphan);
    assert!(loaded.contains(&dir.join("docs/devpage.yml")), "{loaded:?}");
    assert!(walked.len() > 1, "{walked:?}");
    assert_eq!(urls(&find(&index, "lonely")), ["/docs/orphan.md"]);

    // So are a page's Topics: the guide no longer lists the layout.
    let guide = dir.join("docs/guide/README.md");
    std::fs::write(&guide, "# Guide\n\nHow to read it.\n\n## Topics\n\n").unwrap();
    let (loaded, nav, _) = changed(&guide);
    assert!(!loaded.is_empty());
    assert!(nav.contains(&guide), "{nav:?}");
    assert!(find(&index, "arrows").pages.is_empty());

    // And a chapter's title, which is its trail.
    let why = dir.join("plan/why.md");
    std::fs::write(&why, "# Reasons\n\n## Context\n\nUsers scroll forever.\n").unwrap();
    assert!(!changed(&why).0.is_empty());
    assert_eq!(find(&index, "forever").pages[0].trail, ["Reasons"]);
}

/// A small random number generator, the same sequence for the same seed.
struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// One random edit of the served folder `dir`, as a writer makes them: a
/// page's text, its title, its Topics or front matter, a page added or
/// deleted, a `.gitignore` rule, a doc's chapters. The paths it touched.
fn edit(dice: &mut Dice, dir: &Path, step: usize) -> Vec<PathBuf> {
    const WORDS: &[&str] = &["heron", "otter", "walrus", "quokka", "sidebar", "plan"];
    let markdown: Vec<PathBuf> = ignore::WalkBuilder::new(dir)
        .hidden(false)
        .build()
        .flatten()
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    let pages: Vec<&PathBuf> = markdown
        .iter()
        .filter(|path| path.starts_with(dir.join("docs")))
        .collect();
    // Half the time, a file whose footprint decides what is published: a
    // page out of the nav, a chapter, a page with Topics.
    let decisive: Vec<&PathBuf> = [
        "docs/orphan.md",
        "plan/why.md",
        "plan/how.md",
        "docs/README.md",
        "docs/guide/README.md",
    ]
    .iter()
    .filter_map(|file| markdown.iter().find(|path| path.ends_with(file)))
    .collect();
    let any = |dice: &mut Dice| -> PathBuf {
        if !decisive.is_empty() && dice.below(2) == 0 {
            dice.pick(&decisive).to_path_buf()
        } else {
            dice.pick(&markdown).clone()
        }
    };
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    let write = |path: &Path, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        path.to_path_buf()
    };
    let word = *dice.pick(WORDS);
    // A link from `page` to `target`, through the served folder's root.
    let link = |page: &Path, target: &Path| {
        let depth = page
            .parent()
            .unwrap()
            .strip_prefix(dir)
            .unwrap()
            .components()
            .count();
        format!(
            "{}{}",
            "../".repeat(depth),
            target.strip_prefix(dir).unwrap().display()
        )
    };
    match dice.below(12) {
        0..=2 if !markdown.is_empty() => {
            let file = &any(dice);
            vec![write(
                file,
                &format!("{}\n\nA {word} paragraph {step}.\n", read(file)),
            )]
        }
        3 if !markdown.is_empty() => {
            let file = &any(dice);
            let text = read(file);
            let text = match text.find("\n# ") {
                Some(at) => format!("{}\n# {word} {step}{}", &text[..at], &text[at + 2..]),
                None => format!("# {word} {step}\n\n{text}"),
            };
            vec![write(file, &text)]
        }
        4 | 5 if !markdown.is_empty() => {
            let file = &any(dice);
            let text = read(file);
            let body =
                crate::markup::split_front_matter(&text).map_or(text.as_str(), |(_, page)| page);
            let front = dice.pick(&[
                "search: true",
                "search: true",
                "search: false",
                "description: a",
            ]);
            vec![write(file, &format!("---\n{front}\n---\n{body}"))]
        }
        6 if !pages.is_empty() => {
            let page = *dice.pick(&pages);
            let targets: Vec<PathBuf> = pages
                .iter()
                .map(|page| page.to_path_buf())
                .chain([
                    dir.join("plan/plan.yml"),
                    dir.join("talk/talk.yml"),
                    dir.join(format!("docs/new-{step}.md")),
                ])
                .collect();
            let target = dice.pick(&targets);
            vec![write(
                page,
                &format!(
                    "{}\n## Topics\n\n- [{word} {step}]({})\n",
                    read(page),
                    link(page, target)
                ),
            )]
        }
        7 if !pages.is_empty() => {
            let page = *dice.pick(&pages);
            let text = read(page);
            let text = text
                .split("## Topics")
                .next()
                .unwrap_or_default()
                .to_string();
            vec![write(page, &text)]
        }
        8 => {
            let folder = *dice.pick(&["docs", "docs/guide", "plan", "talk"]);
            let file = dir.join(format!("{folder}/new-{step}.md"));
            let front = if dice.below(2) == 0 {
                "---\nsearch: true\n---\n"
            } else {
                ""
            };
            vec![write(
                &file,
                &format!("{front}# New {step}\n\nA new {word}.\n"),
            )]
        }
        9 if dice.below(4) == 0 => {
            let folder = dir.join(dice.pick(&["docs/guide", "docs/features", "talk"]));
            let _ = std::fs::remove_dir_all(&folder);
            vec![folder]
        }
        9 if !markdown.is_empty() => {
            let file = dice.pick(&markdown);
            std::fs::remove_file(file).unwrap();
            vec![file.clone()]
        }
        _ => {
            let file = dir.join(*dice.pick(&[
                ".gitignore",
                "plan/plan.yml",
                "docs/features/features.yml",
            ]));
            let text = match file.file_name().unwrap().to_str().unwrap() {
                ".gitignore" => dice
                    .pick(&["", "talk/\n", "docs/guide/\n", "plan/how.md\n"])
                    .to_string(),
                "plan.yml" => dice
                    .pick(&[
                        "kind: doc\ntitle: The plan\nchapters: [why.md, how.md]\n",
                        "kind: doc\ntitle: The plan\nchapters: [how.md, why.md]\n",
                        "kind: doc\ntitle: A plan\nchapters: [why.md]\n",
                    ])
                    .to_string(),
                _ => dice
                    .pick(&[
                        "kind: doc\ntitle: Features\nchapters: [tour.md]\n",
                        "kind: slides\ntitle: Features\nslides: [tour.md]\n",
                    ])
                    .to_string(),
            };
            vec![write(&file, &text)]
        }
    }
}

/// Random edits, one at a time and in bursts: after each, the index kept
/// current publishes the same pages, and answers every query, as one built
/// fresh.
#[test]
fn an_index_kept_current_through_random_edits_is_one_built_fresh() {
    for seed in [7, 42, 1234, 99991, 2026] {
        let dir = served(&format!("random-{seed}"));
        let index = index(&dir);
        let mut dice = Dice(seed);
        for step in 0..40 {
            let mut paths = edit(&mut dice, &dir, step);
            if dice.below(4) == 0 {
                paths.extend(edit(&mut dice, &dir, step + 1000));
            }
            index.folder.changed(&paths);
            index.search.changed(&index.folder, &paths);
            let fresh = Search::build(&index.folder, &dir.join("docs/devpage.yml"));
            assert_eq!(
                index.search.units(),
                fresh.units(),
                "seed {seed}, step {step}: {paths:?}"
            );
            for query in ALIKE {
                assert_eq!(
                    index.search.search(query),
                    fresh.search(query),
                    "seed {seed}, step {step}: {query:?} after {paths:?}"
                );
            }
        }
    }
}
