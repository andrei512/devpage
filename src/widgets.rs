//! Widgets: React components used as capitalized HTML tags in Markdown.
//!
//! On the server a page's widget tags become placeholder elements carrying the
//! widget's name, its props' source and its content, which comrak renders as
//! Markdown. In the browser the page script loads React and the widgets,
//! compiled here from TSX by oxc, and mounts each placeholder.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{JsxRuntime, TransformOptions, Transformer};
use sha2::{Digest, Sha256};

use crate::addresses::{Owned, address};
use crate::document::Document;
use crate::served::ServedFolder;

/// The widgets devpage ships, compiled into the binary.
pub const SYSTEM: &[(&str, &str)] = &[
    ("Callout", include_str!("../assets/widgets/Callout.tsx")),
    (
        SIDEBAR,
        include_str!("../assets/widgets/DevpageSidebar.tsx"),
    ),
    ("Cards", include_str!("../assets/widgets/Cards.tsx")),
    ("Example", include_str!("../assets/widgets/Example.tsx")),
    ("FileTree", include_str!("../assets/widgets/FileTree.tsx")),
    ("Keys", include_str!("../assets/widgets/Keys.tsx")),
    ("Steps", include_str!("../assets/widgets/Steps.tsx")),
    ("Tab", include_str!("../assets/widgets/Tab.tsx")),
    ("Tabs", include_str!("../assets/widgets/Tabs.tsx")),
    ("Terminal", include_str!("../assets/widgets/Terminal.tsx")),
];

/// The widget that is a site's sidebar: devpage's own chrome, mounted by the
/// page script on the sidebar the server rendered, never written in a page.
/// Its `Devpage` prefix keeps it clear of a project's names: a project may
/// have its own `Sidebar`.
pub const SIDEBAR: &str = "DevpageSidebar";

pub use crate::markup::NOTES;
use crate::markup::{UNREADABLE_NOTES, code_ranges, find_close, parse_open, unreadable_notes};

/// System widgets that show their own source: they also get it highlighted
/// as Markdown, as the `sourceHtml` prop.
const SHOWS_SOURCE: &[&str] = &["Example"];

/// The widgets a document's pages can use: name → the address its module is
/// served at.
#[derive(Debug, Default)]
pub struct Widgets {
    pub modules: BTreeMap<String, String>,
    /// The served folder, and each project widget's file, by name.
    served: PathBuf,
    files: BTreeMap<String, PathBuf>,
    /// Every name in `modules`: what a project widget can render by name.
    known: Vec<String>,
    /// Widget files that can't be widgets: a lowercase name, or a system
    /// widget's name.
    pub problems: Vec<String>,
}

