//! Widgets: React components used as capitalized HTML tags in Markdown.
//!
//! On the server a page's widget tags become placeholder elements carrying the
//! widget's name, its props' source and its content, which comrak renders as
//! Markdown. In the browser the page script loads React and the widgets,
//! compiled here from TSX by oxc, and mounts each placeholder.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{JsxRuntime, TransformOptions, Transformer};

use crate::document::Document;

/// The widgets devpage ships, compiled into the binary.
pub const SYSTEM: &[(&str, &str)] = &[
    ("Callout", include_str!("../assets/widgets/Callout.tsx")),
    ("Cards", include_str!("../assets/widgets/Cards.tsx")),
    ("Diagram", include_str!("../assets/widgets/Diagram.tsx")),
    ("Example", include_str!("../assets/widgets/Example.tsx")),
    ("FileTree", include_str!("../assets/widgets/FileTree.tsx")),
    ("Keys", include_str!("../assets/widgets/Keys.tsx")),
    ("Steps", include_str!("../assets/widgets/Steps.tsx")),
    ("Tab", include_str!("../assets/widgets/Tab.tsx")),
    ("Tabs", include_str!("../assets/widgets/Tabs.tsx")),
    ("Terminal", include_str!("../assets/widgets/Terminal.tsx")),
];

/// The tag that holds a slide's notes: devpage's own, never a widget. It is
/// taken out of the page and its text kept (see [`notes`]).
pub const NOTES: &str = "Notes";

/// System widgets that show their own source: they also get it highlighted
/// as Markdown, as the `sourceHtml` prop.
const SHOWS_SOURCE: &[&str] = &["Example"];

/// The widgets a document's pages can use: name → the address its module is
/// served at.
#[derive(Debug, Default)]
pub struct Widgets {
    pub modules: BTreeMap<String, String>,
    /// Widget files that can't be widgets: a lowercase name, or a system
    /// widget's name.
    pub problems: Vec<String>,
}

impl Widgets {
    /// The system widgets plus the document's own, from its `widgets` folder.
    pub fn of(document: &Document) -> Self {
        let mut widgets = Self::default();
        for (name, _) in SYSTEM {
            widgets.modules.insert(
                name.to_string(),
                format!("/__devpage/system-widgets/{name}.tsx"),
            );
        }
        let Some(folder) = &document.widgets else {
            return widgets;
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return widgets;
        };
        // One version for the whole folder, in every widget's address: an
        // edited widget or helper is fetched again rather than taken from the
        // browser's module cache, and so is every widget that imports it.
        let version = version(folder);
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for file in files {
            if file.extension().is_none_or(|ext| ext != "tsx") {
                continue;
            }
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let url = document.url(&file);
            if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                // A lowercase .tsx is a helper a widget imports, not a widget.
                continue;
            }
            if SYSTEM.iter().any(|(system, _)| *system == name) {
                widgets.problems.push(format!(
                    "{} — {name} is a system widget's name",
                    url.trim_start_matches('/')
                ));
                continue;
            }
            if name == NOTES {
                widgets.problems.push(format!(
                    "{} — {NOTES} is devpage's tag for a slide's notes",
                    url.trim_start_matches('/')
                ));
                continue;
            }
            widgets
                .modules
                .insert(name, format!("/__devpage/widgets{url}?v={version}"));
        }
        widgets
    }

    /// What a page's `<head>` needs: the import map naming React, and the
    /// widgets the page script loads.
    pub fn head(&self) -> String {
        let modules = serde_json::to_string(&self.modules).unwrap_or_default();
        format!(
            r#"<script type="importmap">{{"imports":{{"react":"/__devpage/react.js","react/jsx-runtime":"/__devpage/react.js","react-dom/client":"/__devpage/react.js"}}}}</script>
<script>window.devpageWidgets = {modules};</script>"#
        )
    }
}

/// A widgets folder's version: the latest change time of the folder and of the
/// `.ts` and `.tsx` files in it, in milliseconds.
pub fn version(folder: &Path) -> u128 {
    let changed = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis())
    };
    let files = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "ts" || ext == "tsx")
        });
    files
        .map(|path| changed(&path))
        .chain([changed(folder)])
        .max()
        .unwrap_or(0)
}

/// Whether `path` is a widget or helper file: a `.ts` or `.tsx` directly in a
/// widgets folder.
pub fn is_widget_file(path: &Path, folder: &Path) -> bool {
    path.parent() == Some(folder)
        && path
            .extension()
            .is_some_and(|ext| ext == "ts" || ext == "tsx")
}

