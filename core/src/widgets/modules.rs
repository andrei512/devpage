//! The module cache: what was read of each project widget and helper, and
//! what each compiled to, kept until a file changes — so a widget is read and
//! compiled once per change rather than once per request. One cache belongs to
//! one served folder, and forgets a file when the watcher says it changed.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

use super::compile::{compile, relative_imports};
use super::{SYSTEM, system};
use crate::addresses::{Owned, address};
use crate::folder::ServedFolder;

/// What was read and compiled, shared by every handle to it.
#[derive(Clone, Default)]
pub struct Modules {
    inner: Arc<Caches>,
}

#[derive(Default)]
struct Caches {
    read: Mutex<HashMap<PathBuf, Read>>,
    /// Compiled modules, by a hash of what went into them.
    compiled: Mutex<HashMap<u64, Result<String, String>>>,
}

impl std::fmt::Debug for Modules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Modules")
    }
}

/// A module file as last read: its source and the files its relative imports
/// name, remembered until its change time or size moves or the cache is told
/// it — or a file one of its imports could name — changed.
#[derive(Clone, Debug)]
struct Read {
    changed: Option<std::time::SystemTime>,
    len: u64,
    source: Arc<str>,
    /// Each relative specifier and the file it names, when there is one.
    imports: Vec<(String, PathBuf)>,
    /// Each relative specifier's path, resolved or not: a file created or
    /// removed at one of them, with or without an extension [`Modules::read`]
    /// tries, changes what the module imports.
    specified: Vec<PathBuf>,
}

/// The extensions a relative specifier is tried with, in order.
pub(super) const IMPORT_EXTENSIONS: [&str; 3] = ["", ".tsx", ".ts"];

impl Modules {
    /// Reads `file` and every module its relative imports reach inside the
    /// served folder `served`, each once. A file that resolves outside the
    /// folder — through `..` or a symbolic link — is never read.
    pub fn snapshot(&self, served: &ServedFolder, file: &Path) -> Snapshot {
        let mut reads = BTreeMap::new();
        let mut queue = vec![file.to_path_buf()];
        while let Some(next) = queue.pop() {
            if reads.contains_key(&next) || served.file(&next).is_none() {
                continue;
            }
            let Some(read) = self.read(&next) else {
                continue;
            };
            queue.extend(read.imports.iter().map(|(_, p)| p.clone()));
            reads.insert(next, read);
        }
        Snapshot {
            served: served.clone(),
            reads,
            modules: self.clone(),
        }
    }

    /// Forgets what was read of `file`, so the next address or compile reads it
    /// again, and what was read of every module with an import that could name
    /// it, so a helper created or removed after its importer was read changes
    /// what the importer imports. Called for every changed file: a change time
    /// can miss an edit made within its resolution that keeps the size.
    pub fn forget(&self, file: &Path) {
        let file = lexical(file);
        if let Ok(mut cache) = self.inner.read.lock() {
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

    /// [`compile`], remembered: the same file, source, widget names and import
    /// addresses compile once.
    pub fn compile(
        &self,
        path: &Path,
        source: &str,
        known: &[String],
        imports: &BTreeMap<String, String>,
    ) -> Result<String> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (path, source, known, imports).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(found) = self
            .inner
            .compiled
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            return found.map_err(|e| anyhow!(e));
        }
        let result = compile(path, source, known, imports).map_err(|e| e.to_string());
        if let Ok(mut cache) = self.inner.compiled.lock() {
            // A long session edits many versions; start over rather than grow.
            if cache.len() > 512 {
                cache.clear();
            }
            cache.insert(key, result.clone());
        }
        result.map_err(|e| anyhow!(e))
    }

    /// A system widget, compiled as its address serves it — system widgets
    /// compose only each other. `None` for a name that isn't one.
    pub fn system(&self, name: &str) -> Option<Result<String>> {
        let source = system(name)?;
        let known: Vec<String> = SYSTEM.iter().map(|(name, _)| name.to_string()).collect();
        Some(self.compile(
            Path::new(&format!("{name}.tsx")),
            source,
            &known,
            &Default::default(),
        ))
    }