impl Widgets {
    /// The system widgets plus the document's own, from its `widgets` folder,
    /// each at its [content address](crate::addresses).
    pub fn of(document: &Document) -> Self {
        let mut widgets = Self::default();
        for (name, _) in SYSTEM {
            widgets
                .modules
                .insert(name.to_string(), address(Owned::SystemWidget(name)));
        }
        widgets.served = document.served.path().to_path_buf();
        widgets.known = widgets.modules.keys().cloned().collect();
        let Some(folder) = &document.widgets else {
            return widgets;
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return widgets;
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        let mut own = Vec::new();
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
            own.push((name, file));
        }
        // A widget's address covers the names it can render, so every name is
        // known before any address is made.
        let known: Vec<String> = widgets
            .modules
            .keys()
            .cloned()
            .chain(own.iter().map(|(name, _)| name.clone()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for (name, file) in own {
            let url = address(Owned::Module {
                snapshot: &Snapshot::of(&document.served, &file),
                file: &file,
                known: &known,
            });
            widgets.modules.insert(name.clone(), url);
            widgets.files.insert(name, file);
        }
        widgets.known = known;
        widgets
    }

    /// What the `<head>` of a page whose rendered content is `content`, in a
    /// frame that mounts the widgets `chrome`, needs: the import map naming
    /// React, the widgets the page script loads, and the page's
    /// [load set](Self::load_set) — each module as a `modulepreload` at the
    /// very address its import asks for, and the design engine's wasm as a
    /// low-priority fetch — so the browser asks for all of it at once, as
    /// soon as it reads the head.
    pub fn head(&self, content: &str, chrome: &[&str]) -> String {
        let modules = serde_json::to_string(&self.modules).unwrap_or_default();
        let react = address(Owned::Asset("react.js"));
        let (preloads, fetches) = self.load_set(content, chrome);
        let mut links = String::new();
        for module in preloads {
            links.push_str(&format!("\n<link rel=\"modulepreload\" href=\"{module}\">"));
        }
        for fetch in fetches {
            links.push_str(&format!(
                "\n<link rel=\"preload\" href=\"{fetch}\" as=\"fetch\" crossorigin fetchpriority=\"low\">"
            ));
        }
        format!(
            r#"<script type="importmap">{{"imports":{{"react":"{react}","react/jsx-runtime":"{react}","react-dom/client":"{react}"}}}}</script>
<script>window.devpageWidgets = {modules};</script>{links}"#
        )
    }

    /// Everything a page whose rendered content is `content`, in a frame that
    /// mounts the widgets `chrome` — a site's sidebar, [`SIDEBAR`] — loads
    /// from devpage beyond its stylesheet and page script: the modules —
    /// React if it has a widget; each widget it names, then the frame's; the
    /// widgets each of those renders by name (its `__devpageUses`), and so
    /// on; every helper a project widget's imports reach; the design script
    /// and engine with an
    /// embedded design — and what is fetched, not imported: the engine's
    /// wasm. Each is the address [`address`] makes, so it is the one the
    /// import asks for.
    pub fn load_set(&self, content: &str, chrome: &[&str]) -> (Vec<String>, Vec<String>) {
        let mut queue: Vec<String> = content
            .split("data-widget=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        // The frame's own come after the content's: the server drew them
        // already, so the reader waits on the content's first.
        queue.extend(chrome.iter().map(|name| name.to_string()));
        let mut seen = std::collections::BTreeSet::new();
        let mut widgets = Vec::new();
        let mut helpers = Vec::new();
        // In the order the page names them: the first are wanted first.
        let mut next = 0;
        while let Some(name) = queue.get(next).cloned() {
            next += 1;
            if !self.modules.contains_key(&name) || !seen.insert(name.clone()) {
                continue;
            }
            widgets.push(self.modules[&name].clone());
            let compiled = match self.files.get(&name) {
                Some(file) => {
                    let snapshot = Snapshot::of(&self.served, file);
                    for (helper, _) in snapshot.closure(file).into_iter().skip(1) {
                        helpers.push(address(Owned::Module {
                            snapshot: &snapshot,
                            file: &helper,
                            known: &self.known,
                        }));
                    }
                    snapshot.compile(file, &self.known)
                }
                None => compile_system(&name).unwrap_or_else(|| Err(anyhow!("no {name}"))),
            };
            queue.extend(compiled.map(|js| uses(&js)).unwrap_or_default());
        }
        let mut modules = Vec::new();
        if !seen.is_empty() {
            modules.push(address(Owned::Asset("react.js")));
        }
        modules.extend(widgets);
        for helper in helpers {
            if !modules.contains(&helper) {
                modules.push(helper);
            }
        }
        let mut fetches = Vec::new();
        if content.contains("<canvas class=\"design\"") {
            modules.push(address(Owned::Asset("design.js")));
            modules.push(address(Owned::Asset("design-engine.js")));
            fetches.push(address(Owned::Asset("design-engine.wasm")));
        }
        (modules, fetches)
    }
}

/// A module file as last read: its source and the files its relative imports
/// name, remembered until its change time or size moves or the watcher says
/// it — or a file one of its imports could name — changed.
#[derive(Clone, Debug)]
struct Read {
    changed: Option<std::time::SystemTime>,
    len: u64,
    source: Arc<str>,
    /// Each relative specifier and the file it names, when there is one.
    imports: Vec<(String, PathBuf)>,
    /// Each relative specifier's path, resolved or not: a file created or
    /// removed at one of them, with or without an extension [`read`] tries,
    /// changes what the module imports.
    specified: Vec<PathBuf>,
}

/// The extensions a relative specifier is tried with, in order.
const IMPORT_EXTENSIONS: [&str; 3] = ["", ".tsx", ".ts"];

type ReadCache = Option<HashMap<PathBuf, Read>>;
static READ: Mutex<ReadCache> = Mutex::new(None);

/// `file`'s source and resolved imports, read again only when it changed.
fn read(file: &Path) -> Option<Read> {
    let meta = std::fs::metadata(file).ok().filter(|m| m.is_file())?;
    let (changed, len) = (meta.modified().ok(), meta.len());
    if let Some(found) = READ.lock().ok().and_then(|cache| {
        cache
            .as_ref()?
            .get(file)
            .filter(|r| r.changed == changed && r.len == len)
            .cloned()
    }) {
        return Some(found);
    }
    let source: Arc<str> = std::fs::read_to_string(file).ok()?.into();
    let folder = file.parent().unwrap_or(file);
    let specifiers = relative_imports(file, &source);
    let specified = specifiers
        .iter()
        .map(|specifier| lexical(&folder.join(specifier)))
        .collect();
    let imports = specifiers
        .into_iter()
        .filter_map(|specifier| {
            let base = folder.join(&specifier);
            IMPORT_EXTENSIONS
                .iter()
                .map(|ext| PathBuf::from(format!("{}{ext}", base.display())))
                .find(|path| path.is_file())
                .and_then(|path| path.canonicalize().ok())
                .map(|path| (specifier, path))
        })
        .collect();
    let read = Read {
        changed,
        len,
        source,
        imports,
        specified,
    };
    if let Ok(mut cache) = READ.lock() {
        let cache = cache.get_or_insert_with(HashMap::new);
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(file.to_path_buf(), read.clone());
    }
    Some(read)
}

/// Forgets what was read of `file`, so the next address or compile reads it
/// again, and what was read of every module with an import that could name
/// it, so a helper created or removed after its importer was read changes
/// what the importer imports. The watcher calls it for every changed file: a
/// change time can miss an edit made within its resolution that keeps the
/// size.
pub fn forget(file: &Path) {
    let file = lexical(file);
    if let Ok(mut cache) = READ.lock()
        && let Some(cache) = cache.as_mut()
    {
        cache.remove(&file);
        cache.retain(|_, read| {
            !read.specified.iter().any(|base| {
                IMPORT_EXTENSIONS
                    .iter()
                    .any(|ext| file.as_os_str() == format!("{}{ext}", base.display()).as_str())
            })
        });
    }
}

/// `path` with its `.` and `..` components resolved by name, not on disk: a
/// specifier's path when the file it names may not exist.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The relative specifiers (`./x`, `../x`) a module imports or re-exports
/// from, as the parser sees them — the same statements [`compile`] rewrites.
fn relative_imports(path: &Path, source: &str) -> Vec<String> {
    use oxc::ast::ast::Statement;
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    parsed
        .program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(decl) => Some(decl.source.value.as_str()),
            Statement::ExportAllDeclaration(decl) => Some(decl.source.value.as_str()),
            Statement::ExportFromDeclaration(decl) => Some(decl.source.value.as_str()),
            _ => None,
        })
        .filter(|value| value.starts_with("./") || value.starts_with("../"))
        .map(str::to_string)
        .collect()
}

/// The files a module is made from, each read once: the module and every
/// module its relative imports reach inside the served folder. Its version
/// and its compiled bytes both come from one snapshot, so a save landing
/// between the two can't serve new bytes under the old version.
#[derive(Debug)]
pub struct Snapshot {
    served: ServedFolder,
    reads: BTreeMap<PathBuf, Read>,
}

impl Snapshot {
    /// Reads `file` and every module its relative imports reach.
    pub fn of(served: impl AsRef<Path>, file: &Path) -> Self {
        let served = ServedFolder::canonical(served.as_ref().to_path_buf());
        let mut reads = BTreeMap::new();
        let mut queue = vec![file.to_path_buf()];
        while let Some(next) = queue.pop() {
            if reads.contains_key(&next) || (next != file && !served.contains(&next)) {
                continue;
            }
            let Some(read) = read(&next) else { continue };
            queue.extend(read.imports.iter().map(|(_, p)| p.clone()));
            reads.insert(next, read);
        }
        Self { served, reads }
    }

