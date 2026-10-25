//! Markdown to a full page: the rendered file inside the document's frame.

use std::path::{Path, PathBuf};

use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};

use crate::check;
use crate::document::{Document, Paper, Pdf};
use crate::highlight::Highlighter;
use crate::nav;

/// GitHub-flavoured Markdown with raw HTML allowed. Fenced code goes through
/// `highlight::Highlighter`, which keeps the language only on Mermaid blocks
/// (`<pre lang="mermaid">`), so the page can find and draw them.
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.header_id_prefix = Some(String::new());
    options.render.r#unsafe = true;
    options.render.github_pre_lang = true;
    options
}

/// Renders Markdown to HTML, highlighting fenced code (see `highlight`).
pub fn markdown(source: &str) -> String {
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&Highlighter);
    markdown_to_html_with_plugins(source, &options(), &plugins)
}

/// Renders a page of `document`'s Markdown, its widget tags expanded first.
pub fn markdown_in(document: &Document, source: &str) -> String {
    let widgets = crate::widgets::Widgets::of(document);
    markdown(&crate::widgets::expand(source, &widgets, &mut Vec::new()))
}

/// A page's title: its first level-one heading, or its file name.
pub fn title(source: &str, path: &Path) -> String {
    source
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|heading| heading.trim().to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

/// The whole page for one Markdown file: navigation, the rendered content and
/// the scripts that draw diagrams and keep it live.
pub fn page(document: &Document, file: &Path, source: &str) -> String {
    let current = document.url(file);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{page} · {document}</title>
<link rel="stylesheet" href="/__devpage/style.css">
{widgets}
</head>
<body>
<nav id="nav">
<a class="document-title{root_class}" href="{home}">{document}</a>
{nav}
</nav>
<main id="page">{problems}<article data-source="{current}">{content}</article></main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"></script>
</body>
</html>"#,
        page = escape(&title(source, file)),
        document = escape(&document.title),
        nav = navigation(document, &current),
        home = escape(&document.entry()),
        root_class = if current == document.entry() {
            " current"
        } else {
            ""
        },
        content = markdown_in(document, source),
        problems = problems(document, &[file.to_path_buf()]),
        widgets = crate::widgets::Widgets::of(document).head(),
    )
}

/// A doc: every chapter on one page, in order, with an outline of the
/// chapters' headings beside them. In print mode the outline and the live
/// connection are left out and the page size is the doc's paper.
pub fn doc(document: &Document, chapters: &[PathBuf], pdf: &Pdf, print: bool) -> String {
    let mut content = String::new();
    let mut outline = String::new();
    // Several chapters share one page, so each one's ids are prefixed with its
    // name, and links to a chapter or its headings point at those ids.
    let names: Vec<(PathBuf, String)> = chapters
        .iter()
        .map(|chapter| {
            let canonical = chapter.canonicalize().unwrap_or_else(|_| chapter.clone());
            let name = chapter
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            (canonical, name)
        })
        .collect();
    for chapter in chapters {
        let source = std::fs::read_to_string(chapter).unwrap_or_default();
        let mut html = markdown_in(document, &source);
        if chapters.len() > 1 {
            html = scope_ids(&html, chapter, &names);
        }
        let html = rebase(&html, chapter, document);
        for (level, id, text) in headings(&html) {
            outline.push_str(&format!(
                "<li class=\"h{level}\"><a href=\"#{id}\">{text}</a></li>"
            ));
        }
        // A link to a chapter lands on its section, which only needs an id
        // when there is more than one — and then can't clash with a heading's,
        // which carries the chapter's prefix.
        let id = if chapters.len() > 1 {
            format!(" id=\"{}\"", chapter_name(chapter, &names))
        } else {
            String::new()
        };
        content.push_str(&format!(
            "<section class=\"chapter\"{id} data-source=\"{}\">{html}</section>",
            document.url(chapter)
        ));
    }
    let paper = match pdf.paper {
        Paper::A4 => "A4",
        Paper::Letter => "letter",
    };
    let outline = if print {
        String::new()
    } else {
        format!(
            "<nav id=\"nav\" class=\"outline\"><a class=\"document-title current\" href=\"#\">{}</a><ul>{outline}</ul></nav>",
            escape(&document.title)
        )
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/__devpage/style.css">
{widgets}
<style>@page {{ size: {paper}; margin: 22mm 18mm; }}</style>
</head>
<body class="doc{print_class}">
{outline}
<main id="page">{problems}<article>{heading}{content}</article></main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"{live}></script>
</body>
</html>"#,
        title = escape(&document.title),
        widgets = crate::widgets::Widgets::of(document).head(),
        // A lone Markdown file's title is its own first heading; don't repeat it.
        heading = if document.file_name.ends_with(".md") {
            String::new()
        } else {
            format!("<p class=\"doc-title\">{}</p>", escape(&document.title))
        },
        print_class = if print { " print" } else { "" },
        live = if print { " data-print" } else { "" },
        problems = if print {
            String::new()
        } else {
            problems(document, chapters)
        },
    )
}