    /// `file`'s source and resolved imports, read again only when it changed.
    fn read(&self, file: &Path) -> Option<Read> {
        let meta = std::fs::metadata(file).ok().filter(|m| m.is_file())?;
        let (changed, len) = (meta.modified().ok(), meta.len());
        if let Some(found) = self.inner.read.lock().ok().and_then(|cache| {
            cache
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
        if let Ok(mut cache) = self.inner.read.lock() {
            if cache.len() > 4096 {
                cache.clear();
            }
            cache.insert(file.to_path_buf(), read.clone());
        }
        Some(read)
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

/// The files a module is made from, each read once: the module and every
/// module its relative imports reach inside the served folder. Its version
/// and its compiled bytes both come from one snapshot, so a save landing
/// between the two can't serve new bytes under the old version.
#[derive(Debug)]
pub struct Snapshot {
    served: ServedFolder,
    reads: BTreeMap<PathBuf, Read>,
    /// Where its compiles are remembered.
    modules: Modules,
}

impl Snapshot {
    /// The served folder the snapshot was read in.
    pub fn served(&self) -> &ServedFolder {
        &self.served
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
    /// made from. [`Snapshot::address`] is the address that carries it.
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

    /// The address `file` is served at, carrying its [version](Self::hash):
    /// the one a page names it by and an import of it asks for.
    pub fn address(&self, file: &Path, known: &[String]) -> String {
        address(Owned::Module {
            mount: self.served.mount(),
            name: &self.served.name(file),
            hash: &self.hash(file, known),
        })
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
            .map(|(specifier, path)| (specifier.clone(), self.address(path, known)))
            .collect();
        self.modules.compile(file, &read.source, known, &imports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::is_widget_file;

    /// [`Snapshot::hash`] of `file`, read now.
    fn module_hash(modules: &Modules, served: &Path, file: &Path, known: &[String]) -> String {
        modules
            .snapshot(&ServedFolder::new(served).unwrap(), file)
            .hash(file, known)
    }

    #[test]
    fn compiling_is_remembered_until_the_source_or_an_import_changes() {
        let modules = Modules::default();
        let compile_cached =
            |path: &Path, source: &str, known: &[String], imports: &BTreeMap<String, String>| {
                modules.compile(path, source, known, imports)
            };
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
        let modules = Modules::default();
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
                .map(|name| module_hash(&modules, &dir, &w.join(name), &known))
        };
        let before = hashes();
        // Same size, and possibly the same change time: the watcher's word is
        // what makes it read again.
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        modules.forget(&w.join("label.ts"));
        let after = hashes();
        assert_ne!(before[0], after[0], "the widget importing it");
        assert_ne!(before[1], after[1], "a widget importing that widget");
        assert_eq!(before[2], after[2], "a widget that doesn't import it");
        assert_ne!(before[3], after[3], "the helper itself");
        // The importer names the helper by the helper's own address.
        let card = modules
            .snapshot(&ServedFolder::new(&dir).unwrap(), &w.join("Card.tsx"))
            .compile(&w.join("Card.tsx"), &known)
            .unwrap();
        let label = modules
            .snapshot(&ServedFolder::new(&dir).unwrap(), &w.join("label.ts"))
            .address(&w.join("label.ts"), &known);
        assert!(card.contains(&format!("\"{label}\"")), "{card}");
        assert!(label.ends_with(&format!("?v={}", after[3])), "{label}");
        // The names a widget can render are part of what it is compiled from.
        assert_ne!(
            module_hash(&modules, &dir, &w.join("Other.tsx"), &known[..2]),
            after[2]
        );
        assert!(is_widget_file(&w.join("label.ts"), &w));
        assert!(!is_widget_file(&w.join("notes.md"), &w));
    }

    #[test]
    fn a_helper_created_after_its_importer_moves_the_importers_address() {
        let modules = Modules::default();
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
        let missing = module_hash(&modules, &dir, &other, &known);
        // The watcher reports the created file, then each edit to it.
        std::fs::write(w.join("label.ts"), "export const label = 1;").unwrap();
        modules.forget(&w.join("label.ts"));
        let created = module_hash(&modules, &dir, &other, &known);
        assert_ne!(missing, created, "creating the helper");
        let label = modules
            .snapshot(&ServedFolder::new(&dir).unwrap(), &w.join("label.ts"))
            .address(&w.join("label.ts"), &known);
        let compiled = modules
            .snapshot(&ServedFolder::new(&dir).unwrap(), &other)
            .compile(&other, &known)
            .unwrap();
        assert!(compiled.contains(&format!("\"{label}\"")), "{compiled}");
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        modules.forget(&w.join("label.ts"));
        assert_ne!(
            created,
            module_hash(&modules, &dir, &other, &known),
            "editing the helper"
        );
        // Removing it moves the address again.
        let edited = module_hash(&modules, &dir, &other, &known);
        std::fs::remove_file(w.join("label.ts")).unwrap();
        modules.forget(&w.join("label.ts"));
        assert_ne!(
            edited,
            module_hash(&modules, &dir, &other, &known),
            "removing it"
        );
    }

    #[test]
    fn a_snapshot_hashes_and_compiles_what_it_read_whatever_is_saved_since() {
        let modules = Modules::default();
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
        let snapshot = modules.snapshot(&ServedFolder::new(&dir).unwrap(), &card);
        let hash = snapshot.hash(&card, &known);
        // Saves land between hashing and compiling.
        std::fs::write(
            &card,
            "import { label } from \"./label\";\nexport default () => \"new\" + label;",
        )
        .unwrap();
        std::fs::write(w.join("label.ts"), "export const label = 2;").unwrap();
        modules.forget(&card);
        modules.forget(&w.join("label.ts"));
        assert_ne!(
            hash,
            module_hash(&modules, &dir, &card, &known),
            "the files did change"
        );
        assert_eq!(hash, snapshot.hash(&card, &known));
        let compiled = snapshot.compile(&card, &known).unwrap();
        assert!(compiled.contains("\"old\""), "{compiled}");
        let label = snapshot.address(&w.join("label.ts"), &known);
        assert!(compiled.contains(&format!("\"{label}\"")), "{compiled}");
        assert_ne!(
            label,
            modules
                .snapshot(&ServedFolder::new(&dir).unwrap(), &w.join("label.ts"))
                .address(&w.join("label.ts"), &known)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_module_or_import_that_links_out_of_the_served_folder_is_never_read() {
        let top = std::env::temp_dir().join(format!("devpage-wlinkout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join("served/w")).unwrap();
        std::fs::create_dir_all(top.join("outside")).unwrap();
        let top = top.canonicalize().unwrap();
        let (served, w) = (top.join("served"), top.join("served/w"));
        std::fs::write(
            top.join("outside/secret.ts"),
            "export const secret = \"SECRET\";",
        )
        .unwrap();
        std::fs::write(
            top.join("outside/Evil.tsx"),
            "export default () => \"EVIL\";",
        )
        .unwrap();
        std::os::unix::fs::symlink(top.join("outside/secret.ts"), w.join("leak.ts")).unwrap();
        std::os::unix::fs::symlink(top.join("outside/Evil.tsx"), w.join("Evil.tsx")).unwrap();
        std::fs::write(
            w.join("Card.tsx"),
            "import { secret } from \"./leak\";\nexport default () => secret;",
        )
        .unwrap();
        let modules = Modules::default();
        let card = modules.snapshot(&ServedFolder::new(&served).unwrap(), &w.join("Card.tsx"));
        let closure: Vec<PathBuf> = card
            .closure(&w.join("Card.tsx"))
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(closure, vec![w.join("Card.tsx")]);
        let compiled = card.compile(&w.join("Card.tsx"), &[]).unwrap();
        assert!(!compiled.contains("SECRET"), "{compiled}");
        let evil = modules.snapshot(&ServedFolder::new(&served).unwrap(), &w.join("Evil.tsx"));
        assert!(evil.closure(&w.join("Evil.tsx")).is_empty());
        assert!(evil.compile(&w.join("Evil.tsx"), &[]).is_err());
    }
}
