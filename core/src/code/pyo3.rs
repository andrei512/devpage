//! PyO3 registration relationships read from the Rust grammar. Attributes
//! describe declarations; calls decide which module exposes a function.
use std::collections::BTreeMap;
use tree_sitter::Node;

/// All descendants, including the node, in source order.
fn descendants<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    out.push(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        descendants(child, out);
    }
}
/// A Rust string literal's decoded text, including line continuations.
fn string(text: &str) -> String {
    let text = text.replace("\\\n", "");
    let text = text.replace("\\\r\n", "");
    super::languages::literal(&text)
}
/// Functions registered with `wrap_pyfunction!`, by their module suffix, and
/// `PyModule::new` submodule docs supplied with `add("__doc__", ...)`.
/// Runtime/computed registration is deliberately not inferred.
pub(super) fn registrations(
    root: Node<'_>,
    source: &str,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let text = |n: Node<'_>| &source[n.byte_range()];
    let mut nodes = Vec::new();
    descendants(root, &mut nodes);
    let constants: BTreeMap<String, String> = nodes
        .iter()
        .filter(|n| n.kind() == "const_item")
        .filter_map(|n| {
            Some((
                text(n.child_by_field_name("name")?).into(),
                string(text(n.child_by_field_name("value")?)),
            ))
        })
        .collect();
    let mut exports = BTreeMap::new();
    let mut modules = BTreeMap::new();
    for function in nodes.iter().filter(|n| n.kind() == "function_item") {
        let Some(body) = function.child_by_field_name("body") else {
            continue;
        };
        let mut body_nodes = Vec::new();
        descendants(body, &mut body_nodes);
        let mut receivers = BTreeMap::new();
        for declaration in body_nodes.iter().filter(|n| n.kind() == "let_declaration") {
            let Some(name) = declaration
                .child_by_field_name("pattern")
                .filter(|n| n.kind() == "identifier")
            else {
                continue;
            };
            let Some(mut value) = declaration.child_by_field_name("value") else {
                continue;
            };
            if value.kind() == "try_expression" {
                if let Some(inner) = value.named_child(0) {
                    value = inner;
                }
            }
            if value.kind() != "call_expression"
                || value
                    .child_by_field_name("function")
                    .is_none_or(|n| text(n) != "PyModule::new")
            {
                continue;
            }
            let Some(args) = value.child_by_field_name("arguments") else {
                continue;
            };
            let Some(module) = args.named_child(1).filter(|n| n.kind() == "string_literal") else {
                continue;
            };
            let module = string(text(module));
            receivers.insert(text(name).to_string(), module.clone());
            modules.insert(module, String::new());
        }
        for call in body_nodes.iter().filter(|n| n.kind() == "call_expression") {
            let Some(field) = call
                .child_by_field_name("function")
                .filter(|n| n.kind() == "field_expression")
            else {
                continue;
            };
            let Some(receiver) = field.child_by_field_name("value") else {
                continue;
            };
            let Some(method) = field.child_by_field_name("field") else {
                continue;
            };
            let Some(arguments) = call.child_by_field_name("arguments") else {
                continue;
            };
            let module = receivers.get(text(receiver)).cloned().unwrap_or_default();
            if text(method) == "add_function" {
                let mut arguments_nodes = Vec::new();
                descendants(arguments, &mut arguments_nodes);
                for invocation in arguments_nodes
                    .iter()
                    .filter(|n| n.kind() == "macro_invocation")
                {
                    if invocation
                        .child_by_field_name("macro")
                        .is_none_or(|n| text(n) != "wrap_pyfunction")
                    {
                        continue;
                    }
                    let mut cursor = invocation.walk();
                    let Some(tokens) = invocation
                        .named_children(&mut cursor)
                        .find(|n| n.kind() == "token_tree")
                    else {
                        continue;
                    };
                    let inside = text(tokens).trim_start_matches(['(', '{', '[']);
                    let Some((function, _)) = inside.split_once(',') else {
                        continue;
                    };
                    exports.insert(function.trim().to_string(), module.clone());
                }
            }
            if matches!(text(method), "add" | "setattr")
                && arguments
                    .named_child(0)
                    .is_some_and(|n| string(text(n)) == "__doc__")
                && !module.is_empty()
            {
                if let Some(doc) = arguments.named_child(1) {
                    let doc = constants
                        .get(text(doc))
                        .cloned()
                        .unwrap_or_else(|| string(text(doc)));
                    modules.insert(module, doc);
                }
            }
        }
    }
    (exports, modules)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Registration decides the module; source attributes decide public fields.
    #[test]
    fn bindings_keep_runtime_names_and_registration_modules() {
        let source = r#"
/// A graph.
#[pyclass(name = "Graph", module = "odin.research")]
struct PyGraph {
    private: u32,
    /// Its value.
    #[pyo3(get)]
    value: u32,
}
#[pymethods]
impl PyGraph {
    #[new]
    fn new() -> Self { todo!() }
    /// Runs the graph.
    fn run(&self) {}
    fn __repr__(&self) -> String { todo!() }
}
/// Build a graph.
#[pyfunction]
fn build() {}
const MODULE_DOC: &str = "Research tools.";
fn register(m: &Bound<'_, PyModule>) {
    let sub = PyModule::new(py, "research")?;
    sub.add("__doc__", MODULE_DOC)?;
    sub.add_function(wrap_pyfunction!(build, &sub)?)?;
    m.add_submodule(&sub)?;
}
pyo3::create_exception!(odin, Failed, PyValueError, "A failed run.");
"#;
        let extracted = super::super::grammar("pyo3", None).unwrap().extract(source);
        assert!(extracted.errors.is_empty());
        assert_eq!(extracted.exports["build"], "research");
        assert_eq!(extracted.modules["research"], "Research tools.");
        let file = std::path::PathBuf::from("/src/lib.rs");
        let files = BTreeMap::from([(file, std::sync::Arc::new(extracted))]);
        let (items, failures) = super::super::languages::link(
            &files,
            std::path::Path::new("/src"),
            &["odin".into()],
            false,
            false,
            "pyo3",
        );
        assert!(failures.is_empty(), "{failures:?}");
        let graph = &items.items[&vec!["odin".into(), "research".into(), "Graph".into()]];
        assert_eq!(
            graph
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["value", "run"]
        );
        assert!(
            items
                .items
                .contains_key(&vec!["odin".into(), "research".into(), "build".into()])
        );
        let failed = &items.items[&vec!["odin".into(), "Failed".into()]];
        assert_eq!(failed.doc, "A failed run.");
    }
}
