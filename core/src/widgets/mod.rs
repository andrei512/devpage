//! Widgets: React components used as capitalized HTML tags in Markdown.
//!
//! On the server a page's widget tags become placeholder elements carrying the
//! widget's name, its props' source and its content ([`tags`]), which comrak
//! renders as Markdown. In the browser the page script loads React and the
//! widgets, compiled here from TSX by oxc ([`compile`]), and mounts each
//! placeholder. What was read and compiled is kept in a [`Modules`] cache.
//!
//! This module is the widget set a document has: its names, the address
//! each is served at, and what a page loads.

mod compile;
mod modules;
mod tags;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

pub use modules::{Modules, Snapshot};
pub use tags::expand;

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
    ("Example", include_str!("../../assets/widgets/Example.tsx")),
    (
        "FileTree",
        include_str!("../../assets/widgets/FileTree.tsx"),
    ),
    ("Keys", include_str!("../../assets/widgets/Keys.tsx")),
    ("Steps", include_str!("../../assets/widgets/Steps.tsx")),
    ("Tab", include_str!("../../assets/widgets/Tab.tsx")),
    ("Tabs", include_str!("../../assets/widgets/Tabs.tsx")),
    (
        "Terminal",
        include_str!("../../assets/widgets/Terminal.tsx"),
    ),
];

/// The widget that is a site's sidebar: devpage's own chrome, mounted by the
/// page script on the sidebar the server rendered, never written in a page.
/// Its `Devpage` prefix keeps it clear of a project's names: a project may
/// have its own `Sidebar`.
pub const SIDEBAR: &str = "DevpageSidebar";

pub use crate::markup::NOTES;

/// The widgets a document's pages can use: name → the address its module is
/// served at.
#[derive(Debug, Default)]
pub struct Widgets {
    pub modules: BTreeMap<String, String>,
    /// Where its modules are read and compiled.
    cache: Modules,
    /// The served folder, and each project widget's file, by name.
    served: ServedFolder,
    files: BTreeMap<String, PathBuf>,
    /// Every name in `modules`: what a project widget can render by name.
    known: Vec<String>,
    /// Widget files that can't be widgets: a lowercase name, or a system
    /// widget's name.
    pub problems: Vec<String>,
}

impl Widgets {
    /// The system widgets plus the document's own, from its `widgets` folder,
    /// each at its [content address](crate::addresses), read and compiled
    /// through `cache`.
    pub fn of(document: &Document, cache: &Modules) -> Self {
        let mut widgets = Self {
            cache: cache.clone(),
            ..Self::default()
        };
        for (name, _) in SYSTEM {
            let mount = document.served.mount();
            widgets.modules.insert(
                name.to_string(),
                address(Owned::SystemWidget { name, mount }),
            );
        }
        widgets.served = document.served.clone();
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
            // A file that resolves outside the served folder is never read.
            if file.extension().is_none_or(|ext| ext != "tsx")
                || document.served.file(&file).is_none()
            {
                continue;
            }
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let label = document.name(&file);
            if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                // A lowercase .tsx is a helper a widget imports, not a widget.
                continue;
            }
            if SYSTEM.iter().any(|(system, _)| *system == name) {
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
            let url = address(Owned::Module {
                snapshot: &cache.snapshot(&document.served, &file),
                file: &file,
                known: &known,
            });
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

    /// A project widget or helper `file` of this set, compiled now as its
    /// address serves it.
    pub fn compile(&self, file: &Path) -> Result<String> {
        self.cache
            .snapshot(&self.served, file)
            .compile(file, &self.known)
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
                    let snapshot = self.cache.snapshot(&self.served, file);
                    for (helper, _) in snapshot.closure(file).into_iter().skip(1) {
                        helpers.push(address(Owned::Module {
                            snapshot: &snapshot,
                            file: &helper,
                            known: &self.known,
                        }));
                    }
                    snapshot.compile(file, &self.known)
                }
                None => self
                    .cache
                    .system(&name)
                    .unwrap_or_else(|| Err(anyhow!("no {name}"))),
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

/// The source of a system widget, by name.
pub fn system(name: &str) -> Option<&'static str> {
    SYSTEM.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            address(Owned::Module {
                snapshot: &Modules::default().snapshot(
                    &ServedFolder::new(&dir).unwrap(),
                    &dir.join("widgets/Card.tsx")
                ),
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
}
