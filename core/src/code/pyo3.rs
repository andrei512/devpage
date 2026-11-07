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
            if value.kind() == "try_expression"
                && let Some(inner) = value.named_child(0)
            {
                value = inner;
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
                && let Some(doc) = arguments.named_child(1)
            {
                let doc = constants
                    .get(text(doc))
                    .cloned()
                    .unwrap_or_else(|| string(text(doc)));
                modules.insert(module, doc);
            }
        }
    }
    (exports, modules)
}

/// The balanced parenthesized value of an attribute's `signature` property.
/// Quoted defaults may contain delimiters; those do not close the signature.
fn explicit_signature(attributes: &[String]) -> Option<String> {
    for attribute in attributes {
        let Some((_, rest)) = attribute.split_once("signature") else {
            continue;
        };
        let Some(rest) = rest
            .trim_start()
            .strip_prefix('=')
            .map(str::trim_start)
            .filter(|s| s.starts_with('('))
        else {
            continue;
        };
        let mut depth = 0;
        let mut quote = None;
        let mut escaped = false;
        for (offset, ch) in rest.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if quote.is_some() {
                if ch == '\\' {
                    escaped = true;
                } else if quote == Some(ch) {
                    quote = None;
                }
                continue;
            }
            if ch == '"' || ch == '\'' {
                quote = Some(ch);
                continue;
            }
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[..=offset].into());
                }
            }
        }
    }
    None
}
/// Python-visible parameters of a Rust function. PyO3's signature attribute
/// supplies defaults and keyword-only rules; interpreter/module arguments are
/// omitted, and an instance receiver becomes positional-only `self`.
pub(super) fn callable(node: Node<'_>, source: &str) -> Option<String> {
    if !matches!(node.kind(), "function_item" | "function_signature_item") {
        return None;
    }
    let attributes = super::languages::attributes(node, source);
    let parameters = node.child_by_field_name("parameters")?;
    let text = |n: Node<'_>| &source[n.byte_range()];
    let mut names = Vec::new();
    let mut receiver = false;
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        if parameter.kind() == "self_parameter" {
            receiver = true;
            continue;
        }
        if parameter.kind() != "parameter" {
            continue;
        }
        let kind = parameter
            .child_by_field_name("type")
            .map(text)
            .unwrap_or("");
        if kind.starts_with("Python<") || kind.contains("::Python<") {
            continue;
        }
        if kind.contains("PyModule") && attributes.iter().any(|a| a.contains("pass_module")) {
            continue;
        }
        if (kind.starts_with("PyRef<") || kind.starts_with("PyRefMut<")) && kind.contains("Self") {
            receiver = true;
            continue;
        }
        let pattern = parameter.child_by_field_name("pattern")?;
        names.push(
            text(pattern)
                .trim_start_matches("mut ")
                .trim_start_matches("r#")
                .to_string(),
        );
    }
    let signature =
        explicit_signature(&attributes).unwrap_or_else(|| format!("({})", names.join(", ")));
    if receiver {
        let parameters = signature
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        Some(if parameters.is_empty() {
            "(self, /)".into()
        } else {
            format!("(self, /, {parameters})")
        })
    } else {
        Some(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Python call contracts retain keyword-only defaults and omit interpreter injection.
    #[test]
    fn callable_parameters_match_pyo3_rules() {
        let source = r#"#[pymethods] impl Graph {
          #[pyo3(signature=(experiment, *, profile, record=None))]
          fn replay(&self, py: Python<'_>, experiment: PyRef<'_, Experiment>, profile: String, record: Option<String>) {}
          #[pyo3(signature=(**parameters))]
          fn bind(&self, py: Python<'_>, parameters: Option<&Bound<'_,PyDict>>) {}
          #[new]
          #[pyo3(signature=(capital, *, accounting="netting", tolerance=10.0))]
          fn new(py: Python<'_>, capital:f64,accounting:String,tolerance:f64)->Self {todo!()}
        }"#;
        let extracted = super::super::grammar("pyo3", None).unwrap().extract(source);
        let signatures: BTreeMap<_, _> = extracted
            .declarations
            .iter()
            .filter_map(|d| Some((d.name.as_str(), d.callable.as_deref()?)))
            .collect();
        assert_eq!(
            signatures["replay"],
            "(self, /, experiment, *, profile, record=None)"
        );
        assert_eq!(signatures["bind"], "(self, /, **parameters)");
        assert_eq!(
            signatures["new"],
            "(capital, *, accounting=\"netting\", tolerance=10.0)"
        );
    }

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
