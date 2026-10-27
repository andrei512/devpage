//! Markdown to a full page: the rendered file inside the document's frame.

use std::path::{Path, PathBuf};

use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};

use crate::addresses::{Owned, address};
use crate::check;
use crate::document::{Document, Paper, Pdf, Slide};
use crate::highlight::Highlighter;
use crate::nav;

/// GitHub-flavoured Markdown with raw HTML allowed. Fenced code goes through
/// `highlight::Highlighter`, which keeps the language only on Mermaid blocks
/// (`<pre lang="mermaid">`), so the page can find and draw them.
pub(crate) fn options() -> Options<'static> {
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

/// The ids of devpage's own frame — the page, the sidebar and its tree, the
/// navbar, the theme corner and the comments — used by `assets/style.css` and
/// `assets/devpage.js`. They all start with `devpage-`, so a heading's id,
/// taken from its text, matches one only if the heading spells it out; such a
/// heading's id gets `-heading` added, so a frame id is never a heading's.
pub const FRAME_IDS: [&str; 8] = [
    "devpage-page",
    "devpage-nav",
    "devpage-sidebar",
    "devpage-navbar",
    "devpage-corner",
    "devpage-comments",
    "devpage-comments-toggle",
    "devpage-comment-mode",
];

/// Renders Markdown to HTML, highlighting fenced code (see `highlight`) and
/// turning an image of a design into its canvas (see `embed_designs`).
pub fn markdown(source: &str) -> String {
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&Highlighter);
    // A wide table scrolls inside its own box rather than widening the page.
    let mut html = markdown_to_html_with_plugins(source, &options(), &plugins)
        .replace("<table>", "<div class=\"table-scroll\"><table>")
        .replace("</table>", "</table></div>");
    for id in FRAME_IDS {
        if html.contains(&format!("id=\"{id}\"")) {
            html = html
                .replace(&format!("id=\"{id}\""), &format!("id=\"{id}-heading\""))
                .replace(
                    &format!("href=\"#{id}\""),
                    &format!("href=\"#{id}-heading\""),
                );
        }
    }
    embed_designs(&html)
}

/// Renders a page of `document`'s Markdown, its widget tags expanded first.
pub fn markdown_in(document: &Document, source: &str) -> String {
    let widgets = crate::widgets::Widgets::of(document);
    markdown(&crate::widgets::expand(source, &widgets, &mut Vec::new()))
}

/// `![Name](x.design)` embeds the design: comrak's `<img>` becomes the canvas
/// the design script draws on. The canvas keeps the link in `src`, so links
/// are rebased and checked as an image's are. A raw `<img>` in the page is an
/// image like any other, with or without a closing `/`.
fn embed_designs(html: &str) -> String {
    let mut out = String::new();
    // ASCII lowercasing keeps every byte where it was, so a position found in
    // `lower` is the same position in `html`.
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<img").map(|at| from + at) {
        out.push_str(&html[from..at]);
        let rest = &html[at..];
        if !rest[4..].starts_with(|c: char| c.is_ascii_whitespace()) {
            out.push_str(&rest[..4]);
            from = at + 4;
            continue;
        }
        let end = tag_end(rest);
        let tag = &rest[..end];
        let attributes = attributes(&tag[4..]);
        let attribute = |name: &str| {
            attributes
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        };
        match attribute("src") {
            Some(src)
                if src
                    .split(['?', '#'])
                    .next()
                    .is_some_and(|p| p.ends_with(".design")) =>
            {
                out.push_str(&format!(
                    "<span class=\"design-embed\"><canvas class=\"design\" src=\"{}\" role=\"img\" aria-label=\"{}\"></canvas></span>",
                    src.replace('"', "&quot;"),
                    attribute("alt").unwrap_or_default().replace('"', "&quot;")
                ));
            }
            _ => out.push_str(tag),
        }
        from = at + end;
    }
    out.push_str(&html[from..]);
    out
}

