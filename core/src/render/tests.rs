use std::path::{Path, PathBuf};

use super::*;
use crate::addresses::{Owned, address};
use crate::document::Document;
use crate::folder::ServedFolder;
use crate::widgets::Widgets;

/// `document`'s widget set, read through a cache of its own.
fn widgets_of(document: &Document) -> Widgets {
    Widgets::of(document, &crate::widgets::Modules::default())
}

#[test]
fn an_image_of_a_design_becomes_its_canvas() {
    let html = markdown("Here:\n\n![The checkout](app/checkout.design)\n\n![A photo](photo.png)\n");
    assert!(
        html.contains(
            &format!(
                "<span class=\"design-embed\"><canvas class=\"design\" src=\"app/checkout.design\" role=\"img\" aria-label=\"The checkout\"></canvas>{}</span>",
                crate::navbar::design_theme_toggle()
            )
        ),
        "{html}"
    );
    assert!(html.contains("<img src=\"photo.png\" alt=\"A photo\" />"));
    // The design's script is loaded by the page script only when a page has one.
    assert!(!markdown("![A photo](photo.png)").contains("canvas"));
}

#[test]
fn a_raw_image_without_a_closing_slash_leaves_the_embed_after_it_working() {
    let html =
        markdown("<img src=\"logo.png\" width=\"40\">\n\nThen:\n\n![Checkout](checkout.design)\n");
    assert!(
        html.contains("<img src=\"logo.png\" width=\"40\">"),
        "{html}"
    );
    assert!(
        html.contains("<canvas class=\"design\" src=\"checkout.design\""),
        "{html}"
    );
    assert!(!html.contains("<img src=\"checkout.design\""), "{html}");
}

#[test]
fn a_raw_image_of_a_design_embeds_it_and_keeps_what_follows() {
    let html = markdown(
        "<img src=\"checkout.design\" alt=\"Raw\" width=\"300\">\n\nA paragraph.\n\n![Logo](logo.png)\n",
    );
    assert!(
        html.contains(
            "<canvas class=\"design\" src=\"checkout.design\" role=\"img\" aria-label=\"Raw\"></canvas>"
        ),
        "{html}"
    );
    assert!(html.contains("<p>A paragraph.</p>"), "{html}");
    assert!(
        html.contains("<img src=\"logo.png\" alt=\"Logo\" />"),
        "{html}"
    );
    // A `>` inside a quoted value doesn't end the tag.
    let html = markdown("<img alt=\"a > b\" src=\"x.design\">\n\nAfter.\n");
    assert!(html.contains("aria-label=\"a > b\""), "{html}");
    assert!(html.contains("<p>After.</p>"), "{html}");
}

#[test]
fn a_raw_image_of_a_design_is_found_however_its_attributes_are_written() {
    let canvas = |alt: &str| {
        format!(
            "<canvas class=\"design\" src=\"checkout.design\" role=\"img\" aria-label=\"{alt}\"></canvas>"
        )
    };
    for (source, alt) in [
        ("<img src='checkout.design' alt='Single'>", "Single"),
        (
            "<img\n  alt=\"New line\"\n  src=\"checkout.design\">",
            "New line",
        ),
        ("<img src=checkout.design alt=Bare>", "Bare"),
        ("<IMG ALT=\"Upper\" SRC=\"checkout.design\">", "Upper"),
        (
            "<img\tsrc=\"checkout.design\"\talt='Say \"hi\"'>",
            "Say &quot;hi&quot;",
        ),
    ] {
        let html = markdown(&format!("Before.\n\n{source}\n\nAfter.\n"));
        assert!(html.contains(&canvas(alt)), "{source}\n→ {html}");
        assert!(html.contains("<p>After.</p>"), "{source}\n→ {html}");
    }
    // `data-src` is not `src`.
    let html = markdown("<img data-src=\"checkout.design\" src=\"logo.png\">\n");
    assert!(!html.contains("canvas"), "{html}");
}