    /// The served folder the snapshot was read in.
    pub fn served(&self) -> &Path {
        self.served.path()
    }

    /// `file` and every module its relative imports reach, as read: `file`
    /// first, then the rest in path order, each with its source. Empty when
    /// `file` couldn't be read.
    pub fn closure(&self, file: &Path) -> Vec<(PathBuf, Arc<str>)> {
        let Some(root) = self.reads.get(file) else {
            return Vec::new();
        };
        let mut found: BTreeMap<PathBuf, Arc<str>> = BTreeMap::new();
        let mut queue: Vec<PathBuf> = root.imports.iter().map(|(_, p)| p.clone()).collect();
        while let Some(next) = queue.pop() {
            if next == file || found.contains_key(&next) || !self.served.contains(&next) {
                continue;
            }
            let Some(read) = self.reads.get(&next) else {
                continue;
            };
            queue.extend(read.imports.iter().map(|(_, p)| p.clone()));
            found.insert(next, read.source.clone());
        }
        std::iter::once((file.to_path_buf(), root.source.clone()))
            .chain(found)
            .collect()
    }

    /// A module's version: a hash of its source, the sources of every module
    /// its relative imports reach (each with its path), the widget names it
    /// can render and devpage's build — everything its compiled bytes are
    /// made from. [`address`] is the address that carries it.
    pub fn hash(&self, file: &Path, known: &[String]) -> String {
        let mut hash = Sha256::new();
        hash.update(crate::addresses::BUILD);
        hash.update(known.join("\0"));
        for (path, source) in self.closure(file) {
            hash.update([0]);
            hash.update(
                self.served
                    .relative(&path)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update([0]);
            hash.update(source.as_bytes());
        }
        crate::addresses::short(&hash.finalize())
    }

    /// A project widget or helper, compiled as its address serves it: each
    /// relative import rewritten to the imported module's address, made
    /// from this same snapshot.
    pub fn compile(&self, file: &Path, known: &[String]) -> Result<String> {
        let read = self
            .reads
            .get(file)
            .ok_or_else(|| anyhow!("{} can't be read", file.display()))?;
        let imports = read
            .imports
            .iter()
            .filter(|(_, path)| self.served.contains(path))
            .map(|(specifier, path)| {
                let url = address(Owned::Module {
                    snapshot: self,
                    file: path,
                    known,
                });
                (specifier.clone(), url)
            })
            .collect();
        compile_cached(file, &read.source, known, &imports)
    }
}

/// [`Snapshot::compile`] of `file`, read now.
pub fn compile_module(served: impl AsRef<Path>, file: &Path, known: &[String]) -> Result<String> {
    Snapshot::of(served, file).compile(file, known)
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
        let line = lines_before + source[..start].matches('\n').count() + 1;
        let Some(tag) = parse_open(&source[start..]) else {
            // A `<Notes>` whose attributes don't parse still never reaches the
            // slide: it is reported and taken out up to its `</Notes>`.
            if let Some(len) = unreadable_notes(&source[start..]) {
                problems.push(format!("{line}: {UNREADABLE_NOTES}"));
                at = find_close(source, start + len, NOTES, &in_code)
                    .map_or(source.len(), |(_, end)| end);
                continue;
            }
            out.push('<');
            continue;
        };
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
            if tag.has_prop("delivery") && tag.string_prop("delivery").is_none() {
                problems.push(format!(
                    "{line}: a <{NOTES}> delivery must be a quoted string"
                ));
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
                r#" data-widget-source-html="{}""#,
                escape(&crate::highlight::highlight(
                    "markdown",
                    inner.trim_matches('\n')
                ))
            )
        } else {
            String::new()
        };
        let open = format!(
            r#"<{element} data-widget="{name}" data-props="{props}" data-widget-source="{src}"{source_html}>"#,
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

/// [`compile`], remembered: the same file, source, widget names and import
/// addresses compile once.
pub fn compile_cached(
    path: &Path,
    source: &str,
    known: &[String],
    imports: &BTreeMap<String, String>,
) -> Result<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (path, source, known, imports).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(found) = COMPILED
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|c| c.get(&key).cloned()))
    {
        return found.map_err(|e| anyhow!(e));
    }
    let result = compile(path, source, known, imports).map_err(|e| e.to_string());
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
/// first. A relative import named in `imports` is rewritten to the address
/// given there — the imported module's own content address.
pub fn compile(
    path: &Path,
    source: &str,
    known: &[String],
    imports: &BTreeMap<String, String>,
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
    rewrite_imports(&allocator, &mut program, imports);
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

/// The widgets a module [compiled](compile) renders by name: the
/// `__devpageUses` list `compile` ends it with.
fn uses(compiled: &str) -> Vec<String> {
    compiled
        .rsplit_once("export const __devpageUses = ")
        .and_then(|(_, list)| serde_json::from_str(list.trim().trim_end_matches(';')).ok())
        .unwrap_or_default()
}

/// A system widget, compiled as its address serves it — system widgets
/// compose only each other. `None` for a name that isn't one.
pub fn compile_system(name: &str) -> Option<Result<String>> {
    let source = system(name)?;
    let known: Vec<String> = SYSTEM.iter().map(|(name, _)| name.to_string()).collect();
    Some(compile_cached(
        Path::new(&format!("{name}.tsx")),
        source,
        &known,
        &Default::default(),
    ))
}

/// Rewrites every import and re-export whose specifier `imports` names to
/// the address it gives.
fn rewrite_imports<'a>(
    allocator: &'a Allocator,
    program: &mut oxc::ast::ast::Program<'a>,
    imports: &BTreeMap<String, String>,
) {
    use oxc::ast::ast::Statement;
    let versioned = |value: &str| -> Option<&'a str> {
        imports
            .get(value)
            .map(|address| allocator.alloc_str(address) as &str)
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

    /// [`Snapshot::hash`] of `file`, read now.
    fn module_hash(served: &Path, file: &Path, known: &[String]) -> String {
        Snapshot::of(served, file).hash(file, known)
    }

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
                r#"<span data-widget="Keys" data-props="{}" data-widget-source="⌘ P">⌘ P</span>"#
            ),
            "{html}"
        );
        // `data-source` names a page's Markdown file, which comments are
        // asked for; a widget's own source is not one.
        assert!(!html.contains("data-source"), "{html}");
    }

    #[test]
    fn compiles_tsx_with_the_automatic_runtime_and_widget_shims() {
        let js = compile(
            Path::new("Card.tsx"),
            "export default function Card({ n }: { n: number }) { new DOMParser(); return <Badge n={n}><b>{n}</b></Badge>; }",
            &["Badge".to_string()],
            &BTreeMap::new(),
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
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("line 2:"), "{error}");
    }

    #[test]
    fn a_module_lists_the_widgets_it_renders_and_imports_its_helpers_by_their_addresses() {
        let imports = BTreeMap::from([
            (
                "./label".to_string(),
                "/__devpage/widgets/w/label.ts?v=1".to_string(),
            ),
            (
                "./tone".to_string(),
                "/__devpage/widgets/w/tone.ts?v=2".to_string(),
            ),
        ]);
        let js = compile(
            Path::new("Card.tsx"),
            "import { label } from \"./label\";\nexport { tone } from \"./tone\";\nimport React from \"react\";\nexport default function Card() { return <Badge>{label}</Badge>; }",
            &["Badge".to_string()],
            &imports,
        )
        .unwrap();
        assert!(js.contains("\"/__devpage/widgets/w/label.ts?v=1\""), "{js}");
        assert!(js.contains("\"/__devpage/widgets/w/tone.ts?v=2\""), "{js}");
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
    fn compiling_is_remembered_until_the_source_or_an_import_changes() {
        let path = Path::new("Same.tsx");
        let none = BTreeMap::new();
        let first = compile_cached(path, "export default () => <b>1</b>;", &[], &none).unwrap();
        let again = compile_cached(path, "export default () => <b>1</b>;", &[], &none).unwrap();
        assert_eq!(first, again);
        let edited = compile_cached(path, "export default () => <b>2</b>;", &[], &none).unwrap();
        assert!(edited.contains('2'), "{edited}");
        let source = "export { x } from \"./x\";";
        let imports = |v: &str| BTreeMap::from([("./x".to_string(), format!("/x.ts?v={v}"))]);
        let one = compile_cached(path, source, &[], &imports("1")).unwrap();
        let two = compile_cached(path, source, &[], &imports("2")).unwrap();
        assert!(
            one.contains("/x.ts?v=1") && two.contains("/x.ts?v=2"),
            "{two}"
        );
        let error = compile_cached(path, "export default (", &[], &none).unwrap_err();
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn editing_a_helper_changes_its_importers_addresses_only() {
        let dir = std::env::temp_dir().join(format!("devpage-wversion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("w")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let w = dir.join("w");
        std::fs::write(
            w.join("Card.tsx"),
            "import { label } from \"./label\";\nexport default () => label;",
        )
        .unwrap();
        std::fs::write(
            w.join("Deep.tsx"),
            "export * from \"./Card\";\nexport default () => null;",
        )
        .unwrap();
        std::fs::write(w.join("Other.tsx"), "export default () => null;").unwrap();
        std::fs::write(w.join("label.ts"), "export const label = 1;").unwrap();
        let known = vec!["Card".to_string(), "Deep".to_string(), "Other".to_string()];
        let hashes = || {
            ["Card.tsx", "Deep.tsx", "Other.tsx", "label.ts"]
                .map(|name| module_hash(&dir, &w.join(name), &known))
        };
        let before = hashes();
        // Same size, and possibly the same change time: the watcher's word is
        // what makes it read again.
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        forget(&w.join("label.ts"));
        let after = hashes();
        assert_ne!(before[0], after[0], "the widget importing it");
        assert_ne!(before[1], after[1], "a widget importing that widget");
        assert_eq!(before[2], after[2], "a widget that doesn't import it");
        assert_ne!(before[3], after[3], "the helper itself");
        // The importer names the helper by the helper's own address.
        let card = compile_module(&dir, &w.join("Card.tsx"), &known).unwrap();
        let label = address(Owned::Module {
            snapshot: &Snapshot::of(&dir, &w.join("label.ts")),
            file: &w.join("label.ts"),
            known: &known,
        });
        assert!(card.contains(&format!("\"{label}\"")), "{card}");
        assert!(label.ends_with(&format!("?v={}", after[3])), "{label}");
        // The names a widget can render are part of what it is compiled from.
        assert_ne!(
            module_hash(&dir, &w.join("Other.tsx"), &known[..2]),
            after[2]
        );
        assert!(is_widget_file(&w.join("label.ts"), &w));
        assert!(!is_widget_file(&w.join("notes.md"), &w));
    }

    #[test]
    fn a_helper_created_after_its_importer_moves_the_importers_address() {
        let dir = std::env::temp_dir().join(format!("devpage-wlate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("w")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let w = dir.join("w");
        let other = w.join("Other.tsx");
        std::fs::write(
            &other,
            "import { label } from \"./label\";\nexport default () => label;",
        )
        .unwrap();
        let known = vec!["Other".to_string()];
        // Read while `./label` names nothing.
        let missing = module_hash(&dir, &other, &known);
        // The watcher reports the created file, then each edit to it.
        std::fs::write(w.join("label.ts"), "export const label = 1;").unwrap();
        forget(&w.join("label.ts"));
        let created = module_hash(&dir, &other, &known);
        assert_ne!(missing, created, "creating the helper");
        let label = address(Owned::Module {
            snapshot: &Snapshot::of(&dir, &w.join("label.ts")),
            file: &w.join("label.ts"),
            known: &known,
        });
        let compiled = compile_module(&dir, &other, &known).unwrap();
        assert!(compiled.contains(&format!("\"{label}\"")), "{compiled}");
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        forget(&w.join("label.ts"));
        assert_ne!(
            created,
            module_hash(&dir, &other, &known),
            "editing the helper"
        );
        // Removing it moves the address again.
        let edited = module_hash(&dir, &other, &known);
        std::fs::remove_file(w.join("label.ts")).unwrap();
        forget(&w.join("label.ts"));
        assert_ne!(edited, module_hash(&dir, &other, &known), "removing it");
    }

    #[test]
    fn a_snapshot_hashes_and_compiles_what_it_read_whatever_is_saved_since() {
        let dir = std::env::temp_dir().join(format!("devpage-wsnap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("w")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let w = dir.join("w");
        let card = w.join("Card.tsx");
        std::fs::write(
            &card,
            "import { label } from \"./label\";\nexport default () => \"old\" + label;",
        )
        .unwrap();
        std::fs::write(w.join("label.ts"), "export const label = 1;").unwrap();
        let known = vec!["Card".to_string()];
        let snapshot = Snapshot::of(&dir, &card);
        let hash = snapshot.hash(&card, &known);
        // Saves land between hashing and compiling.
        std::fs::write(
            &card,
            "import { label } from \"./label\";\nexport default () => \"new\" + label;",
        )
        .unwrap();
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        forget(&card);
        forget(&w.join("label.ts"));
        assert_ne!(
            hash,
            module_hash(&dir, &card, &known),
            "the files did change"
        );
        assert_eq!(hash, snapshot.hash(&card, &known));
        let compiled = snapshot.compile(&card, &known).unwrap();
        assert!(compiled.contains("\"old\""), "{compiled}");
        let label = address(Owned::Module {
            snapshot: &snapshot,
            file: &w.join("label.ts"),
            known: &known,
        });
        assert!(compiled.contains(&format!("\"{label}\"")), "{compiled}");
        assert_ne!(
            label,
            address(Owned::Module {
                snapshot: &Snapshot::of(&dir, &w.join("label.ts")),
                file: &w.join("label.ts"),
                known: &known,
            })
        );
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
        let known: Vec<String> = widgets.modules.keys().cloned().collect();
        assert_eq!(
            widgets.modules["Card"],
            address(Owned::Module {
                snapshot: &Snapshot::of(&dir, &dir.join("widgets/Card.tsx")),
                file: &dir.join("widgets/Card.tsx"),
                known: &known
            })
        );
    }

    #[test]
    fn devpage_sidebar_is_reserved_and_a_project_sidebar_is_not() {
        let dir = std::env::temp_dir().join(format!("devpage-wsidebar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("widgets")).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# T\n").unwrap();
        for name in ["DevpageSidebar", "Sidebar"] {
            std::fs::write(
                dir.join(format!("widgets/{name}.tsx")),
                "export default () => null;",
            )
            .unwrap();
        }
        let dir = dir.canonicalize().unwrap();
        let widgets = Widgets::of(&Document::load(&dir, &dir).unwrap());
        assert_eq!(
            widgets.problems,
            vec![
                "widgets/DevpageSidebar.tsx — DevpageSidebar is a system widget's name".to_string()
            ]
        );
        assert!(widgets.modules["DevpageSidebar"].starts_with("/__devpage/system-widgets/"));
        assert!(widgets.modules["Sidebar"].starts_with("/__devpage/widgets/"));
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
        assert!(out.contains("data-widget-source-html=\""), "{out}");
        assert!(out.contains("hl-"), "{out}");
        let callout = expand("<Callout>x</Callout>\n", &set(&["Callout"]), &mut problems);
        assert!(!callout.contains("data-widget-source-html"), "{callout}");
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
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(!html.contains("teasing"), "{html}");
    }

    #[test]
    fn a_notes_with_unreadable_attributes_is_a_problem_and_never_shown() {
        for source in [
            "<Notes delivery=\"the \"aha\" moment\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
            "<Notes delivery=\"say \\\"wow\\\" now\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
        ] {
            let mut problems = Vec::new();
            let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
            assert!(
                !html.contains("SAYTHIS") && !html.contains("Notes"),
                "{html}"
            );
            assert!(html.contains("<p>After.</p>"), "{html}");
            assert_eq!(problems, vec![format!("1: {UNREADABLE_NOTES}")]);
        }
    }

    #[test]
    fn a_notes_delivery_that_is_not_a_quoted_string_is_a_problem() {
        let source = "<Notes delivery={\"warm\"}>\nSay this.\n</Notes>\n";
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
        assert!(!html.contains("Say this"), "{html}");
        assert_eq!(
            problems,
            vec!["1: a <Notes> delivery must be a quoted string".to_string()]
        );
    }

    #[test]
    fn notes_inside_code_and_comments_are_not_notes() {
        let source = "`<Notes>x</Notes>`\n\n```\n<Notes>fenced</Notes>\n```\n\n    <Notes>indented</Notes>\n\n<!-- <Notes>hidden</Notes> -->\n";
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(
            html.contains("fenced") && html.contains("indented"),
            "{html}"
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
