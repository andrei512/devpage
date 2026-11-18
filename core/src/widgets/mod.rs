//! Widgets: React components used as capitalized HTML tags in Markdown.
//!
//! On the server a page's widget tags become placeholder elements carrying the
//! widget's name, its props' source and its content ([`tags`]), which comrak
//! renders as Markdown. In the browser the page script loads React and the
//! widgets, compiled here from TSX by oxc ([`compile`]), and mounts each
//! placeholder. What was read and compiled is kept in a [`Modules`] cache.
//!
//! This module is the widget set a document has: its names, the address
//! each is served at, what a page loads, and what a request for one of its
//! modules answers ([`request`]).

mod compile;
pub(crate) use compile::{compile as compile_extension, relative_imports as extension_imports};
mod modules;
mod request;
mod tags;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

pub use modules::{Modules, Snapshot};
pub use request::{Compiled, project_module, system_module};
pub use tags::{expand, expand_with};

use crate::addresses::{Owned, address};
use crate::document::Document;
use crate::folder::ServedFolder;

/// The widgets devpage ships, compiled into the binary.
pub const SYSTEM: &[(&str, &str)] = &[
    ("Callout", include_str!("../../assets/widgets/Callout.tsx")),
    (
        SIDEBAR,
        include_str!("../../assets/widgets/DevpageSidebar.tsx"),
    ),
    ("Cards", include_str!("../../assets/widgets/Cards.tsx")),
    (
        LIVE_CANVAS,
        include_str!("../../assets/widgets/DevpageLiveCanvas.tsx"),
    ),
    (
        LIVE_ORB,
        include_str!("../../assets/widgets/DevpageLiveOrb.tsx"),
    ),
    (
        LIVE_TASKS,
        include_str!("../../assets/widgets/DevpageLiveTasks.tsx"),
    ),
    (
        LIVE_TASKS_BUTTON,
        include_str!("../../assets/widgets/DevpageLiveTasksButton.tsx"),
    ),
    ("Example", include_str!("../../assets/widgets/Example.tsx")),
    (
        "FileTree",
        include_str!("../../assets/widgets/FileTree.tsx"),
    ),
    ("Keys", include_str!("../../assets/widgets/Keys.tsx")),
    (
        REQUIREMENTS,
        include_str!("../../assets/widgets/Requirements.tsx"),
    ),
    ("Soon", include_str!("../../assets/widgets/Soon.tsx")),
    ("Steps", include_str!("../../assets/widgets/Steps.tsx")),
    ("Tab", include_str!("../../assets/widgets/Tab.tsx")),
    ("Tabs", include_str!("../../assets/widgets/Tabs.tsx")),
    (
        "Terminal",
        include_str!("../../assets/widgets/Terminal.tsx"),
    ),
];

/// The modules system widgets share, by file name: what a system widget
/// imports by a relative specifier (`./live`) beside the other system
/// widgets. None is a widget: a page can't name one.
pub const SHARED: &[(&str, &str)] = &[
    (
        "live-agent-orb.tsx",
        include_str!("../../assets/widgets/live-agent-orb.tsx"),
    ),
    (
        "live-orb.ts",
        include_str!("../../assets/widgets/live-orb.ts"),
    ),
    ("live.ts", include_str!("../../assets/widgets/live.ts")),
];

/// The live session's orb, which starts and stops it: devpage's own chrome,
/// [placed](crate::document::Places) by devpage where the session is on.
pub const LIVE_ORB: &str = "DevpageLiveOrb";

/// The live session's side panel: its agents and their tasks.
pub const LIVE_TASKS: &str = "DevpageLiveTasks";

/// The navbar's button that opens the live session's side panel.
pub const LIVE_TASKS_BUTTON: &str = "DevpageLiveTasksButton";

/// The live session's canvas: a scratch pad over the page.
pub const LIVE_CANVAS: &str = "DevpageLiveCanvas";

