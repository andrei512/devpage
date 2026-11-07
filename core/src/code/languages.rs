//! Python, TypeScript and PyO3 declaration conventions and module linking.
//! Language-specific syntax is read from tree-sitter nodes; the result uses
//! the same item/member catalog as Rust and native semantic providers.
use super::{Declaration, Extracted, Item, Items, Member, Visibility, extract::Docs};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tree_sitter::Node;

/// Rust attributes immediately preceding a declaration, in source order.
pub(super) fn attributes(node: Node<'_>, source: &str) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut at = node.prev_sibling();
    while let Some(sibling) = at {
        match sibling.kind() {
            "attribute_item" => attributes.push(source[sibling.byte_range()].to_string()),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        at = sibling.prev_sibling();
    }
    attributes.reverse();
    attributes
}
/// A captured string's content; identifiers and expressions remain unchanged.
pub(super) fn literal(text: &str) -> String {
    if text.starts_with('"') {
        let continued = text.replace("\\\n", "");
        serde_json::from_str::<String>(&continued)
            .unwrap_or_else(|_| text.trim_matches('"').to_string())
    } else {
        text.to_string()
    }
}
/// Visibility written in each supported language; member visibility is also
/// bounded by its enclosing declaration during linking.
pub(super) fn visibility(docs: Docs, node: Node<'_>, source: &str, rust: Visibility) -> Visibility {
    match docs {
        Docs::Rust => rust,
        Docs::PyO3 => Visibility::Public,
        Docs::Python => {
            let name = node
                .child_by_field_name("name")
                .map(|n| &source[n.byte_range()])
                .unwrap_or("");
            if name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__")) {
                Visibility::Private
            } else {
                Visibility::Public
            }
        }
        Docs::TypeScript => {
            let text = &source[node.byte_range()];
            if text.starts_with("private ")
                || text.starts_with("protected ")
                || text.starts_with('#')
            {
                return Visibility::Private;
            }
            let mut at = node.parent();
            while let Some(parent) = at {
                match parent.kind() {
                    "export_statement" | "interface_body" | "class_body" => {
                        return Visibility::Public;
                    }
                    "program" | "statement_block" => break,
                    _ => at = parent.parent(),
                }
            }
            Visibility::Private
        }
    }
}
/// Union/intersection branches have no single field identity on their alias;
/// their fields remain in the alias's complete source declaration instead.
pub(super) fn anonymous_member(node: Node<'_>) -> bool {
    if !matches!(node.kind(), "property_signature" | "method_signature") {
        return false;
    }
    let mut at = node.parent();
    while let Some(parent) = at {
        match parent.kind() {
            "union_type" | "intersection_type" => return true,
            "type_alias_declaration" | "interface_declaration" | "class_declaration" => {
                return false;
            }
            _ => at = parent.parent(),
        }
    }
    false
}

