//! Link for Rust: every file's declarations into the targets they make,
//! named as Rust names them — crate, modules, item. Resolves what no single
//! file can: the crate and module a file is (`Cargo.toml`, `lib.rs`, `mod.rs`),
//! the type an `impl` in another file belongs to, what `pub use` makes
//! reachable, and whether an item is reachable from its crate's root.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::extract::{Declaration, Extracted, Visibility};
use super::{Member, Target, Targets};

/// One file of a crate, with what it declares.
pub struct Unit<'a> {
    pub file: &'a Path,
    pub extracted: &'a Extracted,
}

/// The crate a file belongs to and the module it is, from the nearest
/// `Cargo.toml` above it: `None` for a file outside its crate's `src`, which
/// no crate root reaches.
pub fn module_of(
    file: &Path,
    crates: &mut HashMap<PathBuf, Option<String>>,
) -> Option<Vec<String>> {
    let mut at = file.parent();
    while let Some(folder) = at {
        let manifest = folder.join("Cargo.toml");
        let name = crates
            .entry(folder.to_path_buf())
            .or_insert_with(|| crate_name(&manifest))
            .clone();
        if let Some(name) = name {
            let relative = file.strip_prefix(folder.join("src")).ok()?;
            let mut path = vec![name];
            let parts: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            let (last, folders) = parts.split_last()?;
            path.extend(folders.iter().cloned());
            let stem = last.strip_suffix(".rs")?;
            if !(folders.is_empty() && (stem == "lib" || stem == "main")) && stem != "mod" {
                path.push(stem.to_string());
            }
            return Some(path);
        }
        at = folder.parent();
    }
    None
}

/// A crate's name as Rust code names it: its `[lib] name`, or its
/// `[package] name` with `-` as `_`. `None` when there is no manifest, or it
/// names no package.
fn crate_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let mut section = "";
    let mut package = None;
    let mut lib = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        let Some(value) = line
            .strip_prefix("name")
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix('='))
            .map(|value| value.trim().trim_matches('"').to_string())
        else {
            continue;
        };
        match section {
            "[package]" => package = Some(value),
            "[lib]" => lib = Some(value),
            _ => {}
        }
    }
    lib.or(package).map(|name| name.replace('-', "_"))
}

/// Kinds a member of a type is; every other kind is an item of its module.
fn is_type(kind: &str) -> bool {
    matches!(kind, "struct" | "enum" | "union" | "trait")
}