/// The widget that is a site's sidebar: devpage's own chrome, mounted by the
/// page script on the sidebar the server rendered, never written in a page.
/// Its `Devpage` prefix keeps it clear of a project's names: a project may
/// have its own `Sidebar`.
pub const SIDEBAR: &str = "DevpageSidebar";

/// The system widget that lists a part's requirements: the server writes
/// its content.
pub const REQUIREMENTS: &str = "Requirements";

pub use crate::markup::NOTES;

#[cfg(test)]
thread_local! {
    /// The document file of every widget set read on this thread, for the
    /// tests: how often a pass reads each.
    pub(crate) static READ: std::cell::RefCell<Vec<PathBuf>> = Default::default();
}

/// The widgets a document's pages can use: name → the address its module is
/// served at.
#[derive(Debug, Default)]
pub struct Widgets {
    /// The extensions bound to the document, whose widgets are among `modules`.
    pub(crate) extensions: Vec<crate::extensions::Binding>,
    /// Every widget by name — system, extension and project alike — with the
    /// address its module is served at.
    pub modules: BTreeMap<String, String>,
    /// Where its modules are read and compiled.
    cache: Modules,
    /// The served folder, and each project widget's file, by name.
    served: ServedFolder,
    /// Each project widget's file, by name.
    files: BTreeMap<String, PathBuf>,
    /// Every name in `modules`: what a project widget can render by name.
    known: Vec<String>,
    /// What can't be shown: a widget file with a lowercase name or a system
    /// widget's name, and a place that names no widget.
    pub problems: Vec<String>,
    /// The widgets the document shows outside its pages' text.
    places: crate::document::Places,
    /// The document's folder, and the document file's name in the served
    /// folder: what the `Requirements` widget's list is read by.
    document: (PathBuf, String),
}

impl Widgets {
    /// The system widgets plus the document's own, from its `widgets` folder,
    /// each at its [content address](crate::addresses), read and compiled
    /// through `cache`; and the document's places, each naming widgets among
    /// them.
    pub fn of(document: &Document, cache: &Modules) -> Self {
        let mut widgets = Self::read(document, cache);
        widgets.places = document.places.clone();
        for (place, names) in document.places.named() {
            for name in names {
                if !widgets.modules.contains_key(name) {
                    widgets.problems.push(format!(
                        "{} — places: {place}: no widget is named {name}",
                        document.name(&document.path()).unwrap_or_default()
                    ));
                }
            }
        }
        widgets
    }