/// Python's first string expression in a module or declaration body.
pub(super) fn python_doc(node: Node<'_>, source: &str) -> String {
    let body = node.child_by_field_name("body").unwrap_or(node);
    let Some(first) = body.named_child(0) else {
        return String::new();
    };
    if first.kind() != "expression_statement" {
        return String::new();
    }
    let Some(string) = first.named_child(0).filter(|n| n.kind() == "string") else {
        return String::new();
    };
    let text = &source[string.byte_range()];
    let text = text.trim_start_matches(['r', 'u', 'R', 'U']);
    let delimiter = if text.starts_with("\"\"\"") {
        "\"\"\""
    } else if text.starts_with("'''") {
        "'''"
    } else if text.starts_with('"') {
        "\""
    } else {
        "'"
    };
    let body = text
        .strip_prefix(delimiter)
        .and_then(|s| s.strip_suffix(delimiter))
        .unwrap_or(text);
    let mut lines = body.lines();
    let first = lines.next().unwrap_or("").trim();
    let rest: Vec<_> = lines.collect();
    let indent = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    std::iter::once(first)
        .chain(rest.iter().map(|l| l.get(indent..).unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
/// JSDoc immediately before an export or declaration, without comment markers.
pub(super) fn typescript_doc(mut node: Node<'_>, source: &str) -> String {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "export_statement" | "lexical_declaration" | "variable_declaration"
        ) {
            node = parent;
        } else {
            break;
        }
    }
    let Some(comment) = node.prev_named_sibling().filter(|n| n.kind() == "comment") else {
        return String::new();
    };
    let text = &source[comment.byte_range()];
    let Some(text) = text.strip_prefix("/**").and_then(|s| s.strip_suffix("*/")) else {
        return String::new();
    };
    text.lines()
        .map(|l| l.trim().strip_prefix('*').unwrap_or(l.trim()).trim_start())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
/// The value of a named string property in Rust binding attributes.
fn property(declaration: &Declaration, key: &str) -> Option<String> {
    declaration.attributes.iter().find_map(|attribute| {
        let (_, rest) = attribute.split_once(key)?;
        let rest = rest
            .trim_start()
            .strip_prefix('=')?
            .trim_start()
            .strip_prefix('"')?;
        Some(rest.split('"').next()?.to_string())
    })
}
/// Whether a declaration has the named PyO3 attribute.
fn attribute(declaration: &Declaration, name: &str) -> bool {
    declaration
        .attributes
        .iter()
        .any(|a| a.trim_start().starts_with(&format!("#[{name}")))
}
/// Link path-based languages and flat query catalogs, retaining only exposed
/// declarations and class members. Duplicate identities are gate failures.
pub(super) fn link(
    files: &BTreeMap<PathBuf, Arc<Extracted>>,
    root: &Path,
    prefix: &[String],
    flat: bool,
    private: bool,
    language: &str,
) -> (Items, Vec<(PathBuf, usize, String)>) {
    let mut items = Items::default();
    let mut failures = Vec::new();
    let mut pending_exports = Vec::new();
    for (file, extracted) in files {
        let mut module = prefix.to_vec();
        if !flat && language != "pyo3" {
            let Ok(relative) = file.strip_prefix(root) else {
                failures.push((file.clone(), 1, "source lies outside module root".into()));
                continue;
            };
            let mut components: Vec<String> = relative
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if components
                .last()
                .is_some_and(|s| matches!(s.as_str(), "__init__" | "index"))
            {
                components.pop();
            }
            module.extend(components);
        }
        let exports: BTreeMap<String, String> = if matches!(language, "typescript" | "tsx") {
            extracted
                .reexports
                .iter()
                .filter(|e| !e.imported && e.source.is_none())
                .map(|e| {
                    let (source, alias) = e.path.split_once(" as ").unwrap_or((&e.path, &e.path));
                    (source.trim().to_string(), alias.trim().to_string())
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        for export in extracted.reexports.iter().filter(|e| !e.imported) {
            let (original, alias) = export
                .path
                .split_once(" as ")
                .unwrap_or((&export.path, &export.path));
            let original = original.trim().trim_start_matches("type ");
            let source = export
                .source
                .clone()
                .map(|s| (s, original.to_string()))
                .or_else(|| {
                    if extracted
                        .declarations
                        .iter()
                        .any(|d| d.parent.is_none() && d.name == original)
                    {
                        return None;
                    }
                    extracted
                        .reexports
                        .iter()
                        .filter(|e| e.imported)
                        .find_map(|import| {
                            let (from, local) = import
                                .path
                                .split_once(" as ")
                                .unwrap_or((&import.path, &import.path));
                            if local.trim() != original {
                                return None;
                            }
                            Some((
                                import.source.clone()?,
                                from.trim().trim_start_matches("type ").to_string(),
                            ))
                        })
                });
            if let Some((source, original)) = source {
                pending_exports.push((
                    module.clone(),
                    file.clone(),
                    source,
                    original,
                    alias.trim().to_string(),
                ));
            } else if !extracted
                .declarations
                .iter()
                .any(|d| d.parent.is_none() && d.name == original)
            {
                failures.push((
                    file.clone(),
                    1,
                    format!("cannot resolve exported name {original}"),
                ));
            }
        }
        let mut parents: Vec<Option<Vec<String>>> = Vec::new();
        let mut classes = BTreeMap::new();
        if language == "pyo3" {
            for d in &extracted.declarations {
                if attribute(d, "pyclass") {
                    let mut path = property(d, "module")
                        .map(|s| s.split('.').map(str::to_string).collect::<Vec<_>>())
                        .unwrap_or_else(|| module.clone());
                    path.push(property(d, "name").unwrap_or_else(|| d.name.clone()));
                    classes.insert(d.name.clone(), path);
                }
            }
        }
        for declaration in &extracted.declarations {
            let parent = declaration
                .parent
                .and_then(|i| parents.get(i).cloned())
                .flatten();
            let ancestor = declaration.parent.map(|i| &extracted.declarations[i]);
            if declaration.hidden
                || (!private
                    && declaration.visibility != Visibility::Public
                    && !(declaration.parent.is_none() && exports.contains_key(&declaration.name)))
                || (declaration.parent.is_some() && parent.is_none())
            {
                parents.push(None);
                continue;
            }
            if language == "pyo3" && declaration.kind == "attach" {
                parents.push(if attribute(declaration, "pymethods") {
                    classes.get(&declaration.name).cloned()
                } else {
                    None
                });
                continue;
            }
            if ancestor.is_some_and(|d| {
                !matches!(
                    d.kind.as_str(),
                    "class" | "struct" | "enum" | "interface" | "type" | "attach" | "module"
                )
            }) {
                parents.push(None);
                continue;
            }
            if language == "pyo3"
                && ancestor.is_none()
                && !attribute(declaration, "pyclass")
                && !attribute(declaration, "pyfunction")
                && !attribute(declaration, "pymodule")
                && declaration.kind != "exception"
            {
                parents.push(None);
                continue;
            }
            if language == "pyo3"
                && declaration.kind == "field"
                && !declaration
                    .attributes
                    .iter()
                    .any(|a| a.contains("get") || a.contains("set"))
                && !ancestor.is_some_and(|a| {
                    a.attributes
                        .iter()
                        .any(|s| s.contains("get_all") || s.contains("set_all"))
                })
            {
                parents.push(None);
                continue;
            }
            if language == "pyo3" && declaration.name.starts_with('_') {
                parents.push(None);
                continue;
            }
            if language == "pyo3" && attribute(declaration, "new") {
                if let Some(parent) = &parent
                    && let Some(item) = items.items.get_mut(parent)
                    && let Some(parameters) = &declaration.callable
                {
                    item.signature = format!("{}{parameters}", parent.last().unwrap());
                }
                parents.push(None);
                continue;
            }
            let name = if language == "pyo3" {
                if attribute(declaration, "new") {
                    "__init__".into()
                } else {
                    property(declaration, "name").unwrap_or_else(|| declaration.name.clone())
                }
            } else {
                exports
                    .get(&declaration.name)
                    .cloned()
                    .unwrap_or_else(|| declaration.name.clone())
            };
            if let Some(parent) = parent.as_ref().filter(|_| {
                ancestor.is_some_and(|d| {
                    matches!(
                        d.kind.as_str(),
                        "class" | "struct" | "enum" | "interface" | "type" | "attach"
                    )
                })
            }) {
                let kind = if language == "pyo3"
                    && (attribute(declaration, "getter")
                        || attribute(declaration, "setter")
                        || declaration.kind == "field")
                {
                    "property"
                } else if declaration.kind == "function" {
                    "method"
                } else {
                    &declaration.kind
                };
                let member = Member {
                    name: name.clone(),
                    kind: kind.into(),
                    signature: if language == "pyo3" {
                        if kind == "property" {
                            name.clone()
                        } else {
                            format!("{}{}", name, declaration.callable.as_deref().unwrap_or(""))
                        }
                    } else {
                        declaration.signature.clone()
                    },
                    doc: declaration.doc.clone(),
                    file: file.clone(),
                    line: declaration.line,
                    public: true,
                };
                // PyO3 impls may precede classes: attach after all files below.
                let item = items.items.entry(parent.clone()).or_insert_with(|| Item {
                    path: parent.clone(),
                    ..Item::default()
                });
                if item.members.iter().any(|m| m.name == member.name) {
                    failures.push((
                        file.clone(),
                        declaration.line,
                        format!("duplicate member {}", member.name),
                    ));
                } else {
                    item.members.push(member);
                }
                parents.push(None);
                continue;
            }
            let mut path = if let Some(path) = classes
                .get(&declaration.name)
                .filter(|_| language == "pyo3")
            {
                path[..path.len() - 1].to_vec()
            } else {
                parent.unwrap_or_else(|| module.clone())
            };
            if language == "pyo3" && attribute(declaration, "pyfunction") {
                if let Some(submodule) = extracted.exports.get(&declaration.name) {
                    if !submodule.is_empty() {
                        path.extend(submodule.split('.').map(str::to_string));
                    }
                } else {
                    failures.push((
                        file.clone(),
                        declaration.line,
                        format!(
                            "PyO3 function {} has no source registration",
                            declaration.name
                        ),
                    ));
                }
            }
            if !(language == "pyo3"
                && attribute(declaration, "pymodule")
                && path.last() == Some(&name))
            {
                path.extend(name.split("::").map(str::to_string));
            }
            let mut item = Item {
                path: path.clone(),
                kind: if language == "pyo3" && attribute(declaration, "pymodule") {
                    "module".into()
                } else if language == "pyo3" && attribute(declaration, "pyclass") {
                    "class".into()
                } else {
                    declaration.kind.clone()
                },
                signature: if language == "pyo3" {
                    if let Some(parameters) = &declaration.callable {
                        format!("{name}{parameters}")
                    } else {
                        format!("class {name}")
                    }
                } else {
                    declaration.signature.clone()
                },
                doc: declaration.doc.clone(),
                file: file.clone(),
                line: declaration.line,
                public: declaration.visibility == Visibility::Public
                    || exports.contains_key(&declaration.name),
                ..Item::default()
            };
            if let Some(existing) = items.items.get(&path) {
                if existing.file.as_os_str().is_empty() || existing.container {
                    item.members = existing.members.clone();
                } else if matches!(language, "typescript" | "tsx")
                    && existing.file == *file
                    && matches!(
                        (existing.kind.as_str(), item.kind.as_str()),
                        ("type", "function")
                            | ("function", "type")
                            | ("function", "function")
                            | ("interface", "interface")
                            | ("class", "interface")
                            | ("interface", "class")
                    )
                {
                    item.signature = format!("{}\n{}", existing.signature, item.signature);
                    if item.doc != existing.doc {
                        item.doc = format!("{}\n\n{}", existing.doc, item.doc)
                            .trim()
                            .to_string();
                    }
                    item.members = existing.members.clone();
                } else {
                    failures.push((
                        file.clone(),
                        declaration.line,
                        format!("duplicate reference {}", path.join("::")),
                    ));
                }
            }
            items.items.insert(path.clone(), item);
            parents.push(Some(path));
        }
        for (submodule, doc) in &extracted.modules {
            let mut path = prefix.to_vec();
            path.extend(submodule.split('.').map(str::to_string));
            items.items.insert(
                path.clone(),
                Item {
                    path,
                    kind: "module".into(),
                    doc: doc.clone(),
                    file: file.clone(),
                    line: 1,
                    public: true,
                    ..Item::default()
                },
            );
        }
        // Namespace navigation needs explicit intermediate module pages.
        let paths: Vec<_> = items.items.keys().cloned().collect();
        for path in paths {
            for length in 1..path.len() {
                let parent = path[..length].to_vec();
                items.items.entry(parent.clone()).or_insert_with(|| Item {
                    path: parent,
                    kind: "module".into(),
                    container: true,
                    doc: if path[..length] == module {
                        extracted.doc.clone()
                    } else {
                        String::new()
                    },
                    file: file.clone(),
                    line: 1,
                    public: true,
                    ..Item::default()
                });
            }
        }
    }
    for _ in 0..pending_exports.len().max(1) {
        if pending_exports.is_empty() {
            break;
        }
        let mut unresolved = Vec::new();
        for (module, file, source, original, alias) in pending_exports {
            let mut candidates = Vec::new();
            let target = crate::folder::lexical(&file.parent().unwrap_or(root).join(&source));
            for extension in ["ts", "tsx"] {
                candidates.push(target.with_extension(extension));
                candidates.push(target.join(format!("index.{extension}")));
            }
            let resolved = candidates
                .into_iter()
                .filter(|p| files.contains_key(p))
                .find_map(|p| {
                    let relative = p.strip_prefix(root).ok()?;
                    let mut path = prefix.to_vec();
                    let mut parts: Vec<String> = relative
                        .with_extension("")
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    if parts.last().is_some_and(|p| p == "index") {
                        parts.pop();
                    }
                    path.extend(parts);
                    path.push(original.clone());
                    items.items.get(&path).cloned()
                });
            if let Some(mut item) = resolved {
                let mut path = module;
                path.push(alias);
                item.path = path.clone();
                if items.items.insert(path.clone(), item).is_some() {
                    failures.push((file, 1, format!("duplicate reexport {}", path.join("::"))));
                }
            } else {
                unresolved.push((module, file, source, original, alias));
            }
        }
        pending_exports = unresolved;
    }
    for (_, file, source, name, _) in pending_exports {
        failures.push((file, 1, format!("cannot resolve {name} from {source}")));
    }
    (items, failures)
}
