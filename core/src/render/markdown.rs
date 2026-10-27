//! Markdown to HTML: comrak with devpage's extensions, fenced code
//! highlighted, frame ids kept free, designs embedded and diagrams drawn.

use comrak::options::Plugins;
use comrak::{Options, markdown_to_html_with_plugins};

use crate::highlight::Highlighter;
use crate::markup::split_front_matter;
use crate::widgets::Widgets;

/// GitHub-flavoured Markdown with raw HTML allowed. Fenced code goes through
/// `highlight::Highlighter`, which keeps the language only on Mermaid blocks
/// (`<pre lang="mermaid">`), so `markdown` can find and draw them.
pub(crate) fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.header_id_prefix = Some(String::new());
    // A page may open with YAML front matter between `---` lines; it is
    // never shown (see `front_matter`).
    options.extension.front_matter_delimiter = Some("---".to_string());
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

/// Renders Markdown to HTML, highlighting fenced code (see `highlight`),
/// turning an image of a design into its canvas (see `embed_designs`) and
/// drawing each Mermaid fence (see `diagrams`).
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
    crate::diagrams::draw_fences(&embed_designs(&html))
}

/// Renders a page's Markdown, the tags of `widgets` expanded first.
pub fn markdown_in(widgets: &Widgets, source: &str) -> String {
    markdown(&crate::widgets::expand(
        source,
        widgets.names(),
        &mut Vec::new(),
    ))
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
                    "<span class=\"design-embed\"><canvas class=\"design\" src=\"{}\" role=\"img\" aria-label=\"{}\"></canvas>{}</span>",
                    src.replace('"', "&quot;"),
                    attribute("alt").unwrap_or_default().replace('"', "&quot;"),
                    crate::navbar::design_theme_toggle()
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

/// The YAML of a page's front matter: what lies between a `---` on its first
/// line and the next line that is `---`, if the page opens with one.
pub fn front_matter(source: &str) -> Option<&str> {
    split_front_matter(source).map(|(yaml, _)| yaml)
}