/// The targets of `units`: every module and item, with members on their
/// types. With `private`, an item no `pub` path reaches is kept too.
pub fn link(units: &[(Vec<String>, Unit)], private: bool) -> Targets {
    let mut targets: BTreeMap<Vec<String>, Target> = BTreeMap::new();
    // Whether each module's own declaration makes it public; a crate root is.
    let mut module_public: HashMap<Vec<String>, bool> = HashMap::new();
    // Members of `impl` blocks, by the module they are written in and the
    // type they name, attached once every type is known.
    let mut attached = Vec::new();

    for (module, unit) in units {
        let entry = targets.entry(module.clone()).or_insert_with(|| Target {
            path: module.clone(),
            kind: "module".into(),
            file: unit.file.to_path_buf(),
            line: 1,
            ..Target::default()
        });
        // A file's `//!` documents the module it is; a `mod x;` may document
        // it instead, whichever is read first.
        if !unit.extracted.doc.is_empty() {
            entry.doc = unit.extracted.doc.clone();
        }
        entry.file = unit.file.to_path_buf();
        entry.line = 1;
        if module.len() == 1 {
            module_public.insert(module.clone(), true);
        }
        let declarations = &unit.extracted.declarations;
        // The path each declaration gives its children: a module's own, or
        // its parent's for anything else.
        let mut paths: Vec<Option<Vec<String>>> = Vec::with_capacity(declarations.len());
        for declaration in declarations.iter() {
            let parent = declaration.parent.map(|at| &declarations[at]);
            let inside = match declaration.parent {
                None => Some(module.clone()),
                Some(at) => paths[at].clone(),
            };
            let Some(inside) = inside else {
                paths.push(None);
                continue;
            };
            match parent {
                // Inside a function's body: local, not part of the module.
                Some(parent) if parent.kind == "function" => paths.push(None),
                Some(parent) if is_type(&parent.kind) => {
                    if let Some(target) = targets.get_mut(&inside) {
                        let kind = if declaration.kind == "function" {
                            "method".to_string()
                        } else {
                            declaration.kind.clone()
                        };
                        // A trait's items and an enum's variants are as
                        // visible as the trait or the enum.
                        let public = declaration.visibility == Visibility::Public
                            || parent.kind == "trait"
                            || declaration.kind == "variant";
                        target
                            .members
                            .push(member(declaration, &kind, unit.file, public));
                    }
                    paths.push(None);
                }
                Some(parent) if parent.kind == "attach" => {
                    attached.push((
                        module.clone(),
                        parent.attach.clone(),
                        declaration.clone(),
                        unit.file.to_path_buf(),
                    ));
                    paths.push(None);
                }
                _ if declaration.kind == "attach" => {
                    if declaration
                        .attach
                        .as_ref()
                        .is_some_and(|a| a.implements.is_some())
                    {
                        attached.push((
                            module.clone(),
                            declaration.attach.clone(),
                            declaration.clone(),
                            unit.file.to_path_buf(),
                        ));
                    }
                    paths.push(Some(inside));
                }
                _ => {
                    let mut path = inside.clone();
                    path.push(declaration.name.clone());
                    if declaration.kind == "module" {
                        module_public
                            .insert(path.clone(), declaration.visibility == Visibility::Public);
                        let target = targets.entry(path.clone()).or_insert_with(|| Target {
                            path: path.clone(),
                            kind: "module".into(),
                            file: unit.file.to_path_buf(),
                            line: declaration.line,
                            ..Target::default()
                        });
                        // `mod x;` documents the module its own file is.
                        if !declaration.doc.is_empty() && target.doc.is_empty() {
                            target.doc = declaration.doc.clone();
                        }
                        if declaration.has_body {
                            target.file = unit.file.to_path_buf();
                            target.line = declaration.line;
                            target.doc = declaration.doc.clone();
                        }
                    } else {
                        targets.insert(
                            path.clone(),
                            Target {
                                path: path.clone(),
                                kind: declaration.kind.clone(),
                                signature: declaration.signature.clone(),
                                doc: declaration.doc.clone(),
                                file: unit.file.to_path_buf(),
                                line: declaration.line,
                                public: declaration.visibility == Visibility::Public,
                                ..Target::default()
                            },
                        );
                    }
                    paths.push(Some(path));
                }
            }
        }
    }

    // `impl` members onto the types they name: in the same module first,
    // then the one type of that name in the crate.
    for (module, attach, declaration, file) in attached {
        let Some(attach) = attach else { continue };
        let name = type_name(&attach.to);
        let Some(target) = find_type(&targets, &module, &name) else {
            continue;
        };
        let target = targets.get_mut(&target).unwrap();
        if declaration.kind == "attach" {
            if let Some(implements) = &attach.implements {
                let implements = type_name(implements);
                if !target.implements.contains(&implements) {
                    target.implements.push(implements);
                }
            }
        } else if attach.implements.is_none() {
            let kind = if declaration.kind == "function" {
                "method"
            } else {
                declaration.kind.as_str()
            };
            let public = declaration.visibility == Visibility::Public;
            target
                .members
                .push(member(&declaration, kind, &file, public));
        }
    }

    // Modules are public when every module down to them is declared `pub`.
    let reachable = |path: &[String]| {
        (1..=path.len()).all(|at| {
            let module = &path[..at];
            match targets.get(module) {
                Some(target) if target.kind == "module" => {
                    module_public.get(module).copied().unwrap_or(false)
                }
                _ => true,
            }
        })
    };
    let public: Vec<Vec<String>> = targets
        .iter()
        .filter(|(path, target)| reachable(path) && (target.kind == "module" || target.public))
        .map(|(path, _)| path.clone())
        .collect();
    for target in targets.values_mut() {
        target.public = false;
    }
    for path in &public {
        targets.get_mut(path).unwrap().public = true;
    }

    // `pub use` makes an item reachable where it is written: an alias, and
    // the item's address when no `pub` path reached it otherwise.
    let mut aliases = BTreeMap::new();
    for (module, unit) in units {
        for reexport in &unit.extracted.reexports {
            if reexport.visibility != Visibility::Public || reexport.parent.is_some() {
                continue;
            }
            for (path, alias) in use_paths(&reexport.path) {
                let Some(resolved) = resolve(&targets, module, &path) else {
                    continue;
                };
                let mut at = module.clone();
                at.push(alias);
                if at == resolved {
                    continue;
                }
                let module_reachable = targets.get(module).is_some_and(|t| t.public);
                let target = &targets[&resolved];
                if module_reachable && !target.public && target.kind != "module" {
                    let mut moved = targets.remove(&resolved).unwrap();
                    moved.path = at.clone();
                    moved.public = true;
                    targets.insert(at.clone(), moved);
                    aliases.insert(resolved, at);
                } else {
                    aliases.insert(at, resolved);
                }
            }
        }
    }

    if !private {
        targets.retain(|_, target| target.public);
        for target in targets.values_mut() {
            target.members.retain(|member| member.public);
        }
        aliases.retain(|_, to| targets.contains_key(to));
    }
    Targets { targets, aliases }
}

