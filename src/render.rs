//! Markdown to a full page: the rendered file inside the document's frame.

use std::path::{Path, PathBuf};

use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};

use crate::document::Document;
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
        root_class = if current == format!("/{}", document.root) {
            " current"
        } else {
            ""
        },
        content = markdown(source),
    )
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
    fn title_is_first_heading_or_file_name() {
        assert_eq!(
            title("intro\n# The vision\n", Path::new("vision.md")),
            "The vision"
        );
        assert_eq!(title("no heading", Path::new("notes.md")), "notes");
    }
}