#[test]
fn keeps_raw_html_and_draws_mermaid_fences() {
    let html =
        markdown("<div class=\"note\">hi</div>\n\n```mermaid\nflowchart LR\n  A --> B\n```\n");
    assert!(html.contains("<div class=\"note\">hi</div>"));
    assert!(
        html.contains("<div class=\"diagram\"><svg xmlns=\"http://www.w3.org/2000/svg\" class=\"pele pele-flowchart\""),
        "{html}"
    );
    assert!(!html.contains("<pre lang=\"mermaid\">"), "{html}");
    // A diagram in code is an example, never drawn.
    let html = markdown("````markdown\n```mermaid\nflowchart LR\n  A --> B\n```\n````\n");
    assert!(!html.contains("<svg"), "{html}");
}

#[test]
fn a_diagram_that_does_not_parse_shows_its_error_and_its_source() {
    let html = markdown("```mermaid\nsequenceDiagram\n  A->>B: one; two\n```\n");
    assert!(
        html.contains("<pre class=\"diagram-error\">Diagram error: Parse error on line 2:"),
        "{html}"
    );
    assert!(
        html.contains("\n\nsequenceDiagram\n  A-&gt;&gt;B: one; two\n</pre>"),
        "{html}"
    );
    assert!(!html.contains("<svg"), "{html}");
}

#[test]
fn highlights_code_without_a_language_label() {
    let html = markdown("```rust\nfn main() { let x = \"hi\"; }\n```\n");
    assert!(
        html.contains("<pre><code>"),
        "no lang attribute or label: {html}"
    );
    assert!(html.contains("hl-keyword"), "{html}");
    assert!(html.contains("hl-string"), "{html}");
}

#[test]
fn leaves_unknown_languages_plain_and_escaped() {
    let html = markdown("```nosuchlang\na < b\n```\n");
    assert!(html.contains("<pre><code>a &lt; b"), "{html}");
    assert!(!html.contains("hl-"));
}

#[test]
fn headings_never_take_an_id_the_frame_uses() {
    // Every heading named like a part of devpage's own frame stays an
    // ordinary heading: its id can't be the frame's, so no frame style or
    // script ever lands on it.
    let headings: String = ["Comments", "Nav", "Page", "Navbar", "Theme corner"]
        .iter()
        .chain(
            FRAME_IDS
                .iter()
                .map(|id| &id[..])
                .collect::<Vec<_>>()
                .iter(),
        )
        .map(|name| format!("# {}\n\n", name.replace('-', " ")))
        .collect();
    let html = markdown(&headings);
    for id in FRAME_IDS {
        assert!(!html.contains(&format!("id=\"{id}\"")), "{id} in {html}");
    }
    assert!(html.contains("<h1 id=\"comments\">"), "{html}");
    assert!(html.contains("id=\"devpage-comments-heading\""), "{html}");
    assert!(
        html.contains("href=\"#devpage-comments-heading\""),
        "{html}"
    );
}

#[test]
fn renders_tables_and_heading_ids() {
    let html = markdown("## Two words\n\n| a | b |\n| - | - |\n| 1 | 2 |\n");
    assert!(html.contains("id=\"two-words\""));
    assert!(html.contains("<table>"));
}