/// Replaces the tags of known widgets in Markdown with placeholder elements,
/// leaving code alone, and takes `<Notes>` out. An unknown capitalized tag,
/// and a `Tab` outside a `Tabs`, are reported; an unknown tag is replaced by
/// an error in its place rather than passed through as HTML.
pub fn expand(source: &str, widgets: &Widgets, problems: &mut Vec<String>) -> String {
    expand_in(source, widgets, problems, None, 0)
}

/// [`expand`] for content inside the widget `parent`, whose first line is line
/// `lines_before + 1` of the page.
fn expand_in(
    source: &str,
    widgets: &Widgets,
    problems: &mut Vec<String>,
    parent: Option<&str>,
    lines_before: usize,
) -> String {
    let code = code_ranges(source);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    while let Some(offset) = source[at..].find('<') {
        let start = at + offset;
        out.push_str(&source[at..start]);
        at = start + 1;
        let is_open = bytes.get(start + 1).is_some_and(|b| b.is_ascii_uppercase());
        if !is_open || in_code(start) {
            out.push('<');
            continue;
        }
        let Some(tag) = parse_open(&source[start..]) else {
            out.push('<');
            continue;
        };
        let line = lines_before + source[..start].matches('\n').count() + 1;
        let after_open = start + tag.len;
        let (inner, end, closed) = if tag.self_closing {
            ("", after_open, true)
        } else {
            match find_close(source, after_open, &tag.name, &in_code) {
                Some((close_start, close_end)) => {
                    (&source[after_open..close_start], close_end, true)
                }
                None => ("", after_open, false),
            }
        };
        // A widget is a block when it has its lines to itself: nothing before
        // its open tag and nothing after its close tag. Anywhere else it is
        // inline, part of a paragraph.
        let starts_line = source[..start]
            .rsplit('\n')
            .next()
            .is_some_and(|before| before.trim().is_empty());
        let ends_line = source[end..]
            .split('\n')
            .next()
            .is_some_and(|after| after.trim().is_empty());
        let block = starts_line && ends_line;
        let element = if block { "div" } else { "span" };
        // A slide's notes are never shown: [`notes`] reads them. Unclosed,
        // they run to the end, so none of their text reaches the slide.
        if tag.name == NOTES {
            if !closed {
                problems.push(format!("{line}: a <{NOTES}> without its </{NOTES}>"));
            }
            at = if closed { end } else { source.len() };
            continue;
        }
        if !widgets.modules.contains_key(&tag.name) {
            let message = format!("<{}> is not a widget", tag.name);
            problems.push(format!("{line}: {message}"));
            out.push_str(&format!(
                r#"<{element} class="widget-error">{}</{element}>"#,
                escape(&message)
            ));
            at = end;
            continue;
        }
        if tag.name == "Tab" && parent != Some("Tabs") {
            problems.push(format!("{line}: <Tab> is outside a <Tabs>"));
        }
        let inner_lines = lines_before + source[..after_open].matches('\n').count();
        let icons;
        let inner = if tag.name == "Cards" {
            icons = card_icons(inner, inner_lines, problems);
            icons.as_str()
        } else {
            inner
        };
        let props = escape(&tag.props);
        let source_html = if SHOWS_SOURCE.contains(&tag.name.as_str()) {
            format!(
                r#" data-source-html="{}""#,
                escape(&crate::highlight::highlight(
                    "markdown",
                    inner.trim_matches('\n')
                ))
            )
        } else {
            String::new()
        };
        let open = format!(
            r#"<{element} data-widget="{name}" data-props="{props}" data-source="{src}"{source_html}>"#,
            name = tag.name,
            src = escape(inner.trim_matches('\n')),
        );
        // Widgets inside the content are expanded too, then left for comrak to
        // render as Markdown: blank lines around a block widget's content
        // make it Markdown rather than raw HTML.
        let inner = expand_in(inner, widgets, problems, Some(&tag.name), inner_lines);
        if block {
            out.push_str(&format!(
                "{open}\n\n{}\n\n</{element}>",
                inner.trim_matches('\n')
            ));
        } else {
            out.push_str(&format!("{open}{inner}</{element}>"));
        }
        at = end;
    }
    out.push_str(&source[at..]);
    out
}

/// One `<Notes>` block: the line it starts on, its text, trimmed, as written,
/// and its `delivery` attribute — how the narrator should sound saying it.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub line: usize,
    pub text: String,
    pub delivery: Option<String>,
}

