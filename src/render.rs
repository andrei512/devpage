//! Markdown to a full page: the rendered file inside the document's frame.

use std::path::{Path, PathBuf};

use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};

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
    let current = format!("/{}", relative(document, file).display());
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{page} · {document}</title>
<link rel="stylesheet" href="/__devpage/style.css">
</head>
<body>
<nav id="nav">
<a class="document-title{root_class}" href="/">{document}</a>
{nav}
</nav>
<main id="page"><article>{content}</article></main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"></script>
</body>
</html>"#,
        page = escape(&title(source, file)),
        document = escape(&document.title),
        nav = navigation(document, &current),
        root_class = if current == document.entry() {
            " current"
        } else {
            ""
        },
        content = markdown(source),
    )
}

/// A doc: every chapter on one page, in order, with an outline of the
/// chapters' headings beside them. In print mode the outline and the live
/// connection are left out and the page size is the doc's paper.
pub fn doc(document: &Document, chapters: &[PathBuf], pdf: &Pdf, print: bool) -> String {
    let mut content = String::new();
    let mut outline = String::new();
    for chapter in chapters {
        let source = std::fs::read_to_string(chapter).unwrap_or_default();
        let html = markdown(&source);
        for (level, id, text) in headings(&html) {
            outline.push_str(&format!(
                "<li class=\"h{level}\"><a href=\"#{id}\">{text}</a></li>"
            ));
        }
        content.push_str(&format!("<section class=\"chapter\">{html}</section>"));
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
<style>@page {{ size: {paper}; margin: 22mm 18mm; }}</style>
</head>
<body class="doc{print_class}">
{outline}
<main id="page"><article>{heading}{content}</article></main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"{live}></script>
</body>
</html>"#,
        title = escape(&document.title),
        // A lone Markdown file's title is its own first heading; don't repeat it.
        heading = if document.file_name.ends_with(".md") {
            String::new()
        } else {
            format!("<p class=\"doc-title\">{}</p>", escape(&document.title))
        },
        print_class = if print { " print" } else { "" },
        live = if print { " data-print" } else { "" },
    )
}

/// A deck: every slide on its own 1920 × 1080 stage, scaled to the window as a
/// whole, one shown at a time. In print mode each slide is one 16:9 page.
pub fn slides(document: &Document, slides: &[PathBuf], print: bool) -> String {
    let mut content = String::new();
    for (index, slide) in slides.iter().enumerate() {
        let source = std::fs::read_to_string(slide).unwrap_or_default();
        content.push_str(&format!(
            "<section class=\"slide\" data-slide=\"{}\"><div class=\"slide-body\">{}</div></section>",
            index + 1,
            markdown(&source)
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
<style>@page {{ size: 1920px 1080px; margin: 0; }}</style>
</head>
<body class="slides{print_class}">
<main id="page" class="deck">{content}</main>
<script src="/__devpage/pele.js"></script>
<script src="/__devpage/devpage.js"{live}></script>
</body>
</html>"#,
        title = escape(&document.title),
        print_class = if print { " print" } else { "" },
        live = if print { " data-print" } else { "" },
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
            let href = format!("/{}", relative(document, &entry.file).display());
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

fn relative(document: &Document, file: &Path) -> PathBuf {
    file.strip_prefix(&document.base)
        .unwrap_or(file)
        .to_path_buf()
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
        std::fs::write(dir.join("a.md"), "# Context\n\n## Why\n\ntext").unwrap();
        std::fs::write(dir.join("b.md"), "# Rollout\n").unwrap();
        std::fs::write(
            dir.join("d.yml"),
            "kind: doc\ntitle: Plan\nchapters: [a.md, b.md]\n",
        )
        .unwrap();
        let document = Document::load(&dir.join("d.yml")).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
            panic!()
        };
        let html = doc(&document, chapters, pdf, false);
        let (context, rollout) = (
            html.find(">Context<").unwrap(),
            html.find(">Rollout<").unwrap(),
        );
        assert!(context < rollout);
        assert!(html.contains("href=\"#why\">Why</a>"), "{html}");
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
        let document = Document::load(&dir.join("d.yml")).unwrap();
        let crate::document::Kind::Slides { slides: files } = &document.kind else {
            panic!()
        };
        let html = slides(&document, files, false);
        assert_eq!(html.matches("class=\"slide\"").count(), 2);
        assert!(html.find(">Two<").unwrap() < html.find(">One<").unwrap());
        assert!(html.contains("size: 1920px 1080px"));
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