    /// The system and extension widgets plus those in the document's
    /// `widgets` folder.
    fn read(document: &Document, cache: &Modules) -> Self {
        #[cfg(test)]
        READ.with_borrow_mut(|read| read.push(document.path()));
        let mut widgets = Self {
            cache: cache.clone(),
            ..Self::default()
        };
        for (name, _) in SYSTEM {
            let mount = document.served.mount();
            widgets.modules.insert(
                name.to_string(),
                system_address(&format!("{name}.tsx"), mount),
            );
        }
        widgets.served = document.served.clone();
        widgets.document = (
            document.base.clone(),
            document.name(&document.path()).unwrap_or_default(),
        );
        widgets.extensions = document.extensions.clone();
        for binding in &widgets.extensions {
            for (name, url) in binding.widget_urls() {
                if widgets.modules.insert(name.clone(), url).is_some() {
                    widgets
                        .problems
                        .push(format!("Extension widget {name} is already registered"));
                }
            }
        }
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
            // A file that resolves outside the served folder is never read,
            // or named.
            if file.extension().is_none_or(|ext| ext != "tsx")
                || document.served.file(&file).is_none()
            {
                continue;
            }
            let Some(label) = document.name(&file) else {
                continue;
            };
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                // A lowercase .tsx is a helper a widget imports, not a widget.
                continue;
            }
            if widgets.modules.contains_key(&name) {
                widgets
                    .problems
                    .push(format!("{} — {name} is a system widget's name", label));
                continue;
            }
            if name == NOTES {
                widgets.problems.push(format!(
                    "{} — {NOTES} is devpage's tag for a slide's notes",
                    label
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
            let url = cache
                .snapshot(&document.served, &file)
                .address(&file, &known);
            widgets.modules.insert(name.clone(), url);
            widgets.files.insert(name, file);
        }
        widgets.known = known;
        widgets
    }

    /// The cache its modules are read and compiled through.
    pub fn cache(&self) -> &Modules {
        &self.cache
    }

    /// Every name a page can use as a widget tag, sorted.
    pub fn names(&self) -> &[String] {
        &self.known
    }

    /// The content the server writes for the widget `name` on the page
    /// `file`, in place of what its tag holds: the `Requirements` widget's
    /// list. `None` for every other widget.
    pub(crate) fn written(&self, file: &Path, name: &str) -> Option<String> {
        let (base, document) = &self.document;
        (name == REQUIREMENTS)
            .then(|| crate::requirements::listing(&self.served, base, document, file))
    }

    /// A project widget or helper `file` of this set, compiled now as its
    /// address serves it.
    pub fn compile(&self, file: &Path) -> Result<String> {
        self.cache
            .snapshot(&self.served, file)
            .compile(file, &self.known)
    }

    /// What the `<head>` of a page whose rendered content is `content`, in a
    /// frame that mounts the widgets `chrome`, needs: the import map naming
    /// React and the [page module](crate::addresses), the widgets the page
    /// script loads, the document's places and its extensions' addresses
    /// when it has any, and the page's [load set](Self::load_set), its
    /// places' widgets included — each module as a `modulepreload` at the
    /// very address its import asks for, and the design engine's wasm as a
    /// low-priority fetch — so the browser asks for all of it at once, as
    /// soon as it reads the head.
    pub fn head(&self, content: &str, chrome: &[&str]) -> String {
        let modules = serde_json::to_string(&self.modules).unwrap_or_default();
        let react = address(Owned::Asset("react.js"));
        let page = address(Owned::Asset("page.js"));
        let placed = self.places.named();
        let mut setup = format!("window.devpageWidgets = {modules};");
        if !placed.is_empty() {
            let places: BTreeMap<&str, &[String]> = placed.iter().copied().collect();
            setup.push_str(&format!(
                "window.devpagePlaces = {};",
                serde_json::to_string(&places).unwrap_or_default()
            ));
        }
        if !self.extensions.is_empty() {
            let extensions: BTreeMap<String, String> = self
                .extensions
                .iter()
                .map(crate::extensions::Binding::address)
                .collect();
            setup.push_str(&format!(
                "window.devpageExtensions = {};",
                serde_json::to_string(&extensions).unwrap_or_default()
            ));
        }
        // A place's widgets load after the content's and the frame's.
        let chrome: Vec<&str> = chrome
            .iter()
            .copied()
            .chain(
                placed
                    .iter()
                    .flat_map(|(_, names)| names.iter().map(String::as_str)),
            )
            .collect();
        let (preloads, fetches) = self.load_set(content, &chrome);
        let mut links = String::new();
        for module in preloads {
            links.push_str(&format!("\n<link rel=\"modulepreload\" href=\"{module}\">"));
        }
        for fetch in fetches {
            links.push_str(&format!(
                "\n<link rel=\"preload\" href=\"{fetch}\" as=\"fetch\" crossorigin fetchpriority=\"low\">"
            ));
        }
        for binding in &self.extensions {
            for style in binding.style_urls() {
                links.push_str(&format!(
                    "\n<link rel=\"stylesheet\" href=\"{}\">",
                    crate::html::escape(&style)
                ));
            }
        }
        format!(
            r#"<script type="importmap">{{"imports":{{"react":"{react}","react/jsx-runtime":"{react}","react-dom/client":"{react}","devpage":"{page}"}}}}</script>
<script>{setup}</script>{links}"#
        )
    }

    /// Everything a page whose rendered content is `content`, in a frame that
    /// mounts the widgets `chrome` — a site's sidebar, [`SIDEBAR`] — loads
    /// from devpage beyond its stylesheet and page script: the modules —
    /// React if it has a widget; each widget it names, then the frame's; the
    /// widgets each of those renders by name (its `__devpageUses`), and so
    /// on; every module a widget's imports reach; the design script
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
                    let snapshot = self.cache.snapshot(&self.served, file);
                    for (helper, _) in snapshot.closure(file).into_iter().skip(1) {
                        helpers.push(snapshot.address(&helper, &self.known));
                    }
                    snapshot.compile(file, &self.known)
                }
                None => {
                    let file = format!("{name}.tsx");
                    for helper in system_closure(&file).into_iter().skip(1) {
                        helpers.push(system_address(helper, self.served.mount()));
                    }
                    self.cache
                        .system(&file)
                        .unwrap_or_else(|| Err(anyhow!("no {name}")))
                }
            };
            queue.extend(compiled.map(|js| compile::uses(&js)).unwrap_or_default());
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
            // The design script and the modules it imports, the engine's
            // bindings last.
            for module in [
                "design.js",
                "design-stream.js",
                "design-canvas.js",
                "design-tiles.js",
                "design-engine.js",
            ] {
                modules.push(address(Owned::Asset(module)));
            }
            fetches.push(address(Owned::Asset("design-engine.wasm")));
        }
        (modules, fetches)
    }
}