/// Each `<Notes>…</Notes>` block in a page's Markdown, outside code and HTML
/// comments, as found by [`expand`]. A `<Notes>` without its `</Notes>` runs
/// to the end of the page.
pub fn notes(source: &str) -> Vec<Note> {
    let code = code_ranges(source);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let open = format!("<{NOTES}");
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = source[at..].find(&open) {
        let start = at + offset;
        at = start + 1;
        if in_code(start) {
            continue;
        }
        let Some(tag) = parse_open(&source[start..]).filter(|tag| tag.name == NOTES) else {
            continue;
        };
        let after_open = start + tag.len;
        let (inner, end) = if tag.self_closing {
            ("", after_open)
        } else {
            match find_close(source, after_open, NOTES, &in_code) {
                Some((close_start, close_end)) => (&source[after_open..close_start], close_end),
                None => (&source[after_open..], source.len()),
            }
        };
        found.push(Note {
            line: source[..start].matches('\n').count() + 1,
            text: inner.trim().to_string(),
            delivery: tag.string_prop("delivery"),
        });
        at = end;
    }
    found
}

/// A `Cards` list item may start with a Lucide `(icon)` before its link:
/// drawn here, on the server, as inline SVG the widget picks up. An unknown
/// name is a problem and is dropped. `lines_before` counts the page's lines
/// before the one the content starts on.
fn card_icons(inner: &str, lines_before: usize, problems: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(inner.len());
    for (i, line) in inner.split_inclusive('\n').enumerate() {
        let body = line.trim_start();
        let rest = body
            .strip_prefix("- ")
            .or_else(|| body.strip_prefix("* "))
            .or_else(|| body.strip_prefix("+ "));
        let icon = rest.and_then(|rest| {
            let name = rest.strip_prefix('(')?.split_once(')')?.0;
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            valid.then_some(name)
        });
        let Some(name) = icon else {
            out.push_str(line);
            continue;
        };
        let indent = &line[..line.len() - body.len()];
        let after = &body[2 + name.len() + 2..];
        let drawn = match crate::icons::svg(name) {
            Some(svg) => format!("{svg} "),
            None => {
                problems.push(format!("{}: unknown icon ({name})", lines_before + i + 1));
                String::new()
            }
        };
        out.push_str(&format!(
            "{indent}{}{drawn}{}",
            &body[..2],
            after.trim_start()
        ));
    }
    out
}

struct OpenTag {
    name: String,
    /// The props as a JavaScript object literal.
    props: String,
    len: usize,
    self_closing: bool,
}

impl OpenTag {
    /// The string a `key="…"` attribute holds, if the tag has one.
    fn string_prop(&self, key: &str) -> Option<String> {
        let at = self.props.find(&format!("\"{key}\": \""))? + key.len() + 4;
        let mut strings =
            serde_json::Deserializer::from_str(&self.props[at..]).into_iter::<String>();
        strings.next()?.ok()
    }
}

/// Parses `<Name a="x" b={expr} c>` or `<Name … />` at the start of `text`.
fn parse_open(text: &str) -> Option<OpenTag> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 1;
    let mut name = String::new();
    while let Some(&(_, c)) = chars.get(i) {
        if c.is_ascii_alphanumeric() {
            name.push(c);
            i += 1;
        } else {
            break;
        }
    }
    let mut props = Vec::new();
    loop {
        while chars.get(i).is_some_and(|(_, c)| c.is_whitespace()) {
            i += 1;
        }
        let &(_, c) = chars.get(i)?;
        if c == '>' {
            return Some(OpenTag {
                name,
                props: format!("{{{}}}", props.join(", ")),
                len: chars.get(i + 1).map_or(text.len(), |(at, _)| *at),
                self_closing: false,
            });
        }
        if c == '/' && chars.get(i + 1).is_some_and(|(_, c)| *c == '>') {
            return Some(OpenTag {
                name,
                props: format!("{{{}}}", props.join(", ")),
                len: chars.get(i + 2).map_or(text.len(), |(at, _)| *at),
                self_closing: true,
            });
        }
        let mut attr = String::new();
        while let Some(&(_, c)) = chars.get(i) {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                attr.push(c);
                i += 1;
            } else {
                break;
            }
        }
        if attr.is_empty() {
            return None;
        }
        let key = serde_json::to_string(&attr).ok()?;
        if chars.get(i).is_some_and(|(_, c)| *c == '=') {
            i += 1;
            match chars.get(i)?.1 {
                quote @ ('"' | '\'') => {
                    let mut value = String::new();
                    i += 1;
                    while let Some(&(_, c)) = chars.get(i) {
                        i += 1;
                        if c == quote {
                            break;
                        }
                        value.push(c);
                    }
                    props.push(format!("{key}: {}", serde_json::to_string(&value).ok()?));
                }
                '{' => {
                    let mut depth = 0;
                    let mut expr = String::new();
                    let mut string: Option<char> = None;
                    while let Some(&(_, c)) = chars.get(i) {
                        i += 1;
                        if let Some(q) = string {
                            if c == q {
                                string = None;
                            }
                        } else if matches!(c, '"' | '\'' | '`') {
                            string = Some(c);
                        } else if c == '{' {
                            depth += 1;
                            if depth == 1 {
                                continue;
                            }
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        expr.push(c);
                    }
                    props.push(format!("{key}: ({expr})"));
                }
                _ => return None,
            }
        } else {
            props.push(format!("{key}: true"));
        }
    }
}

