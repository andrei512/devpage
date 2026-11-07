//! Semantic declarations contributed by native extensions enter the ordinary
//! code catalog. This boundary validates source provenance and identities;
//! extensions own domain joins, never a second renderer or link checker.
use super::{Item, Items, Namespace};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A prepared namespace and its precise semantic coverage diagnostics.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contribution {
    /// Items keyed by their native code paths after validation.
    items: Vec<Item>,
    /// Semantic failures that must remain visible to checking.
    #[serde(default)]
    problems: Vec<Failure>,
}
/// A source-located semantic failure reported by the provider.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    /// Source path relative to the served root.
    file: PathBuf,
    /// One-based source line.
    line: usize,
    /// What could not be resolved or covered.
    message: String,
}
/// Whether a path component is one unambiguous reference name.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '#', '?', '\0'])
}
/// Validate a source location against a real file inside the served folder.
fn source(scope: &crate::folder::ServedFolder, file: &Path, line: usize) -> Result<PathBuf> {
    if file.is_absolute()
        || file
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("reference source must be relative: {}", file.display());
    }
    let absolute = scope
        .file(&scope.path().join(file))
        .ok_or_else(|| anyhow::anyhow!("reference source does not exist: {}", file.display()))?;
    let content = std::fs::read_to_string(&absolute)?;
    if line == 0 || line > content.lines().count().max(1) {
        bail!(
            "reference source line is out of range: {}:{line}",
            file.display()
        );
    }
    Ok(absolute)
}
impl Namespace {
    /// Refresh a provider namespace atomically. A failed revision clears its
    /// catalog and records an error; stale declarations must not pass a gate.
    pub(super) fn refresh_provider(&self) {
        let Some(binding) = &self.provider else {
            return;
        };
        let scope = binding.scope();
        let result = (|| -> Result<_> {
            let contribution: Contribution =
                serde_json::from_value(binding.references(&self.name)?)?;
            let mut items = Items::default();
            let mut ids = BTreeSet::new();
            for mut item in contribution.items {
                if item.container {
                    bail!("providers must supply documented parent items");
                }
                if item.path.is_empty() || item.path.iter().any(|name| !valid_name(name)) {
                    bail!("invalid reference path {:?}", item.path);
                }
                if !ids.insert(item.path.clone()) {
                    bail!("duplicate reference {}", item.path.join("::"));
                }
                item.file = source(&scope, &item.file, item.line)?;
                for member in &mut item.members {
                    if !valid_name(&member.name) {
                        bail!("invalid reference member {}", member.name);
                    }
                    let mut path = item.path.clone();
                    path.push(member.name.clone());
                    if !ids.insert(path.clone()) {
                        bail!("duplicate reference {}", path.join("::"));
                    }
                    member.file = source(&scope, &member.file, member.line)?;
                }
                items.items.insert(item.path.clone(), item);
            }
            // Require parents so every item is discoverable from its namespace.
            for path in items.items.keys() {
                if path.len() > 1 && !items.items.contains_key(&path[..path.len() - 1]) {
                    bail!("reference {} has no parent item", path.join("::"));
                }
            }
            let mut failures = Vec::new();
            for problem in contribution.problems {
                failures.push((
                    source(&scope, &problem.file, problem.line)?,
                    problem.line,
                    problem.message,
                ));
            }
            Ok((items, failures))
        })();
        match result {
            Ok((items, failures)) => {
                *self.items.write().unwrap() = Arc::new(items);
                *self.failures.write().unwrap() = failures;
            }
            Err(error) => {
                *self.items.write().unwrap() = Arc::default();
                *self.failures.write().unwrap() = vec![(
                    binding.input(),
                    1,
                    format!("{} reference provider: {error:#}", self.name),
                )];
            }
        }
    }
}