/// A tag's attributes, from just after its name up to its closing `>`: each
/// name with its value — double-quoted, single-quoted, unquoted, or empty for
/// a bare name — separated by any whitespace.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let mut chars = tag.char_indices().peekable();
    loop {
        while chars
            .next_if(|(_, c)| c.is_ascii_whitespace() || *c == '/')
            .is_some()
        {}
        let Some(&(start, c)) = chars.peek() else {
            break;
        };
        if c == '>' {
            break;
        }
        let mut end = start;
        while let Some((i, c)) =
            chars.next_if(|(_, c)| !c.is_ascii_whitespace() && !matches!(c, '=' | '>' | '/'))
        {
            end = i + c.len_utf8();
        }
        let name = tag[start..end].to_string();
        while chars.next_if(|(_, c)| c.is_ascii_whitespace()).is_some() {}
        let mut value = String::new();
        if chars.next_if(|(_, c)| *c == '=').is_some() {
            while chars.next_if(|(_, c)| c.is_ascii_whitespace()).is_some() {}
            match chars.peek() {
                Some(&(_, quote @ ('"' | '\''))) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == quote {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some((_, c)) =
                        chars.next_if(|(_, c)| !c.is_ascii_whitespace() && *c != '>')
                    {
                        value.push(c);
                    }
                }
            }
        }
        if name.is_empty() {
            // Not a name at all (a stray `=`): skip one character and go on.
            chars.next();
            continue;
        }
        attributes.push((name, value));
    }
    attributes
}