/// Finds `</Name>` matching an open tag, counting nested ones of the same
/// name and skipping code.
fn find_close(
    source: &str,
    from: usize,
    name: &str,
    in_code: &dyn Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut depth = 1;
    let mut at = from;
    loop {
        let next_close = source[at..].find(&close).map(|o| at + o)?;
        let next_open = source[at..]
            .find(&open)
            .map(|o| at + o)
            .filter(|&o| o < next_close);
        if let Some(o) = next_open {
            at = o + open.len();
            let boundary = source[at..].starts_with(|c: char| c.is_whitespace() || c == '>');
            if !in_code(o) && boundary {
                depth += 1;
            }
            continue;
        }
        at = next_close + close.len();
        if in_code(next_close) {
            continue;
        }
        depth -= 1;
        if depth == 0 {
            return Some((next_close, at));
        }
    }
}

/// Byte ranges of fenced code blocks and inline code spans.
fn code_ranges(source: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(usize, String)> = None;
    let mut offset = 0;
    // An indented code block starts after a blank line, outside a list, and
    // runs while lines stay indented by four spaces or a tab (or are blank).
    let mut previous_blank = true;
    let mut in_list = false;
    let mut indented: Option<usize> = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let blank = trimmed.is_empty();
        let is_indented = line.starts_with("    ") || line.starts_with('\t');
        if fence.is_none() {
            if let Some(start) = indented {
                if !blank && !is_indented {
                    ranges.push(start..offset);
                    indented = None;
                }
            } else if is_indented && !blank && previous_blank && !in_list {
                indented = Some(offset);
            }
            if indented.is_some() {
                previous_blank = blank;
                offset += line.len();
                continue;
            }
            if !blank && !is_indented {
                in_list = is_list_item(trimmed);
            }
        }
        previous_blank = blank;
        let marker: String = trimmed
            .chars()
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        match &fence {
            Some((start, open)) => {
                if marker.len() >= open.len() && marker.starts_with(&open[..1]) {
                    ranges.push(*start..offset + line.len());
                    fence = None;
                }
            }
            None if marker.len() >= 3 => fence = Some((offset, marker)),
            None => {
                // Inline code spans on this line.
                let mut i = 0;
                let b = line.as_bytes();
                while i < b.len() {
                    if b[i] == b'`' {
                        let run = b[i..].iter().take_while(|c| **c == b'`').count();
                        let ticks = "`".repeat(run);
                        if let Some(end) = line[i + run..].find(&ticks) {
                            let close = i + run + end + run;
                            ranges.push(offset + i..offset + close);
                            i = close;
                            continue;
                        }
                        i += run;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        offset += line.len();
    }
    if let Some((start, _)) = fence {
        ranges.push(start..source.len());
    }
    if let Some(start) = indented {
        ranges.push(start..source.len());
    }
    // HTML comments, outside code, hide whatever they hold.
    let mut from = 0;
    while let Some(open) = source[from..].find("<!--") {
        let open = from + open;
        if ranges.iter().any(|r| r.contains(&open)) {
            from = open + 4;
            continue;
        }
        let close = source[open + 4..]
            .find("-->")
            .map_or(source.len(), |end| open + 4 + end + 3);
        ranges.push(open..close);
        from = close;
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// Whether a line, without its indentation, starts a Markdown list item.
fn is_list_item(trimmed: &str) -> bool {
    let bullet =
        trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ");
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    bullet || (digits > 0 && matches!(trimmed[digits..].chars().next(), Some('.' | ')')))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        // An attribute stays on one line, or a blank line in it would end the
        // HTML block before the widget's content.
        .replace('\n', "&#10;")
}

/// Compiled modules, by a hash of what went into them, so a widget is compiled
/// once per change rather than once per request.
static COMPILED: Mutex<Option<HashMap<u64, Result<String, String>>>> = Mutex::new(None);

/// [`compile`], remembered: the same file, source, widget names and version
/// compile once.
pub fn compile_cached(
    path: &Path,
    source: &str,
    known: &[String],
    version: Option<u128>,
) -> Result<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (path, source, known, version).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(found) = COMPILED
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|c| c.get(&key).cloned()))
    {
        return found.map_err(|e| anyhow!(e));
    }
    let result = compile(path, source, known, version).map_err(|e| e.to_string());
    if let Ok(mut cache) = COMPILED.lock() {
        let cache = cache.get_or_insert_with(HashMap::new);
        // A long session edits many versions; start over rather than grow.
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(key, result.clone());
    }
    result.map_err(|e| anyhow!(e))
}

/// Compiles a widget or helper from TSX to a browser module. Capitalized names
/// the file uses but doesn't declare are other widgets, looked up by name when
/// rendered; the module lists them in `__devpageUses`, so the page loads them
/// first. A relative import carries `version`, so an edited helper is fetched
/// again.
pub fn compile(
    path: &Path,
    source: &str,
    known: &[String],
    version: Option<u128>,
) -> Result<String> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let errors =
        |d: &&oxc::diagnostics::OxcDiagnostic| d.severity == oxc::diagnostics::Severity::Error;
    if let Some(error) = parsed.diagnostics.iter().find(errors) {
        let line = error.labels.first().map(|label| {
            source[..(label.offset() as usize).min(source.len())]
                .matches('\n')
                .count()
                + 1
        });
        return Err(anyhow!(
            "{}{}",
            line.map(|l| format!("line {l}: ")).unwrap_or_default(),
            error.message
        ));
    }
    let mut program = parsed.program;
    if let Some(version) = version {
        version_imports(&allocator, &mut program, version);
    }
    let semantic = SemanticBuilder::new().build(&program).semantic;
    let scoping = semantic.into_scoping();
    let mut used: Vec<String> = scoping
        .root_unresolved_references()
        .keys()
        .map(|name| name.to_string())
        .filter(|name| known.contains(name))
        .collect();
    used.sort();
    let mut options = TransformOptions::default();
    options.jsx.runtime = JsxRuntime::Automatic;
    options.jsx.import_source = Some("react".into());
    let transformed =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if let Some(error) = transformed.diagnostics.iter().find(errors) {
        return Err(anyhow!("{}", error.message));
    }
    let code = Codegen::new().build(&program).code;
    let shims: String = used
        .iter()
        .map(|name| {
            format!(
                "const {name} = (props) => __devpageCreate(globalThis.__devpageWidget({name:?}), props);\n"
            )
        })
        .collect();
    let import = if shims.is_empty() {
        String::new()
    } else {
        "import { createElement as __devpageCreate } from \"react\";\n".to_string()
    };
    let uses = serde_json::to_string(&used).unwrap_or_else(|_| "[]".into());
    Ok(format!(
        "{import}{shims}{code}\nexport const __devpageUses = {uses};\n"
    ))
}

/// Adds `?v=<version>` to every relative import and re-export.
fn version_imports<'a>(
    allocator: &'a Allocator,
    program: &mut oxc::ast::ast::Program<'a>,
    version: u128,
) {
    use oxc::ast::ast::Statement;
    let versioned = |value: &str| -> Option<&'a str> {
        (value.starts_with("./") || value.starts_with("../"))
            .then(|| allocator.alloc_str(&format!("{value}?v={version}")) as &str)
    };
    for statement in program.body.iter_mut() {
        let source = match statement {
            Statement::ImportDeclaration(decl) => Some(&mut decl.source),
            Statement::ExportAllDeclaration(decl) => Some(&mut decl.source),
            Statement::ExportFromDeclaration(decl) => Some(&mut decl.source),
            _ => None,
        };
        if let Some(source) = source
            && let Some(value) = versioned(source.value.as_str())
        {
            source.value = value.into();
            source.raw = None;
        }
    }
}