/// Whether `path` is a widget or helper file: a `.ts` or `.tsx` directly in a
/// widgets folder.
pub fn is_widget_file(path: &Path, folder: &Path) -> bool {
    path.parent() == Some(folder)
        && path
            .extension()
            .is_some_and(|ext| ext == "ts" || ext == "tsx")
}

/// A system module: a system widget's file or one of the [`SHARED`] modules.
struct SystemModule {
    /// Its file name, as its address has it: `Tabs.tsx`, `live.ts`.
    file: String,
    /// Its source.
    source: &'static str,
    /// Each relative specifier it imports and the system module's file the
    /// specifier names.
    imports: Vec<(String, String)>,
    /// Its [version](system_hash).
    hash: String,
}

/// Every system module: the system widgets, each as `<name>.tsx`, and the
/// shared modules.
static SYSTEM_MODULES: LazyLock<Vec<SystemModule>> = LazyLock::new(|| {
    let sources: Vec<(String, &'static str)> = SYSTEM
        .iter()
        .map(|(name, source)| (format!("{name}.tsx"), *source))
        .chain(
            SHARED
                .iter()
                .map(|(file, source)| (file.to_string(), *source)),
        )
        .collect();
    let names: Vec<&str> = SYSTEM.iter().map(|(n, _)| *n).collect();
    let imports: BTreeMap<&str, Vec<(String, String)>> = sources
        .iter()
        .map(|(file, source)| {
            let found = compile::relative_imports(Path::new(file), source)
                .into_iter()
                .filter_map(|specifier| {
                    let stem = specifier.strip_prefix("./")?;
                    modules::IMPORT_EXTENSIONS
                        .iter()
                        .map(|ext| format!("{stem}{ext}"))
                        .find(|named| sources.iter().any(|(file, _)| file == named))
                        .map(|named| (specifier, named))
                })
                .collect();
            (file.as_str(), found)
        })
        .collect();
    sources
        .iter()
        .map(|(file, source)| {
            let mut hash = Sha256::new();
            hash.update(crate::addresses::BUILD);
            hash.update(names.join("\0"));
            for reached in closure(file, &imports) {
                let (_, source) = sources
                    .iter()
                    .find(|(file, _)| *file == reached)
                    .expect("an import names a system module");
                hash.update([0]);
                hash.update(reached);
                hash.update([0]);
                hash.update(source);
            }
            SystemModule {
                file: file.clone(),
                source,
                imports: imports[file.as_str()].clone(),
                hash: crate::addresses::short(&hash.finalize()),
            }
        })
        .collect()
});

/// `file` and every system module its relative imports reach: `file` first,
/// then the rest in name order.
fn closure<'a>(file: &'a str, imports: &'a BTreeMap<&str, Vec<(String, String)>>) -> Vec<&'a str> {
    let mut found = std::collections::BTreeSet::new();
    let mut queue = vec![file];
    while let Some(next) = queue.pop() {
        for (_, imported) in imports.get(next).into_iter().flatten() {
            if imported != file && found.insert(imported.as_str()) {
                queue.push(imported);
            }
        }
    }
    std::iter::once(file).chain(found).collect()
}

/// The system module in `file` — `Tabs.tsx`, `live.ts`.
fn system_module_named(file: &str) -> Option<&'static SystemModule> {
    SYSTEM_MODULES.iter().find(|module| module.file == file)
}

