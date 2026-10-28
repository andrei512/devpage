//! The frame each kind of document is shown in: a site's page, a doc's
//! chapters on one page, a deck's slides, and the page for a document file
//! that can't be loaded — with the problems listed above the content.

use std::path::{Path, PathBuf};

use crate::addresses::{Owned, address};
use crate::document::{Document, Paper, Pdf, Slide};
use crate::problem::Problem;
use crate::served::ServedFolder;
use crate::widgets::Widgets;

use super::sidebar::sidebar;
use super::{escape, markdown_in, title};

/// A site page's tab title: `<page> · <site>`, or just the site's name when
/// the page is titled like it (trimmed, ignoring case).
pub fn tab_title(page: &str, site: &str) -> String {
    let (page, site) = (page.trim(), site.trim());
    if page.to_lowercase() == site.to_lowercase() {
        site.to_string()
    } else {
        format!("{page} · {site}")
    }
}

/// The page's `<html>` tag, carrying the served folder's mount when it has
/// one, for the page script and the design script to ask their endpoints
/// under.
pub fn html_open(served: &ServedFolder) -> String {
    match served.mount() {
        "" => r#"<html lang="en">"#.to_string(),
        mount => format!(r#"<html lang="en" data-mount="{}">"#, escape(mount)),
    }
}

/// The page script's tag, for the head: deferred, so it is fetched with
/// everything else the head names and runs once the page is parsed. In print mode it keeps no live connection and shows
/// no controls; with comments off (`devpage --no-comments`) it shows no
/// comment threads and offers no way to start one.
fn page_script(print: bool, comments: bool) -> String {
    let setting = if print {
        " data-print"
    } else if !comments {
        " data-no-comments"
    } else {
        ""
    };
    format!(
        "<script defer fetchpriority=\"high\" src=\"{}\"{setting}></script>",
        address(Owned::Asset("devpage.js"))
    )
}

/// The whole page for one Markdown file: navigation, the rendered content and
/// the scripts that mount its widgets and keep it live. `comments` is whether
/// comments are on.
/// `found` is the page's problems, listed above it.
pub fn page(
    document: &Document,
    widgets: &Widgets,
    file: &Path,
    source: &str,
    comments: bool,
    found: &[Problem],
) -> String {
    let current = document.url(file);
    let content = markdown_in(widgets, source);
    format!(
        r#"<!doctype html>
{html}
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{tab}</title>
{theme}
<link rel="stylesheet" href="{stylesheet}">
{script}
{widgets}
</head>
<body class="{body_class}" data-build="{build}">
{navbar}{sidebar}{corner}
<main id="devpage-page">{problems}<article data-source="{current}">{content}</article></main>
</body>
</html>"#,
        html = html_open(&document.served),
        stylesheet = address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        tab = escape(&tab_title(&title(source, file), &document.title)),
        theme = crate::navbar::head(document),
        body_class = site_class(document),
        navbar = crate::navbar::render(document, &current, comments),
        sidebar = sidebar(document, &current),
        script = page_script(false, comments),
        corner = if document.navbar.is_empty() {
            crate::navbar::corner(document, comments)
        } else {
            String::new()
        },
        problems = problems(found),
        widgets = widgets.head(
            &content,
            if document.sidebar {
                &[crate::widgets::SIDEBAR]
            } else {
                &[]
            }
        ),
    )
}

/// A doc: every chapter on one page, in order, with an outline of the
/// chapters' headings beside them. In print mode the outline and the live
/// connection are left out and the page size is the doc's paper.
pub fn doc(
    document: &Document,
    widgets: &Widgets,
    chapters: &[PathBuf],
    pdf: &Pdf,
    print: bool,
    comments: bool,
    found: &[Problem],
) -> String {
    let mut content = String::new();
    let mut outline = String::new();
    // Several chapters share one page, so each one's ids are prefixed with its
    // name, and links to a chapter or its headings point at those ids.
    let names: Vec<(PathBuf, String)> = chapters
        .iter()
        .map(|chapter| {
            let canonical = document
                .served
                .inside(chapter)
                .unwrap_or_else(|| chapter.clone());
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
        let mut html = markdown_in(widgets, &source);
        if chapters.len() > 1 {
            html = scope_ids(&html, chapter, &names, &document.served);
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
            format!(
                " id=\"{}\"",
                chapter_name(chapter, &names, &document.served)
            )
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
            "<nav id=\"devpage-nav\" class=\"outline\"><a class=\"document-title current\" href=\"#\">{}{}</a><ul>{outline}</ul></nav>",
            crate::navbar::logo(document),
            escape(&document.title)
        )
    };
    format!(
        r#"<!doctype html>
{html}
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
{theme}
<link rel="stylesheet" href="{stylesheet}">
{script}
{widgets}
<style>@page {{ size: {paper}; margin: 22mm 18mm; }}</style>
</head>
<body class="doc{print_class}" data-build="{build}">
{corner}{outline}
<main id="devpage-page">{problems}<article>{heading}{content}</article></main>
</body>
</html>"#,
        html = html_open(&document.served),
        stylesheet = address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        title = escape(&document.title),
        theme = crate::navbar::head(document),
        corner = if print {
            String::new()
        } else {
            crate::navbar::corner(document, comments)
        },
        widgets = widgets.head(&content, &[]),
        // A lone Markdown file's title is its own first heading; don't repeat it.
        heading = if document.file_name.ends_with(".md") {
            String::new()
        } else {
            format!("<p class=\"doc-title\">{}</p>", escape(&document.title))
        },
        print_class = if print { " print" } else { "" },
        script = page_script(print, comments),
        problems = if print {
            String::new()
        } else {
            problems(found)
        },
    )
}

fn chapter_name(chapter: &Path, names: &[(PathBuf, String)], served: &ServedFolder) -> String {
    let canonical = served
        .inside(chapter)
        .unwrap_or_else(|| chapter.to_path_buf());
    names
        .iter()
        .find(|(path, _)| *path == canonical)
        .map(|(_, name)| name.clone())
        .unwrap_or_default()
}

/// Prefixes a chapter's ids with its name and points links at the doc's ids:
/// `#why` → `#context--why`, `rollout.md#steps` → `#rollout--steps`,
/// `rollout.md` → `#rollout`.
fn scope_ids(
    html: &str,
    chapter: &Path,
    names: &[(PathBuf, String)],
    served: &ServedFolder,
) -> String {
    let own = chapter_name(chapter, names, served);
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
            let resolved = served.inside(&chapter.parent().unwrap_or(Path::new(".")).join(path));
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
    // Attributes — a diagram's links among them, drawn already.
    const STARTS: [(&str, &str); 2] = [(" href=\"", "\""), (" src=\"", "\"")];
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
{html}
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{address} · {document}</title>
{theme}
<link rel="stylesheet" href="{stylesheet}">
{page_script}
</head>
<body class="invalid" data-build="{build}">
<main id="devpage-page"><div class="problems"><b>{address} is not a valid document</b><ul><li>{error}</li></ul></div></main>
</body>
</html>"#,
        html = html_open(&entry.served),
        stylesheet = crate::addresses::address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        page_script = page_script(false, true),
        address = escape(&address),
        document = escape(&entry.title),
        theme = crate::navbar::head(entry),
        error = escape(error),
    )
}

/// The page's problems, listed above it, with each broken link marked.
fn problems(found: &[Problem]) -> String {
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
pub fn slides(
    document: &Document,
    widgets: &Widgets,
    slides: &[Slide],
    print: bool,
    comments: bool,
    found: &[Problem],
) -> String {
    let files: Vec<PathBuf> = slides.iter().map(|slide| slide.file.clone()).collect();
    let mut content = String::new();
    for (index, slide) in files.iter().enumerate() {
        let source = std::fs::read_to_string(slide).unwrap_or_default();
        content.push_str(&format!(
            "<section class=\"slide\" data-slide=\"{}\" data-source=\"{}\"><div class=\"slide-body\">{}</div></section>",
            index + 1,
            document.url(slide),
            rebase(&markdown_in(widgets, &source), slide, document)
        ));
    }
    format!(
        r#"<!doctype html>
{html}
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
{theme}
<link rel="stylesheet" href="{stylesheet}">
{script}
{widgets}
<style>@page {{ size: 1920px 1080px; margin: 0; }}</style>
</head>
<body class="slides{print_class}" data-build="{build}">
{corner}<main id="devpage-page" class="deck">{problems}{content}</main>
</body>
</html>"#,
        html = html_open(&document.served),
        stylesheet = address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        title = escape(&document.title),
        theme = crate::navbar::head(document),
        corner = if print {
            String::new()
        } else {
            crate::navbar::corner(document, comments)
        },
        widgets = widgets.head(&content, &[]),
        print_class = if print { " print" } else { "" },
        script = page_script(print, comments),
        problems = if print {
            String::new()
        } else {
            problems(found)
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

/// A site page's body classes, which name its frame: a page with another frame
/// can't be swapped in part.
fn site_class(document: &Document) -> String {
    let mut class = String::from("site");
    if !document.navbar.is_empty() {
        class.push_str(" with-navbar");
    }
    if !document.sidebar {
        class.push_str(" no-sidebar");
    }
    class
}