/// The source of a system widget, by name.
pub fn system(name: &str) -> Option<&'static str> {
    SYSTEM.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> Widgets {
        let mut widgets = Widgets::default();
        for name in names {
            widgets.modules.insert(name.to_string(), format!("/{name}"));
        }
        widgets
    }

    #[test]
    fn expands_a_block_widget_with_markdown_content() {
        let mut problems = Vec::new();
        let out = expand(
            "<Callout kind=\"warning\">\nSome **markdown**\n</Callout>\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(
            out.contains(
                r#"<div data-widget="Callout" data-props="{&quot;kind&quot;: &quot;warning&quot;}""#
            ),
            "{out}"
        );
        assert!(out.contains("\n\nSome **markdown**\n\n</div>"), "{out}");
        assert!(problems.is_empty());
        let html = crate::render::markdown(&out);
        assert!(html.contains("<strong>markdown</strong>"), "{html}");
    }

    #[test]
    fn parses_expression_bare_and_self_closing_props() {
        let mut problems = Vec::new();
        let out = expand(
            "<Chart values={[3, {a: \"}\"}]} stacked title='T' />",
            &set(&["Chart"]),
            &mut problems,
        );
        assert!(out.contains(r#"data-props="{&quot;values&quot;: ([3, {a: &quot;}&quot;}]), &quot;stacked&quot;: true, &quot;title&quot;: &quot;T&quot;}""#), "{out}");
    }

    #[test]
    fn leaves_code_alone_and_reports_unknown_tags() {
        let mut problems = Vec::new();
        let out = expand(
            "`<Callout>` and\n\n```\n<Callout>x</Callout>\n```\n\n<Nope />\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(!out.contains("data-widget"), "{out}");
        assert_eq!(problems, vec!["7: <Nope> is not a widget".to_string()]);
    }

    #[test]
    fn leaves_indented_code_and_html_comments_alone() {
        let mut problems = Vec::new();
        let source = "Code:\n\n    let v: Vec<String> = Vec::new();\n    <Callout>shown as code</Callout>\n\nAfter.\n\n<!-- old: <Todo> -->\n<!--\n<Callout>draft</Callout>\n-->\n";
        let out = expand(source, &set(&["Callout"]), &mut problems);
        assert_eq!(out, source, "{out}");
        assert!(problems.is_empty(), "{problems:?}");
        let html = crate::render::markdown(&out);
        assert!(
            html.contains("let v: Vec&lt;String&gt; = Vec::new();"),
            "{html}"
        );
        assert!(
            html.contains("&lt;Callout&gt;shown as code&lt;/Callout&gt;"),
            "{html}"
        );
    }

    #[test]
    fn indented_lines_in_a_list_are_not_code() {
        let mut problems = Vec::new();
        let out = expand(
            "- item\n\n    more of the item with <Callout>x</Callout>\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(out.contains("data-widget=\"Callout\""), "{out}");
    }

    #[test]
    fn an_unknown_tag_is_replaced_by_an_error_not_passed_through() {
        let mut problems = Vec::new();
        let out = expand(
            "Before.\n\n<Nope>hello</Nope>\n\nSay <Gone /> inline.\n",
            &set(&["Callout"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(!html.contains("<nope>"), "{html}");
        assert!(!html.contains("hello"), "{html}");
        assert!(!html.contains("<gone"), "{html}");
        assert!(
            html.contains(r#"<div class="widget-error">&lt;Nope&gt; is not a widget</div>"#),
            "{html}"
        );
        assert!(
            html.contains(
                r#"Say <span class="widget-error">&lt;Gone&gt; is not a widget</span> inline."#
            ),
            "{html}"
        );
        assert_eq!(
            problems,
            vec![
                "3: <Nope> is not a widget".to_string(),
                "5: <Gone> is not a widget".to_string()
            ]
        );
    }

    #[test]
    fn a_tab_outside_a_tabs_is_a_problem_with_its_page_line() {
        let mut problems = Vec::new();
        let widgets = set(&["Tabs", "Tab", "Callout"]);
        expand(
            "# T\n\n<Tabs>\n<Tab title=\"a\">\nfine\n</Tab>\n</Tabs>\n\n<Callout>\n\n<Tab title=\"b\">\nlost\n</Tab>\n\n</Callout>\n\n<Tab title=\"c\">x</Tab>\n",
            &widgets,
            &mut problems,
        );
        assert_eq!(
            problems,
            vec![
                "11: <Tab> is outside a <Tabs>".to_string(),
                "17: <Tab> is outside a <Tabs>".to_string()
            ]
        );
    }

    #[test]
    fn nests_widgets_and_keeps_inline_ones_inline() {
        let mut problems = Vec::new();
        let out = expand(
            "<Tabs>\n<Tab title=\"a\">\n\nPress <Keys>⌘ P</Keys>.\n\n</Tab>\n</Tabs>\n",
            &set(&["Tabs", "Tab", "Keys"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(html.contains(r#"<div data-widget="Tabs""#), "{html}");
        assert!(html.contains(r#"<div data-widget="Tab""#), "{html}");
        assert!(
            html.contains(
                r#"<span data-widget="Keys" data-props="{}" data-source="⌘ P">⌘ P</span>"#
            ),
            "{html}"
        );
    }

    #[test]
    fn compiles_tsx_with_the_automatic_runtime_and_widget_shims() {
        let js = compile(
            Path::new("Card.tsx"),
            "export default function Card({ n }: { n: number }) { new DOMParser(); return <Badge n={n}><b>{n}</b></Badge>; }",
            &["Badge".to_string()],
            None,
        )
        .unwrap();
        assert!(js.contains("react/jsx-runtime"), "{js}");
        assert!(js.contains("const Badge = (props) =>"), "{js}");
        assert!(!js.contains(": number"), "{js}");
        assert!(!js.contains("const DOMParser"), "{js}");
    }

    #[test]
    fn a_compile_error_names_its_line() {
        let error = compile(
            Path::new("Bad.tsx"),
            "export default 1;\nconst = ;\n",
            &[],
            None,
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("line 2:"), "{error}");
    }

    #[test]
    fn a_module_lists_the_widgets_it_renders_and_versions_its_relative_imports() {
        let js = compile(
            Path::new("Card.tsx"),
            "import { label } from \"./label\";\nexport { tone } from \"./tone\";\nimport React from \"react\";\nexport default function Card() { return <Badge>{label}</Badge>; }",
            &["Badge".to_string()],
            Some(42),
        )
        .unwrap();
        assert!(js.contains("\"./label?v=42\""), "{js}");
        assert!(js.contains("\"./tone?v=42\""), "{js}");
        assert!(
            js.contains("from \"react\""),
            "a package import is left alone: {js}"
        );
        assert!(
            js.contains("export const __devpageUses = [\"Badge\"];"),
            "{js}"
        );
    }

    #[test]
    fn compiling_is_remembered_until_the_source_or_version_changes() {
        let path = Path::new("Same.tsx");
        let first = compile_cached(path, "export default () => <b>1</b>;", &[], Some(1)).unwrap();
        let again = compile_cached(path, "export default () => <b>1</b>;", &[], Some(1)).unwrap();
        assert_eq!(first, again);
        let edited = compile_cached(path, "export default () => <b>2</b>;", &[], Some(1)).unwrap();
        assert!(edited.contains('2'), "{edited}");
        let error = compile_cached(path, "export default (", &[], Some(2)).unwrap_err();
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn a_folders_version_moves_when_a_helper_changes() {
        let dir = std::env::temp_dir().join(format!("devpage-wversion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Card.tsx"), "export default () => null;").unwrap();
        std::fs::write(dir.join("label.ts"), "export const label = 1;").unwrap();
        let before = version(&dir);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("label.ts"), "export const label = 2;").unwrap();
        assert!(version(&dir) > before);
        assert!(is_widget_file(&dir.join("label.ts"), &dir));
        assert!(!is_widget_file(&dir.join("notes.md"), &dir));
    }

    #[test]
    fn lowercase_files_are_helpers_and_a_system_name_is_a_problem() {
        let dir = std::env::temp_dir().join(format!("devpage-wset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("widgets")).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# T\n").unwrap();
        std::fs::write(dir.join("widgets/Card.tsx"), "export default () => null;").unwrap();
        std::fs::write(dir.join("widgets/cards.tsx"), "export const x = 1;").unwrap();
        std::fs::write(
            dir.join("widgets/Callout.tsx"),
            "export default () => null;",
        )
        .unwrap();
        let dir = dir.canonicalize().unwrap();
        let document = Document::load(&dir, &dir).unwrap();
        let widgets = Widgets::of(&document);
        assert!(widgets.modules.contains_key("Card"));
        assert!(!widgets.modules.contains_key("cards"));
        assert!(widgets.modules["Callout"].starts_with("/__devpage/system-widgets/"));
        assert_eq!(
            widgets.problems,
            vec!["widgets/Callout.tsx — Callout is a system widget's name".to_string()]
        );
        let v = version(&dir.join("widgets"));
        assert!(
            widgets.modules["Card"].ends_with(&format!("?v={v}")),
            "{:?}",
            widgets.modules
        );
    }

    #[test]
    fn an_inline_widget_at_the_start_of_a_line_stays_in_its_paragraph() {
        let mut problems = Vec::new();
        let out = expand(
            "<Keys>⌘ P</Keys> to **print**, then <Keys>⏎</Keys>.\n",
            &set(&["Keys"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(html.starts_with("<p><span data-widget=\"Keys\""), "{html}");
        assert!(html.contains("<strong>print</strong>"), "{html}");
        assert_eq!(html.matches("data-widget=\"Keys\"").count(), 2, "{html}");
    }

    #[test]
    fn an_example_gets_its_source_highlighted() {
        let mut problems = Vec::new();
        let out = expand(
            "<Example>\nA **word**\n</Example>\n",
            &set(&["Example"]),
            &mut problems,
        );
        assert!(out.contains("data-source-html=\""), "{out}");
        assert!(out.contains("hl-"), "{out}");
        let callout = expand("<Callout>x</Callout>\n", &set(&["Callout"]), &mut problems);
        assert!(!callout.contains("data-source-html"), "{callout}");
    }

    #[test]
    fn a_cards_icon_is_drawn_and_an_unknown_one_is_a_problem() {
        let mut problems = Vec::new();
        let out = expand(
            "# T\n\n<Cards>\n\n- (rocket) [Start](a.md) — begin here\n- [Plain](b.md) — no icon\n- (no-such-icon) [Odd](c.md)\n\n</Cards>\n",
            &set(&["Cards"]),
            &mut problems,
        );
        assert!(out.contains(r#"<svg class="icon""#), "{out}");
        assert!(!out.contains("(rocket)"), "{out}");
        assert!(out.contains("[Plain](b.md)"), "{out}");
        assert_eq!(problems, vec!["7: unknown icon (no-such-icon)".to_string()]);
    }

    #[test]
    fn notes_are_taken_out_of_the_page_and_their_text_kept() {
        let source = "# Title\n\n<Notes>\n  Say **this**, word for word.\n</Notes>\n\nAfter.\n";
        assert_eq!(
            notes(source),
            vec![Note {
                line: 3,
                text: "Say **this**, word for word.".to_string(),
                delivery: None
            }]
        );
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&["Callout"]), &mut problems));
        assert!(!html.contains("word for word"), "{html}");
        assert!(!html.contains("widget-error"), "{html}");
        assert!(html.contains("<p>After.</p>"), "{html}");
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn unclosed_notes_are_a_problem_and_never_shown() {
        let source = "# Title\n\nShown.\n\n<Notes>\nSay this.\n</Note>\n\nAfter.\n";
        assert_eq!(
            notes(source)[0].text,
            "Say this.\n</Note>\n\nAfter.".to_string()
        );
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
        assert!(html.contains("<p>Shown.</p>"), "{html}");
        assert!(!html.contains("Say this"), "{html}");
        assert!(!html.contains("After."), "{html}");
        assert_eq!(
            problems,
            vec!["5: a <Notes> without its </Notes>".to_string()]
        );
    }

    #[test]
    fn a_notes_delivery_is_kept_and_never_shown() {
        let source = "<Notes delivery=\"Honest and a little teasing: you caught them.\">\nSay this.\n</Notes>\n";
        assert_eq!(
            notes(source)[0].delivery.as_deref(),
            Some("Honest and a little teasing: you caught them.")
        );
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(!html.contains("teasing"), "{html}");
    }

    #[test]
    fn notes_inside_code_and_comments_are_not_notes() {
        let source = "`<Notes>x</Notes>`\n\n```\n<Notes>fenced</Notes>\n```\n\n    <Notes>indented</Notes>\n\n<!-- <Notes>hidden</Notes> -->\n";
        assert!(notes(source).is_empty(), "{:?}", notes(source));
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(
            html.contains("fenced") && html.contains("indented"),
            "{html}"
        );
    }

    #[test]
    fn every_notes_block_is_found_with_its_line() {
        let found = notes("<Notes>one</Notes>\n\ntext\n\n<Notes>\ntwo\n</Notes>\n<Notes/>\n");
        let found: Vec<(usize, String)> = found.into_iter().map(|n| (n.line, n.text)).collect();
        assert_eq!(
            found,
            vec![
                (1, "one".to_string()),
                (5, "two".to_string()),
                (8, String::new())
            ]
        );
    }

    #[test]
    fn a_project_widget_named_notes_clashes() {
        let dir = std::env::temp_dir().join(format!("devpage-wnotes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("widgets")).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# T\n").unwrap();
        std::fs::write(dir.join("widgets/Notes.tsx"), "export default () => null;").unwrap();
        let dir = dir.canonicalize().unwrap();
        let widgets = Widgets::of(&Document::load(&dir, &dir).unwrap());
        assert!(!widgets.modules.contains_key("Notes"));
        assert_eq!(
            widgets.problems,
            vec!["widgets/Notes.tsx — Notes is devpage's tag for a slide's notes".to_string()]
        );
    }
}