/// The source of the system module in `file`, and each relative specifier
/// it imports with the address that import asks for: the imported module's
/// own, beside the importer's. `None` for a file that isn't a system module.
pub(crate) fn system(file: &str) -> Option<(&'static str, BTreeMap<String, String>)> {
    let module = system_module_named(file)?;
    let imports = module
        .imports
        .iter()
        .map(|(specifier, imported)| {
            let hash = system_hash(imported).unwrap_or_default();
            (specifier.clone(), format!("./{imported}?v={hash}"))
        })
        .collect();
    Some((module.source, imports))
}

/// The system module in `file` and every system module its relative imports
/// reach: `file` first, then the rest in name order. Empty for a file that
/// isn't a system module.
pub(crate) fn system_closure(file: &str) -> Vec<&'static str> {
    let Some(module) = system_module_named(file) else {
        return Vec::new();
    };
    let imports: BTreeMap<&str, Vec<(String, String)>> = SYSTEM_MODULES
        .iter()
        .map(|module| (module.file.as_str(), module.imports.clone()))
        .collect();
    closure(&module.file, &imports)
        .into_iter()
        .filter_map(|file| system_module_named(file).map(|module| module.file.as_str()))
        .collect()
}

/// A system module's version: its source, the sources of every system
/// module its relative imports reach, the names a system widget can render
/// and the build — everything its compiled bytes are made from. `None` for
/// a file that isn't a system module.
pub fn system_hash(file: &str) -> Option<&'static str> {
    system_module_named(file).map(|module| module.hash.as_str())
}

