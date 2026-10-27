//! TSX compiled to browser modules by oxc: the JSX runtime, the shims that
//! let a widget render another by name, and each relative import rewritten
//! to the imported module's address.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{JsxRuntime, TransformOptions, Transformer};

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
pub(super) fn uses(compiled: &str) -> Vec<String> {
    compiled
        .rsplit_once("export const __devpageUses = ")
        .and_then(|(_, list)| serde_json::from_str(list.trim().trim_end_matches(';')).ok())
        .unwrap_or_default()
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

/// The relative specifiers (`./x`, `../x`) a module imports or re-exports
/// from, as the parser sees them — the same statements [`compile`] rewrites.
pub(super) fn relative_imports(path: &Path, source: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