fn chapter_name(chapter: &Path, names: &[(PathBuf, String)]) -> String {
    let canonical = chapter
        .canonicalize()
        .unwrap_or_else(|_| chapter.to_path_buf());
    names
        .iter()
        .find(|(path, _)| *path == canonical)
        .map(|(_, name)| name.clone())
        .unwrap_or_default()
}

/// Prefixes a chapter's ids with its name and points links at the doc's ids:
/// `#why` → `#context--why`, `rollout.md#steps` → `#rollout--steps`,
/// `rollout.md` → `#rollout`.
fn scope_ids(html: &str, chapter: &Path, names: &[(PathBuf, String)]) -> String {
    let own = chapter_name(chapter, names);
    let html = html.replace(" id=\"", &format!(" id=\"{own}--"));
    let mut out = String::new();
    let mut rest = html.as_str();
    while let Some(at) = rest.find(" href=\"") {
        out.push_str(&rest[..at + 7]);
        rest = &rest[at + 7..];
        let end = rest.find('"').unwrap_or(rest.len());
        let href = &rest[..end];
        let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
        let target = if path.is_empty() {
            Some(own.clone())
        } else if href.contains("://") {
            None
        } else {
            let resolved = chapter
                .parent()
                .unwrap_or(Path::new("."))
                .join(path)
                .canonicalize()
                .ok();
            names
                .iter()
                .find(|(file, _)| Some(file) == resolved.as_ref())
                .map(|(_, name)| name.clone())
        };
        match target {
            Some(name) if fragment.is_empty() => out.push_str(&format!("#{name}")),
            Some(name) => out.push_str(&format!("#{name}--{fragment}")),
            None => out.push_str(href),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Points a file's relative links and images at their served addresses. A
/// doc or a deck is served at its document file's address, so a chapter or
/// slide from another folder would otherwise resolve its links against the
/// wrong one.
fn rebase(html: &str, file: &Path, document: &Document) -> String {
    let folder = file.parent().unwrap_or(Path::new("."));
    let mut out = String::new();
    let mut rest = html;
    // Attributes, and a Mermaid `click … href "…"` line, which is still
    // source here — the browser draws the diagram later.
    const STARTS: [(&str, &str); 3] = [(" href=\"", "\""), (" src=\"", "\""), (" href \"", "\"")];
    loop {
        let next = STARTS
            .iter()
            .filter_map(|(start, end)| rest.find(start).map(|at| (at + start.len(), *end)))
            .min();
        let Some((start, close)) = next else { break };
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(close).unwrap_or(rest.len());
        let value = &rest[..end];
        if value.is_empty() || value.starts_with(['#', '/']) || value.contains(':') {
            out.push_str(value);
        } else {
            let (path, fragment) = match value.split_once('#') {
                Some((path, fragment)) => (path, format!("#{fragment}")),
                None => (value, String::new()),
            };
            out.push_str(&document.url(&normalize(&folder.join(path))));
            out.push_str(&fragment);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Resolves `.` and `..` in a path without touching the disk, so a link to a
/// file that doesn't exist yet still gets an address (and a problem).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// A document file that can't be loaded: a page listing what is wrong with it.
pub fn invalid(entry: &Document, file: &Path, error: &str) -> String {
    let address = entry.url(file);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{address} · {document}</title>
<link rel="stylesheet" href="/__devpage/style.css">
</head>
<body class="invalid">
<main id="page"><div class="problems"><b>{address} is not a valid document</b><ul><li>{error}</li></ul></div></main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"></script>
</body>
</html>"#,
        address = escape(&address),
        document = escape(&entry.title),
        error = escape(error),
    )
}

/// The page's problems, listed above it, with each broken link marked.
fn problems(document: &Document, pages: &[PathBuf]) -> String {
    let found = check::preview(document, pages);
    if found.is_empty() {
        return String::new();
    }
    let items: String = found
        .iter()
        .map(|problem| format!("<li>{}</li>", escape(&problem.to_string())))
        .collect();
    // The links as written, for the page script to mark where they stand.
    let broken: Vec<&str> = found.iter().filter_map(|p| p.target.as_deref()).collect();
    format!(
        "<div class=\"problems\" data-broken=\"{}\"><b>{} problem{}</b><ul>{items}</ul></div>",
        escape(&serde_json::to_string(&broken).unwrap_or_default()),
        found.len(),
        if found.len() == 1 { "" } else { "s" }
    )
}

/// A deck: every slide on its own 1920 × 1080 stage, scaled to the window as a
/// whole, one shown at a time. In print mode each slide is one 16:9 page.
pub fn slides(document: &Document, slides: &[PathBuf], print: bool) -> String {
    let mut content = String::new();
    for (index, slide) in slides.iter().enumerate() {
        let source = std::fs::read_to_string(slide).unwrap_or_default();
        content.push_str(&format!(
            "<section class=\"slide\" data-slide=\"{}\" data-source=\"{}\"><div class=\"slide-body\">{}</div></section>",
            index + 1,
            document.url(slide),
            rebase(&markdown_in(document, &source), slide, document)
        ));
    }
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/__devpage/style.css">
{widgets}
<style>@page {{ size: 1920px 1080px; margin: 0; }}</style>
</head>
<body class="slides{print_class}">
<main id="page" class="deck">{problems}{content}</main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"{live}></script>
</body>
</html>"#,
        title = escape(&document.title),
        widgets = crate::widgets::Widgets::of(document).head(),
        print_class = if print { " print" } else { "" },
        live = if print { " data-print" } else { "" },
        problems = if print {
            String::new()
        } else {
            problems(document, slides)
        },
    )
}

/// The `h1` and `h2` headings of rendered HTML: level, id and text.
fn headings(html: &str) -> Vec<(u8, String, String)> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<h") {
        rest = &rest[start + 2..];
        let level = rest.as_bytes().first().copied().unwrap_or(0);
        if level != b'1' && level != b'2' {
            continue;
        }
        let close = format!("</h{}>", level as char);
        let Some(end) = rest.find(&close) else { break };
        let inner = &rest[..end];
        let id = inner
            .split("id=\"")
            .nth(1)
            .and_then(|tail| tail.split('"').next())
            .unwrap_or_default()
            .to_string();
        let text = strip_tags(inner.split_once('>').map_or(inner, |(_, body)| body));
        found.push((level - b'0', id, text));
        rest = &rest[end..];
    }
    found
}

fn strip_tags(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.trim().to_string()
}

/// The sidebar: the document's title, then the tree the pages' Topics sections form.
fn navigation(document: &Document, current: &str) -> String {
    let mut html = String::new();
    tree(document, &nav::tree(document), current, &mut html);
    html
}

fn tree(document: &Document, groups: &[nav::Group], current: &str, html: &mut String) {
    if groups.is_empty() {
        return;
    }
    html.push_str("<ul>");
    for group in groups {
        if let Some(title) = &group.title {
            html.push_str(&format!(
                "<li class=\"group\"><span>{}</span></li>",
                escape(title)
            ));
        }
        for entry in &group.entries {
            let href = document.url(&entry.file);
            let class = if href == current {
                " class=\"current\""
            } else {
                ""
            };
            html.push_str(&format!(
                "<li><a href=\"{href}\"{class}>{}</a>",
                escape(&entry.label)
            ));
            tree(document, &entry.children, current, html);
            html.push_str("</li>");
        }
    }
    html.push_str("</ul>");
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_raw_html_and_marks_mermaid_fences() {
        let html =
            markdown("<div class=\"note\">hi</div>\n\n```mermaid\nflowchart LR\n  A --> B\n```\n");
        assert!(html.contains("<div class=\"note\">hi</div>"));
        assert!(html.contains("<pre lang=\"mermaid\">"));
        assert!(html.contains("A --&gt; B"));
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
        let document = Document::load(&dir.join("d.yml"), &dir).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
            panic!()
        };
        let html = doc(&document, chapters, pdf, false);
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
        let print = doc(&document, chapters, pdf, true);
        assert!(!print.contains("class=\"outline\""));
        assert!(print.contains("data-print"));
    }

    #[test]
    fn a_deck_renders_one_section_per_slide_in_order() {
        let dir = std::env::temp_dir().join(format!("devpage-render-deck-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one.md"), "# One").unwrap();
        std::fs::write(dir.join("two.md"), "# Two").unwrap();
        std::fs::write(
            dir.join("d.yml"),
            "kind: slides\ntitle: Talk\nslides: [two.md, one.md]\n",
        )
        .unwrap();
        let document = Document::load(&dir.join("d.yml"), &dir).unwrap();
        let crate::document::Kind::Slides { slides: files } = &document.kind else {
            panic!()
        };
        let html = slides(&document, files, false);
        assert_eq!(html.matches("class=\"slide\"").count(), 2);
        assert!(html.find(">Two<").unwrap() < html.find(">One<").unwrap());
        assert!(html.contains("size: 1920px 1080px"));
    }

    #[test]
    fn chapters_and_slides_from_other_folders_keep_their_links() {
        let dir =
            std::env::temp_dir().join(format!("devpage-render-rebase-{}", std::process::id()));
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
        let document = Document::load(&dir.join("doc/d.yml"), &dir).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
            panic!()
        };
        let html = doc(&document, chapters, pdf, false);
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
        assert!(
            html.contains(&format!("href \"{base}/shared/terms.md\"")),
            "{html}"
        );
    }

    #[test]
    fn a_one_file_doc_has_no_separate_title() {
        let dir = std::env::temp_dir().join(format!("devpage-render-one-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("design.md"), "# Design\n\nText.\n").unwrap();
        let document = Document::load(&dir.join("design.md"), &dir).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
            panic!()
        };
        let html = doc(&document, chapters, pdf, false);
        assert!(!html.contains("doc-title\">"), "{html}");
        assert_eq!(html.matches("<h1").count(), 1, "{html}");
        assert_eq!(
            html.matches("id=\"design\"").count(),
            1,
            "ids are unique: {html}"
        );
    }

    #[test]
    fn title_is_first_heading_or_file_name() {
        assert_eq!(
            title("intro\n# The vision\n", Path::new("vision.md")),
            "The vision"
        );
        assert_eq!(title("no heading", Path::new("notes.md")), "notes");
    }
}