/// The address the system module in `file` is served at for the document
/// served under `mount`, carrying its [version](system_hash).
pub fn system_address(file: &str, mount: &str) -> String {
    address(Owned::SystemModule {
        file,
        mount,
        hash: system_hash(file).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codespec_rust_macro::spec;

    #[test]
    fn a_system_widget_address_carries_its_own_version() {
        assert_eq!(
            system_address("Callout.tsx", ""),
            format!(
                "/__devpage/system-widgets/Callout.tsx?v={}",
                system_hash("Callout.tsx").unwrap()
            )
        );
        assert_ne!(
            system_hash("Callout.tsx"),
            system_hash("Tabs.tsx"),
            "each system widget has its own version"
        );
        assert_eq!(system_hash("Nope"), None);
    }

    #[test]
    fn every_system_module_compiles_and_imports_system_modules_at_their_addresses() {
        let modules = Modules::default();
        let files = SYSTEM
            .iter()
            .map(|(name, _)| format!("{name}.tsx"))
            .chain(SHARED.iter().map(|(file, _)| file.to_string()));
        for file in files {
            let (source, imports) = system(&file).unwrap();
            assert_eq!(
                compile::relative_imports(Path::new(&file), source)
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>(),
                imports.keys().cloned().collect(),
                "{file}: every relative import names a system module"
            );
            let js = modules.system(&file).unwrap().unwrap();
            for address in imports.values() {
                assert!(js.contains(&format!("{address:?}")), "{file}: {js}");
            }
        }
        // An import asks for the very address the imported module has: the
        // importer's own folder, the module's file and its version.
        let (_, imports) = system("DevpageLiveOrb.tsx").unwrap();
        assert_eq!(
            imports["./live"],
            format!("./live.ts?v={}", system_hash("live.ts").unwrap())
        );
        assert_eq!(
            system_address("live.ts", "/d"),
            format!(
                "/d/__devpage/system-widgets/live.ts?v={}",
                system_hash("live.ts").unwrap()
            )
        );
        assert_eq!(system("live"), None, "a module is named by its file");
    }

    #[test]
    fn a_system_widget_loads_with_every_module_its_imports_reach() {
        assert_eq!(
            system_closure("DevpageLiveCanvas.tsx"),
            [
                "DevpageLiveCanvas.tsx",
                "DevpageLiveTasks.tsx",
                "live-agent-orb.tsx",
                "live.ts"
            ]
        );
        assert_eq!(system_closure("Callout.tsx"), ["Callout.tsx"]);
        let widgets = Widgets {
            modules: BTreeMap::from([(
                LIVE_CANVAS.to_string(),
                system_address("DevpageLiveCanvas.tsx", ""),
            )]),
            ..Widgets::default()
        };
        let (modules, _) = widgets.load_set("", &[LIVE_CANVAS]);
        assert_eq!(
            modules,
            [
                address(Owned::Asset("react.js")),
                system_address("DevpageLiveCanvas.tsx", ""),
                system_address("DevpageLiveTasks.tsx", ""),
                system_address("live-agent-orb.tsx", ""),
                system_address("live.ts", ""),
            ]
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
        let document = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        let widgets = Widgets::of(&document, &Modules::default());
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
            Modules::default()
                .snapshot(
                    &ServedFolder::new(&dir).unwrap(),
                    &dir.join("widgets/Card.tsx")
                )
                .address(&dir.join("widgets/Card.tsx"), &known)
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
        let widgets = Widgets::of(
            &Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap(),
            &Modules::default(),
        );
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
    #[spec("widget-names-are-unique")]
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
        let widgets = Widgets::of(
            &Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap(),
            &Modules::default(),
        );
        assert!(!widgets.modules.contains_key("Notes"));
        assert_eq!(
            widgets.problems,
            vec!["widgets/Notes.tsx — Notes is devpage's tag for a slide's notes".to_string()]
        );
    }
    #[test]
    fn a_place_names_widgets_the_head_lists_and_loads() {
        let dir = std::env::temp_dir().join(format!("devpage-wplaces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("widgets")).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\nplaces:\n  navbar: [Dot]\n  panel: [Dot, Gone]\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# T\n").unwrap();
        std::fs::write(dir.join("widgets/Dot.tsx"), "export default () => null;").unwrap();
        let dir = dir.canonicalize().unwrap();
        let document = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        let widgets = Widgets::of(&document, &Modules::default());
        assert_eq!(
            widgets.problems,
            vec!["devpage.yml — places: panel: no widget is named Gone".to_string()]
        );
        let head = widgets.head("<p>no widget</p>", &[]);
        assert!(
            head.contains(r#"window.devpagePlaces = {"navbar":["Dot"],"panel":["Dot","Gone"]};"#),
            "{head}"
        );
        assert!(
            head.contains(&format!(
                "<link rel=\"modulepreload\" href=\"{}\">",
                widgets.modules["Dot"]
            )),
            "a place's widget is in the load set: {head}"
        );
        assert!(
            head.contains(&format!(
                r#""devpage":"{}""#,
                address(Owned::Asset("page.js"))
            )),
            "{head}"
        );
        // A document with no places says nothing of them.
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        let document = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        let head = Widgets::of(&document, &Modules::default()).head("", &[]);
        assert!(!head.contains("devpagePlaces"), "{head}");
        // A place devpage doesn't have is refused.
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nplaces:\n  footer: [Dot]\n",
        )
        .unwrap();
        assert!(Document::load(&dir, &ServedFolder::new(&dir).unwrap()).is_err());
    }
}