fn member(declaration: &Declaration, kind: &str, file: &Path, public: bool) -> Member {
    Member {
        name: declaration.name.clone(),
        kind: kind.to_string(),
        signature: declaration.signature.clone(),
        doc: declaration.doc.clone(),
        file: file.to_path_buf(),
        line: declaration.line,
        public,
    }
}

/// A type as written, as its bare name: `crate::a::Foo<T>` is `Foo`.
fn type_name(written: &str) -> String {
    let bare = written.split('<').next().unwrap_or(written);
    let bare = bare.rsplit("::").next().unwrap_or(bare);
    bare.trim().trim_start_matches('&').trim().to_string()
}

/// The type named `name` that code in `module` means: one in the module,
/// or the only one of that name in the crate.
fn find_type(
    targets: &BTreeMap<Vec<String>, Target>,
    module: &[String],
    name: &str,
) -> Option<Vec<String>> {
    let mut here = module.to_vec();
    here.push(name.to_string());
    if targets.get(&here).is_some_and(|t| is_type(&t.kind)) {
        return Some(here);
    }
    let mut found = targets
        .iter()
        .filter(|(path, target)| {
            path.first() == module.first()
                && path.last().is_some_and(|last| last == name)
                && is_type(&target.kind)
        })
        .map(|(path, _)| path.clone());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// What a `use` argument names, each with the name it is used as:
/// `a::{B, C as D}` is `a::B` as `B` and `a::C` as `D`. A glob names nothing
/// to alias.
fn use_paths(written: &str) -> Vec<(Vec<String>, String)> {
    let written: String = written.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    expand(&written, &[], &mut out);
    out
}

fn expand(written: &str, prefix: &[String], out: &mut Vec<(Vec<String>, String)>) {
    let written = written.trim();
    if let Some(open) = written.find('{') {
        let head: Vec<String> = written[..open]
            .trim_end_matches("::")
            .split("::")
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let inner = written[open + 1..].trim_end().trim_end_matches('}');
        let mut path = prefix.to_vec();
        path.extend(head);
        let mut depth = 0;
        let mut start = 0;
        for (at, c) in inner.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    expand(&inner[start..at], &path, out);
                    start = at + 1;
                }
                _ => {}
            }
        }
        expand(&inner[start..], &path, out);
        return;
    }
    let (path, alias) = match written.split_once(" as ") {
        Some((path, alias)) => (path.trim(), Some(alias.trim().to_string())),
        None => (written, None),
    };
    if path.ends_with('*') || path.is_empty() || path == "self" {
        return;
    }
    let mut full = prefix.to_vec();
    full.extend(path.split("::").map(str::to_string));
    let alias = alias.unwrap_or_else(|| full.last().cloned().unwrap_or_default());
    if alias != "_" {
        out.push((full, alias));
    }
}