#[test]
fn a_doc_renders_its_chapters_in_order_with_an_outline() {
    let dir = std::env::temp_dir().join(format!("devpage-render-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.md"),
        "# Context\n\n## Why\n\nSee [it](b.md#rollout), [here](#why), [web](https://x.y/a.md).",
    )
    .unwrap();
    std::fs::write(dir.join("b.md"), "# Rollout\n").unwrap();
    std::fs::write(
        dir.join("d.yml"),
        "kind: doc\ntitle: Plan\nchapters: [a.md, b.md]\n",
    )
    .unwrap();
    let document = Document::load(&dir.join("d.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
        panic!()
    };
    let html = doc(
        &document,
        &widgets_of(&document),
        chapters,
        pdf,
        false,
        true,
        &[],
    );
    let (context, rollout) = (
        html.find(">Context<").unwrap(),
        html.find(">Rollout<").unwrap(),
    );
    assert!(context < rollout);
    assert!(html.contains("href=\"#a--why\">Why</a>"), "{html}");
    assert!(html.contains("id=\"a--why\""), "{html}");
    assert!(html.contains("href=\"#b--rollout\""), "{html}");
    assert!(html.contains("href=\"https://x.y/a.md\""), "{html}");
    assert!(html.contains("size: A4"));
    let print = doc(
        &document,
        &widgets_of(&document),
        chapters,
        pdf,
        true,
        true,
        &[],
    );
    assert!(!print.contains("class=\"outline\""));
    assert!(print.contains("data-print"));
}

#[test]
fn a_deck_renders_one_section_per_slide_in_order() {
    let dir = std::env::temp_dir().join(format!("devpage-render-deck-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.md"), "# One\n\n<Notes>Say one.</Notes>\n").unwrap();
    std::fs::write(dir.join("two.md"), "# Two").unwrap();
    std::fs::write(
        dir.join("d.yml"),
        "kind: slides\ntitle: Talk\nslides: [two.md, one.md]\n",
    )
    .unwrap();
    let document = Document::load(&dir.join("d.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let crate::document::Kind::Slides { slides: files, .. } = &document.kind else {
        panic!()
    };
    let html = slides(&document, &widgets_of(&document), files, false, true, &[]);
    assert_eq!(html.matches("class=\"slide\"").count(), 2);
    assert!(html.find(">Two<").unwrap() < html.find(">One<").unwrap());
    assert!(html.contains("size: 1920px 1080px"));
    assert!(!html.contains("Say one."), "notes are never on the slide");
}

#[test]
fn chapters_and_slides_from_other_folders_keep_their_links() {
    let dir = std::env::temp_dir().join(format!("devpage-render-rebase-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("doc")).unwrap();
    std::fs::create_dir_all(dir.join("shared")).unwrap();
    std::fs::write(
        dir.join("shared/glossary.md"),
        "# Glossary\n\n![map](map.svg) [up](../README.md#top) [web](https://x.y/z) [here](#glossary)\n\n```mermaid\nflowchart LR\n  a\n  click a href \"terms.md\" \"Terms\"\n```\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("doc/d.yml"),
        "kind: doc\ntitle: Plan\nchapters: [../shared/glossary.md]\n",
    )
    .unwrap();
    let document =
        Document::load(&dir.join("doc/d.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
        panic!()
    };
    let html = doc(
        &document,
        &widgets_of(&document),
        chapters,
        pdf,
        false,
        true,
        &[],
    );
    // Served from `dir`, so addresses start at its top.
    let base = "";
    assert!(
        html.contains(&format!("src=\"{base}/shared/map.svg\"")),
        "{html}"
    );
    assert!(
        html.contains(&format!("href=\"{base}/README.md#top\"")),
        "{html}"
    );
    assert!(html.contains("href=\"https://x.y/z\""), "{html}");
    assert!(html.contains("href=\"#glossary\""), "{html}");
    // The diagram's link, drawn by the server, is rebased like the rest.
    assert!(
        html.contains(&format!(
            "<a href=\"{base}/shared/terms.md\" rel=\"noopener\">"
        )),
        "{html}"
    );
}

#[test]
fn a_one_file_doc_has_no_separate_title() {
    let dir = std::env::temp_dir().join(format!("devpage-render-one-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("design.md"), "# Design\n\nText.\n").unwrap();
    let document =
        Document::load(&dir.join("design.md"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
        panic!()
    };
    let html = doc(
        &document,
        &widgets_of(&document),
        chapters,
        pdf,
        false,
        true,
        &[],
    );
    assert!(!html.contains("doc-title\">"), "{html}");
    assert_eq!(html.matches("<h1").count(), 1, "{html}");
    assert_eq!(
        html.matches("id=\"design\"").count(),
        1,
        "ids are unique: {html}"
    );
}

#[test]
fn every_frame_carries_the_logo_as_its_tab_icon() {
    let dir = std::env::temp_dir().join(format!("devpage-render-logo-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("art")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("art/logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )
    .unwrap();
    std::fs::write(
        dir.join("README.md"),
        "# Home\n\n## Topics\n\n- [Page](page.md)\n",
    )
    .unwrap();
    std::fs::write(dir.join("page.md"), "# Page\n").unwrap();
    std::fs::write(
        dir.join("devpage.yml"),
        "kind: site\ntitle: Garden\nroot: README.md\nlogo: art/logo.svg\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("doc.yml"),
        "kind: doc\ntitle: Plan\nchapters: [page.md]\nlogo: art/logo.svg\n",
    )
    .unwrap();
    let icon = "<link rel=\"icon\" href=\"/art/logo.svg\">";
    let img = "<img class=\"logo\" src=\"/art/logo.svg\" alt=\"\">";
    // A site with no navbar: the logo heads the sidebar, on every page.
    let site = Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let html = page(
        &site,
        &widgets_of(&site),
        &dir.join("page.md"),
        "# Page\n",
        true,
        &[],
    );
    assert!(html.contains(icon), "{html}");
    assert!(html.contains(&format!("{img}Garden</a>")), "{html}");
    // A doc: before its title in the outline.
    let plan = Document::load(&dir.join("doc.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let crate::document::Kind::Doc { chapters, pdf } = &plan.kind else {
        panic!()
    };
    let html = doc(&plan, &widgets_of(&plan), chapters, pdf, false, true, &[]);
    assert!(html.contains(icon), "{html}");
    assert!(html.contains(&format!("{img}Plan</a>")), "{html}");
}

#[test]
fn with_comments_off_every_frame_carries_the_setting_and_no_switch() {
    let dir =
        std::env::temp_dir().join(format!("devpage-render-no-comments-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
    std::fs::write(
        dir.join("devpage.yml"),
        "kind: site\ntitle: S\nroot: README.md\nnavbar:\n  Home: README.md\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("d.yml"),
        "kind: doc\ntitle: D\nchapters: [README.md]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("s.yml"),
        "kind: slides\ntitle: T\nslides: [README.md]\n",
    )
    .unwrap();
    let site = Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let doc_file = Document::load(&dir.join("d.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let deck = Document::load(&dir.join("s.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let (
        crate::document::Kind::Doc { chapters, pdf },
        crate::document::Kind::Slides { slides: files, .. },
    ) = (&doc_file.kind, &deck.kind)
    else {
        panic!()
    };
    let frames = |comments| {
        [
            page(
                &site,
                &widgets_of(&site),
                &dir.join("README.md"),
                "# Home\n",
                comments,
                &[],
            ),
            doc(
                &doc_file,
                &widgets_of(&doc_file),
                chapters,
                pdf,
                false,
                comments,
                &[],
            ),
            slides(&deck, &widgets_of(&deck), files, false, comments, &[]),
        ]
    };
    let script = format!(
        "<script defer fetchpriority=\"high\" src=\"{}\" data-no-comments>",
        address(Owned::Asset("devpage.js"))
    );
    for html in frames(true) {
        assert!(html.contains("class=\"comment-mode\""), "{html}");
        assert!(!html.contains("data-no-comments"), "{html}");
    }
    for html in frames(false) {
        assert!(html.contains(&script), "{html}");
        assert!(!html.contains("comment-mode"), "{html}");
    }
    // Print mode shows no controls either way, and stays print mode.
    let print = slides(&deck, &widgets_of(&deck), files, true, false, &[]);
    assert!(print.contains("data-print") && !print.contains("data-no-comments"));
}

/// The `href` of every `<link rel="{rel}">` in a page's head.
fn linked(html: &str, rel: &str) -> Vec<String> {
    let head = &html[..html.find("</head>").unwrap()];
    head.split("<link ")
        .skip(1)
        .filter(|tag| tag.starts_with(&format!("rel=\"{rel}\"")))
        .map(|tag| {
            tag.split("href=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// A site of `files` (path, text) in a fresh folder, with a `widgets`
/// folder, and its `README.md` rendered as a page.
fn site_page(name: &str, readme: &str, files: &[(&str, &str)]) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("devpage-render-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("widgets")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("devpage.yml"),
        "kind: site\ntitle: S\nroot: README.md\nwidgets: widgets\n",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), readme).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    let site = Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let html = page(
        &site,
        &widgets_of(&site),
        &dir.join("README.md"),
        readme,
        true,
        &[],
    );
    (dir, html)
}

#[test]
fn a_diagram_page_comes_drawn_and_loads_nothing_for_it() {
    let (_, html) = site_page(
        "diagram",
        "# D\n\n```mermaid\nflowchart LR\n  A --> B\n```\n",
        &[],
    );
    assert!(html.contains("<div class=\"diagram\"><svg"), "{html}");
    assert_eq!(
        linked(&html, "modulepreload"),
        [
            address(Owned::Asset("react.js")),
            crate::widgets::system_address("DevpageSidebar", "")
        ],
        "{html}"
    );
    // The import map names no pele: the browser never runs it.
    assert!(!html.contains("\"pele\":"), "{html}");
}

#[test]
fn a_plain_page_loads_only_react_and_its_sidebar() {
    let (_, html) = site_page("plain", "# Plain\n\nJust text.\n", &[]);
    assert_eq!(
        linked(&html, "modulepreload"),
        [
            address(Owned::Asset("react.js")),
            crate::widgets::system_address("DevpageSidebar", "")
        ],
        "{html}"
    );
    assert!(linked(&html, "preload").is_empty(), "{html}");
    // The page script is in the head, deferred: fetched with the rest.
    let head = &html[..html.find("</head>").unwrap()];
    assert!(
        head.contains(&format!(
            "<script defer fetchpriority=\"high\" src=\"{}\"></script>",
            address(Owned::Asset("devpage.js"))
        )),
        "{head}"
    );
}

#[test]
fn a_widget_page_preloads_its_widgets_what_they_render_and_their_helpers() {
    let (dir, html) = site_page(
        "helpers",
        "# Deck\n\n<Card />\n",
        &[
            (
                "widgets/Card.tsx",
                "import { frame } from \"./frames\";\nexport default function Card() { return <Callout>{frame}</Callout>; }\n",
            ),
            ("widgets/frames.tsx", "export const frame = 'f';\n"),
            ("widgets/Unused.tsx", "export default () => <i />;\n"),
        ],
    );
    let modules = linked(&html, "modulepreload");
    let known: Vec<String> = widgets_of(
        &Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap(),
    )
    .modules
    .into_keys()
    .collect();
    let module = |file: &str| {
        crate::widgets::Modules::default()
            .snapshot(&ServedFolder::new(&dir).unwrap(), &dir.join(file))
            .address(&dir.join(file), &known)
    };
    let frames = module("widgets/frames.tsx");
    assert!(frames.contains("frames.tsx?v="), "{frames}");
    for expected in [
        address(Owned::Asset("react.js")),
        module("widgets/Card.tsx"),
        frames,
        crate::widgets::system_address("Callout", ""),
        crate::widgets::system_address("DevpageSidebar", ""),
    ] {
        assert!(modules.contains(&expected), "{expected} in {modules:?}");
    }
    assert!(!modules.iter().any(|m| m.contains("Unused")), "{modules:?}");
    assert_eq!(modules.len(), 5, "{modules:?}");
}

#[test]
fn a_design_page_preloads_the_design_chain_and_its_wasm_last() {
    let (_, html) = site_page("design", "# D\n\n![Checkout](checkout.design)\n", &[]);
    let modules = linked(&html, "modulepreload");
    assert_eq!(
        modules,
        [
            address(Owned::Asset("react.js")),
            crate::widgets::system_address("DevpageSidebar", ""),
            address(Owned::Asset("design.js")),
            address(Owned::Asset("design-stream.js")),
            address(Owned::Asset("design-canvas.js")),
            address(Owned::Asset("design-tiles.js")),
            address(Owned::Asset("design-engine.js"))
        ]
    );
    let wasm = format!(
        "<link rel=\"preload\" href=\"{}\" as=\"fetch\" crossorigin fetchpriority=\"low\">",
        address(Owned::Asset("design-engine.wasm"))
    );
    assert!(html.contains(&wasm), "{html}");
}

#[test]
fn the_sidebar_is_collapsed_with_the_current_branch_open_and_carries_its_tree() {
    let (dir, _) = site_page(
        "sidebar",
        "# Home\n\n## Topics\n\n- [Guide](guide.md)\n\n### Reference\n\n- [Fields](fields.md)\n",
        &[
            (
                "guide.md",
                "# Guide\n\n## Topics\n\n- [Step](step.md)\n- [Deep](deep.md)\n",
            ),
            ("deep.md", "# Deep\n\n## Topics\n\n- [Leaf](leaf.md)\n"),
            (
                "fields.md",
                "# Fields\n\n## Topics\n\n- [Field](field.md)\n",
            ),
            ("step.md", "# Step\n"),
            ("leaf.md", "# Leaf\n"),
            ("field.md", "# Field\n"),
        ],
    );
    let site = Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let html = page(
        &site,
        &widgets_of(&site),
        &dir.join("deep.md"),
        "# Deep\n",
        true,
        &[],
    );
    let nav = &html[html.find("<nav id=\"devpage-nav\"").unwrap()..];
    let nav = &nav[..nav.find("</nav>").unwrap() + 6];
    let url = |file: &str| site.url(&dir.join(file));
    let branch = |file: &str, label: &str, open: bool, current: bool| {
        format!(
            "<li class=\"branch{}\" data-href=\"{href}\"><div class=\"entry\"><a href=\"{href}\"{}>{label}</a><button type=\"button\" class=\"chevron\" aria-expanded=\"{open}\" aria-label=\"{label}\"></button></div>",
            if open { " open" } else { "" },
            if current { " class=\"current\"" } else { "" },
            href = url(file),
        )
    };
    // The branch down to the page shown is open, the page's own children
    // included; every other branch is closed.
    assert!(
        nav.contains(&branch("guide.md", "Guide", true, false)),
        "{nav}"
    );
    assert!(
        nav.contains(&branch("deep.md", "Deep", true, true)),
        "{nav}"
    );
    assert!(
        nav.contains(&branch("fields.md", "Fields", false, false)),
        "{nav}"
    );
    assert!(
        nav.contains(&format!("<li><a href=\"{}\">Step</a></li>", url("step.md"))),
        "a page with no children is a plain entry: {nav}"
    );
    assert!(
        nav.contains("<li class=\"group\"><span>Reference</span></li>"),
        "group headings stay where they are: {nav}"
    );
    assert!(
        !nav.contains('\n'),
        "no text between the elements React renders: {nav}"
    );
    assert!(
        nav.starts_with(&format!(
            "<nav id=\"devpage-nav\" data-sidebar=\"{}\">",
            site.entry()
        )),
        "{nav}"
    );
    // The search box sits between the title and the tree, empty.
    let search = nav
        .find("<div class=\"search\"><input type=\"search\"")
        .unwrap();
    assert!(
        nav.find("class=\"document-title").unwrap() < search && search < nav.find("<ul>").unwrap(),
        "{nav}"
    );
    // The tree, as JSON, for DevpageSidebar to take the sidebar over from.
    let data = html
        .split("<script type=\"application/json\" id=\"devpage-sidebar\">")
        .nth(1)
        .and_then(|rest| rest.split("</script>").next())
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(data["document"], site.entry());
    assert_eq!(data["document_path"], site.url(&dir.join("devpage.yml")));
    assert_eq!(data["current"], url("deep.md"));
    assert_eq!(data["title"]["label"], "S");
    assert_eq!(data["groups"][1]["title"], "Reference");
    assert_eq!(
        data["groups"][0]["entries"][0]["children"][0]["entries"][1]["href"],
        url("deep.md")
    );
    // DevpageSidebar is preloaded with React, though the content uses
    // no widget.
    let modules = linked(&html, "modulepreload");
    assert!(
        modules.contains(&crate::widgets::system_address("DevpageSidebar", "")),
        "{modules:?}"
    );
}

#[test]
fn a_site_with_sidebar_false_has_no_sidebar_and_loads_nothing_for_it() {
    let dir = std::env::temp_dir().join(format!("devpage-render-nosb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("devpage.yml"),
        "kind: site\ntitle: S\nroot: README.md\nsidebar: false\n",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
    let site = Document::load(&dir.join("devpage.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
    let html = page(
        &site,
        &widgets_of(&site),
        &dir.join("README.md"),
        "# Home\n",
        true,
        &[],
    );
    assert!(!html.contains("devpage-nav"), "{html}");
    assert!(!html.contains("devpage-sidebar"), "{html}");
    assert!(linked(&html, "modulepreload").is_empty(), "{html}");
}

#[test]
fn title_is_first_heading_or_file_name() {
    assert_eq!(
        title("intro\n# The vision\n", Path::new("vision.md")),
        "The vision"
    );
    assert_eq!(title("no heading", Path::new("notes.md")), "notes");
}

#[test]
fn tab_title_names_the_site_once() {
    // A page titled like its site shows the site's name alone.
    assert_eq!(tab_title("devpage", "devpage"), "devpage");
    // Any other page shows its own title first.
    assert_eq!(tab_title("Tests", "devpage"), "Tests · devpage");
    // Case and surrounding space make no second name.
    assert_eq!(tab_title(" DevPage ", "devpage"), "devpage");
}