/// The length of the tag `html` starts with: up to and including its closing
/// `>`, skipping any `>` inside a quoted attribute value.
fn tag_end(html: &str) -> usize {
    let mut quote = None;
    for (i, c) in html.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return i + 1,
            _ => {}
        }
    }
    html.len()
}

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
/// the scripts that draw diagrams and keep it live. `comments` is whether
/// comments are on.
pub fn page(document: &Document, file: &Path, source: &str, comments: bool) -> String {
    let current = document.url(file);
    let content = markdown_in(document, source);
    format!(
        r#"<!doctype html>
<html lang="en">
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
        problems = problems(document, &[file.to_path_buf()]),
        widgets = crate::widgets::Widgets::of(document).head(
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
    chapters: &[PathBuf],
    pdf: &Pdf,
    print: bool,
    comments: bool,
) -> String {
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
            "<nav id=\"devpage-nav\" class=\"outline\"><a class=\"document-title current\" href=\"#\">{}{}</a><ul>{outline}</ul></nav>",
            crate::navbar::logo(document),
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
        stylesheet = address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        title = escape(&document.title),
        theme = crate::navbar::head(document),
        corner = if print {
            String::new()
        } else {
            crate::navbar::corner(document, comments)
        },
        widgets = crate::widgets::Widgets::of(document).head(&content, &[]),
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
{theme}
<link rel="stylesheet" href="{stylesheet}">
{page_script}
</head>
<body class="invalid" data-build="{build}">
<main id="devpage-page"><div class="problems"><b>{address} is not a valid document</b><ul><li>{error}</li></ul></div></main>
</body>
</html>"#,
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
pub fn slides(document: &Document, slides: &[Slide], print: bool, comments: bool) -> String {
    let files: Vec<PathBuf> = slides.iter().map(|slide| slide.file.clone()).collect();
    let mut content = String::new();
    for (index, slide) in files.iter().enumerate() {
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
        stylesheet = address(Owned::Asset("style.css")),
        build = crate::addresses::BUILD,
        title = escape(&document.title),
        theme = crate::navbar::head(document),
        corner = if print {
            String::new()
        } else {
            crate::navbar::corner(document, comments)
        },
        widgets = crate::widgets::Widgets::of(document).head(&content, &[]),
        print_class = if print { " print" } else { "" },
        script = page_script(print, comments),
        problems = if print {
            String::new()
        } else {
            problems(document, &files)
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

/// A site's sidebar, unless it sets `sidebar: false`: its title — left to the
/// navbar when there is one — then the tree its Topics sections form,
/// collapsed but for the branch down to the page shown. It is the markup
/// [`DevpageSidebar`](crate::widgets::SIDEBAR) renders from the same tree,
/// which follows it as JSON: the page script has React take the sidebar over
/// without changing a node. Before the page paints, a small script opens the
/// entries the reader opened on another visit, as the widget will.
fn sidebar(document: &Document, current: &str) -> String {
    if !document.sidebar {
        return String::new();
    }
    let groups = nav::tree(document);
    let title = document.navbar.is_empty().then(|| SidebarTitle {
        label: document.title.clone(),
        href: document.entry(),
        logo: document.logo_url(),
    });
    let mut html = format!(
        "<nav id=\"devpage-nav\" data-sidebar=\"{}\">",
        escape(&document.entry())
    );
    if let Some(title) = &title {
        html.push_str(&format!(
            "<a class=\"document-title{}\" href=\"{}\">{}{}</a>",
            if title.href == current {
                " current"
            } else {
                ""
            },
            escape(&title.href),
            crate::navbar::logo(document),
            escape(&title.label)
        ));
    }
    // The search box goes here, at the top of the sidebar, as it does in
    // `DevpageSidebar`.
    let groups: Vec<SidebarGroup> = groups
        .iter()
        .map(|g| SidebarGroup::of(document, g))
        .collect();
    tree(&groups, current, &mut html);
    html.push_str("</nav>");
    let data = SidebarData {
        document: document.entry(),
        current: current.to_string(),
        title,
        groups,
    };
    // `<` escaped, so no label can close the script.
    let json = serde_json::to_string(&data)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    html.push_str(&format!(
        "<script type=\"application/json\" id=\"devpage-sidebar\">{json}</script><script>{OPEN_REMEMBERED}</script>"
    ));
    html
}

/// Opens, before the page paints, the entries the reader opened on another
/// visit to the document, which `DevpageSidebar` keeps in `localStorage`
/// under `devpage-sidebar:` and the document's address. It changes only what
/// the widget's first render will, so React takes over the same DOM.
const OPEN_REMEMBERED: &str = "(()=>{const nav=document.getElementById(\"devpage-nav\");let opened;try{opened=JSON.parse(localStorage.getItem(\"devpage-sidebar:\"+nav.dataset.sidebar))}catch{}if(!Array.isArray(opened))return;for(const li of nav.querySelectorAll(\"li.branch:not(.open)\"))if(opened.includes(li.dataset.href)){li.classList.add(\"open\");li.querySelector(\".chevron\").setAttribute(\"aria-expanded\",\"true\")}})()";

/// What `DevpageSidebar` renders: the document it belongs to (its address,
/// which keys what the reader opened), the page shown, the title link when
/// the sidebar has one, and the tree.
#[derive(serde::Serialize)]
struct SidebarData {
    document: String,
    current: String,
    title: Option<SidebarTitle>,
    groups: Vec<SidebarGroup>,
}

#[derive(serde::Serialize)]
struct SidebarTitle {
    label: String,
    href: String,
    logo: Option<String>,
}

#[derive(serde::Serialize)]
struct SidebarGroup {
    title: Option<String>,
    entries: Vec<SidebarEntry>,
}

#[derive(serde::Serialize)]
struct SidebarEntry {
    label: String,
    href: String,
    children: Vec<SidebarGroup>,
}

impl SidebarGroup {
    fn of(document: &Document, group: &nav::Group) -> Self {
        Self {
            title: group.title.clone(),
            entries: group
                .entries
                .iter()
                .map(|entry| SidebarEntry {
                    label: entry.label.clone(),
                    href: document.url(&entry.file),
                    children: entry
                        .children
                        .iter()
                        .map(|g| SidebarGroup::of(document, g))
                        .collect(),
                })
                .collect(),
        }
    }
}

impl SidebarEntry {
    /// Whether the page shown is this entry or under it: the entries open
    /// when a page is shown.
    fn leads_to(&self, current: &str) -> bool {
        self.href == current
            || self
                .children
                .iter()
                .flat_map(|g| &g.entries)
                .any(|e| e.leads_to(current))
    }
}

/// The tree as nested `<ul>` lists: a group's title as a heading in place, an
/// entry with children as a branch — its link, a chevron that opens and
/// closes it, and its children's list, shown while open. No text sits
/// between elements: React renders none.
fn tree(groups: &[SidebarGroup], current: &str, html: &mut String) {
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
            let href = escape(&entry.href);
            let label = escape(&entry.label);
            let class = if entry.href == current {
                " class=\"current\""
            } else {
                ""
            };
            if entry.children.is_empty() {
                html.push_str(&format!("<li><a href=\"{href}\"{class}>{label}</a></li>"));
                continue;
            }
            let open = entry.leads_to(current);
            html.push_str(&format!(
                "<li class=\"branch{}\" data-href=\"{href}\"><div class=\"entry\"><a href=\"{href}\"{class}>{label}</a><button type=\"button\" class=\"chevron\" aria-expanded=\"{open}\" aria-label=\"{label}\"></button></div>",
                if open { " open" } else { "" },
            ));
            tree(&entry.children, current, html);
            html.push_str("</li>");
        }
    }
    html.push_str("</ul>");
}

pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_of_a_design_becomes_its_canvas() {
        let html =
            markdown("Here:\n\n![The checkout](app/checkout.design)\n\n![A photo](photo.png)\n");
        assert!(
            html.contains(
                "<span class=\"design-embed\"><canvas class=\"design\" src=\"app/checkout.design\" role=\"img\" aria-label=\"The checkout\"></canvas></span>"
            ),
            "{html}"
        );
        assert!(html.contains("<img src=\"photo.png\" alt=\"A photo\" />"));
        // The design's script is loaded by the page script only when a page has one.
        assert!(!markdown("![A photo](photo.png)").contains("canvas"));
    }

    #[test]
    fn a_raw_image_without_a_closing_slash_leaves_the_embed_after_it_working() {
        let html = markdown(
            "<img src=\"logo.png\" width=\"40\">\n\nThen:\n\n![Checkout](checkout.design)\n",
        );
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
        let document = Document::load(&dir.join("d.yml"), &dir).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &document.kind else {
            panic!()
        };
        let html = doc(&document, chapters, pdf, false, true);
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
        let print = doc(&document, chapters, pdf, true, true);
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
        let document = Document::load(&dir.join("d.yml"), &dir).unwrap();
        let crate::document::Kind::Slides { slides: files, .. } = &document.kind else {
            panic!()
        };
        let html = slides(&document, files, false, true);
        assert_eq!(html.matches("class=\"slide\"").count(), 2);
        assert!(html.find(">Two<").unwrap() < html.find(">One<").unwrap());
        assert!(html.contains("size: 1920px 1080px"));
        assert!(!html.contains("Say one."), "notes are never on the slide");
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
        let html = doc(&document, chapters, pdf, false, true);
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
        let html = doc(&document, chapters, pdf, false, true);
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
        let site = Document::load(&dir.join("devpage.yml"), &dir).unwrap();
        let html = page(&site, &dir.join("page.md"), "# Page\n", true);
        assert!(html.contains(icon), "{html}");
        assert!(html.contains(&format!("{img}Garden</a>")), "{html}");
        // A doc: before its title in the outline.
        let plan = Document::load(&dir.join("doc.yml"), &dir).unwrap();
        let crate::document::Kind::Doc { chapters, pdf } = &plan.kind else {
            panic!()
        };
        let html = doc(&plan, chapters, pdf, false, true);
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
        let site = Document::load(&dir.join("devpage.yml"), &dir).unwrap();
        let doc_file = Document::load(&dir.join("d.yml"), &dir).unwrap();
        let deck = Document::load(&dir.join("s.yml"), &dir).unwrap();
        let (
            crate::document::Kind::Doc { chapters, pdf },
            crate::document::Kind::Slides { slides: files, .. },
        ) = (&doc_file.kind, &deck.kind)
        else {
            panic!()
        };
        let frames = |comments| {
            [
                page(&site, &dir.join("README.md"), "# Home\n", comments),
                doc(&doc_file, chapters, pdf, false, comments),
                slides(&deck, files, false, comments),
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
        let print = slides(&deck, files, true, false);
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
        let dir =
            std::env::temp_dir().join(format!("devpage-render-{name}-{}", std::process::id()));
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
        let site = Document::load(&dir.join("devpage.yml"), &dir).unwrap();
        let html = page(&site, &dir.join("README.md"), readme, true);
        (dir, html)
    }

    #[test]
    fn a_diagram_page_preloads_the_diagram_widget_pele_and_react() {
        let (_, html) = site_page(
            "diagram",
            "# D\n\n```mermaid\nflowchart LR\n  A --> B\n```\n",
            &[],
        );
        let modules = linked(&html, "modulepreload");
        for owned in [
            Owned::Asset("react.js"),
            Owned::Asset("pele.js"),
            Owned::SystemWidget("Diagram"),
        ] {
            assert!(
                modules.contains(&address(owned)),
                "{owned:?} in {modules:?}"
            );
        }
        assert!(!html.contains("<script src=\"/__devpage/pele."), "{html}");
    }

    #[test]
    fn a_plain_page_loads_only_react_and_its_sidebar() {
        let (_, html) = site_page("plain", "# Plain\n\nJust text.\n", &[]);
        assert_eq!(
            linked(&html, "modulepreload"),
            [
                address(Owned::Asset("react.js")),
                address(Owned::SystemWidget("DevpageSidebar"))
            ],
            "{html}"
        );
        assert!(linked(&html, "preload").is_empty(), "{html}");
        assert!(!html.contains("<script src=\"/__devpage/pele."), "{html}");
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
        let known: Vec<String> =
            crate::widgets::Widgets::of(&Document::load(&dir.join("devpage.yml"), &dir).unwrap())
                .modules
                .into_keys()
                .collect();
        let module = |file: &str| {
            address(Owned::Module {
                snapshot: &crate::widgets::Snapshot::of(&dir, &dir.join(file)),
                file: &dir.join(file),
                known: &known,
            })
        };
        let frames = module("widgets/frames.tsx");
        assert!(frames.contains("frames.tsx?v="), "{frames}");
        for expected in [
            address(Owned::Asset("react.js")),
            module("widgets/Card.tsx"),
            frames,
            address(Owned::SystemWidget("Callout")),
            address(Owned::SystemWidget("DevpageSidebar")),
        ] {
            assert!(modules.contains(&expected), "{expected} in {modules:?}");
        }
        assert!(!modules.iter().any(|m| m.contains("Unused")), "{modules:?}");
        assert!(!modules.contains(&address(Owned::Asset("pele.js"))));
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
                address(Owned::SystemWidget("DevpageSidebar")),
                address(Owned::Asset("design.js")),
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
        let site = Document::load(&dir.join("devpage.yml"), &dir).unwrap();
        let html = page(&site, &dir.join("deep.md"), "# Deep\n", true);
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
        // The tree, as JSON, for DevpageSidebar to take the sidebar over from.
        let data = html
            .split("<script type=\"application/json\" id=\"devpage-sidebar\">")
            .nth(1)
            .and_then(|rest| rest.split("</script>").next())
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(data).unwrap();
        assert_eq!(data["document"], site.entry());
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
            modules.contains(&address(Owned::SystemWidget("DevpageSidebar"))),
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
        let site = Document::load(&dir.join("devpage.yml"), &dir).unwrap();
        let html = page(&site, &dir.join("README.md"), "# Home\n", true);
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
}