/// The target `path`, written in `module`, names: from the crate root
/// (`crate::`), the module (`self::`), its parent (`super::`), another
/// crate by name, or the module itself.
fn resolve(
    targets: &BTreeMap<Vec<String>, Target>,
    module: &[String],
    path: &[String],
) -> Option<Vec<String>> {
    let (first, rest) = path.split_first()?;
    let start: Vec<String> = match first.as_str() {
        "crate" => module[..1].to_vec(),
        "self" => module.to_vec(),
        "super" => module[..module.len().saturating_sub(1).max(1)].to_vec(),
        _ => {
            let mut here = module.to_vec();
            here.extend(path.iter().cloned());
            if targets.contains_key(&here) {
                return Some(here);
            }
            let other = path.to_vec();
            return targets.contains_key(&other).then_some(other);
        }
    };
    let mut full = start;
    full.extend(rest.iter().cloned());
    targets.contains_key(&full).then_some(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::extract::{Docs, Grammar};

    fn grammar() -> Grammar {
        Grammar::new(
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("rust.scm"),
            Docs::Rust,
        )
        .unwrap()
    }

    fn crate_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-code-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        for (file, source) in files {
            let path = dir.join("src").join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source).unwrap();
        }
        dir
    }

    fn linked(dir: &Path, files: &[&str], private: bool) -> Targets {
        let grammar = grammar();
        let mut crates = HashMap::new();
        let extracted: Vec<(PathBuf, Extracted)> = files
            .iter()
            .map(|file| {
                let path = dir.join("src").join(file);
                let source = std::fs::read_to_string(&path).unwrap();
                (path, grammar.extract(&source))
            })
            .collect();
        let units: Vec<(Vec<String>, Unit)> = extracted
            .iter()
            .map(|(file, extracted)| {
                (
                    module_of(file, &mut crates).unwrap(),
                    Unit { file, extracted },
                )
            })
            .collect();
        link(&units, private)
    }

    fn path(text: &str) -> Vec<String> {
        text.split("::").map(str::to_string).collect()
    }

    #[test]
    fn a_crate_is_named_by_its_manifest_and_its_files_are_its_modules() {
        let dir = crate_dir(
            "modules",
            &[
                ("lib.rs", "//! The crate.\npub mod a;\nmod hidden;\n"),
                ("a/mod.rs", "//! A.\npub mod b;\npub fn top() {}\n"),
                ("a/b.rs", "pub struct Deep;\n"),
                ("hidden.rs", "pub fn secret() {}\n"),
            ],
        );
        let targets = linked(&dir, &["lib.rs", "a/mod.rs", "a/b.rs", "hidden.rs"], false);
        let paths: Vec<String> = targets.targets.keys().map(|p| p.join("::")).collect();
        assert_eq!(
            paths,
            [
                "my_crate",
                "my_crate::a",
                "my_crate::a::b",
                "my_crate::a::b::Deep",
                "my_crate::a::top"
            ]
        );
        assert_eq!(targets.targets[&path("my_crate")].doc, "The crate.");
        assert_eq!(targets.targets[&path("my_crate::a")].doc, "A.");
        let all = linked(&dir, &["lib.rs", "a/mod.rs", "a/b.rs", "hidden.rs"], true);
        assert!(all.targets.contains_key(&path("my_crate::hidden::secret")));
    }

    #[test]
    fn an_impl_in_another_file_adds_its_methods_and_traits_to_the_type() {
        let dir = crate_dir(
            "impls",
            &[
                ("lib.rs", "pub mod store;\nmod read;\n"),
                (
                    "store.rs",
                    "/// Holds entries.\npub struct Store {\n    /// How many.\n    pub count: usize,\n    secret: u8,\n}\nimpl std::fmt::Display for Store {\n    fn fmt(&self) {}\n}\n",
                ),
                (
                    "read.rs",
                    "use crate::store::Store;\nimpl Store {\n    /// Reads one.\n    pub fn read(&self) -> u8 { 0 }\n    fn private(&self) {}\n}\n",
                ),
            ],
        );
        let targets = linked(&dir, &["lib.rs", "store.rs", "read.rs"], false);
        let store = &targets.targets[&path("my_crate::store::Store")];
        let members: Vec<(&str, &str)> = store
            .members
            .iter()
            .map(|m| (m.kind.as_str(), m.name.as_str()))
            .collect();
        assert_eq!(members, [("field", "count"), ("method", "read")]);
        assert_eq!(store.implements, ["Display"]);
        assert_eq!(store.members[1].doc, "Reads one.");
        assert!(store.members[1].file.ends_with("read.rs"));
    }

    #[test]
    fn pub_use_gives_an_item_in_a_private_module_its_public_address() {
        let dir = crate_dir(
            "reexports",
            &[
                (
                    "lib.rs",
                    "mod decimal;\npub use decimal::{Decimal, parse as parse_decimal};\npub mod open;\n",
                ),
                ("decimal.rs", "pub struct Decimal;\npub fn parse() {}\n"),
                ("open.rs", "pub struct Open;\n"),
            ],
        );
        let targets = linked(&dir, &["lib.rs", "decimal.rs", "open.rs"], false);
        assert!(targets.targets.contains_key(&path("my_crate::Decimal")));
        assert!(
            targets
                .targets
                .contains_key(&path("my_crate::parse_decimal"))
        );
        assert!(
            !targets
                .targets
                .contains_key(&path("my_crate::decimal::Decimal"))
        );
        assert_eq!(
            targets.aliases.get(&path("my_crate::decimal::Decimal")),
            Some(&path("my_crate::Decimal"))
        );
    }

    #[test]
    fn use_lists_expand_to_each_name_and_alias() {
        assert_eq!(
            use_paths("crate::a::{B, c::{D, E as F}, *}"),
            [
                (path("crate::a::B"), "B".to_string()),
                (path("crate::a::c::D"), "D".to_string()),
                (path("crate::a::c::E"), "F".to_string()),
            ]
        );
    }
}
