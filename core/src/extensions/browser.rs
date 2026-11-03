//! Package-owned React modules, compiled through the same TSX compiler as widgets.
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

/// An extension package's browser side: its modules compiled to JavaScript, its
/// widgets and its styles, served under an address that changes with their
/// content.
#[derive(Clone, Default)]
pub(super) struct Browser {
    /// The hash of the build and the package's sources, widgets and styles: the
    /// address segment that changes whenever any of them does.
    digest: String,
    /// Each module's compiled JavaScript, by its path in the package.
    modules: BTreeMap<String, String>,
    /// Each widget's module path, by widget name.
    widgets: BTreeMap<String, String>,
    /// Each stylesheet's CSS, by its path in the package.
    styles: BTreeMap<String, String>,
}
/// `path` with `.` and `..` resolved, as `/`-joined parts. An error when it
/// climbs out of the package or isn't relative.
fn normalized(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    bail!("Module import escapes package");
                }
            }
            _ => bail!("Module path must be package-relative"),
        }
    }
    Ok(parts.join("/"))
}
impl Browser {
    /// The package `descriptor` describes, with every module compiled and its
    /// relative imports rewritten to addresses under `base`. An error when a
    /// path is noncanonical, an import is missing, or a widget is misnamed,
    /// missing or replaces a built-in.
    pub fn new(descriptor: &Value, base: &str) -> Result<Self> {
        let sources: BTreeMap<String, String> =
            serde_json::from_value(descriptor.get("modules").cloned().unwrap_or(json!({})))?;
        let widgets: BTreeMap<String, String> =
            serde_json::from_value(descriptor.get("widgets").cloned().unwrap_or(json!({})))?;
        let styles: BTreeMap<String, String> =
            serde_json::from_value(descriptor.get("styles").cloned().unwrap_or(json!({})))?;
        let mut hash = Sha256::new();
        hash.update(crate::addresses::BUILD);
        hash.update(serde_json::to_vec(&json!([sources, widgets, styles]))?);
        let digest = format!("{:x}", hash.finalize());
        let prefix = format!("{base}browser/{digest}/");
        let mut modules = BTreeMap::new();
        for (path, source) in &sources {
            if normalized(Path::new(path))? != *path {
                bail!("Noncanonical package module {path}");
            }
            let parent = Path::new(path).parent().unwrap_or(Path::new(""));
            let mut imports = BTreeMap::new();
            for specifier in crate::widgets::extension_imports(Path::new(path), source) {
                let requested = normalized(&parent.join(&specifier))?;
                let target = ["", ".tsx", ".ts", ".js"]
                    .into_iter()
                    .map(|suffix| format!("{requested}{suffix}"))
                    .find(|p| sources.contains_key(p))
                    .ok_or_else(|| anyhow!("{path}: missing package module {specifier}"))?;
                imports.insert(specifier, format!("{prefix}modules/{target}"));
            }
            let js = crate::widgets::compile_extension(Path::new(path), source, &[], &imports)
                .map_err(|e| anyhow!("{path}: {e:#}"))?;
            modules.insert(path.clone(), js);
        }
        for path in styles.keys() {
            if normalized(Path::new(path))? != *path {
                bail!("Noncanonical package style {path}");
            }
        }
        for (name, path) in &widgets {
            if !name.starts_with(char::is_uppercase)
                || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                bail!("Invalid package widget name {name}");
            }
            if !modules.contains_key(path) {
                bail!("Widget {name} module {path} missing");
            }
            if crate::widgets::SYSTEM.iter().any(|(n, _)| n == name) {
                bail!("Package widget {name} replaces a built-in");
            }
        }
        Ok(Self {
            digest,
            modules,
            widgets,
            styles,
        })
    }
    /// Each widget's module address under `base`, by widget name.
    pub fn widgets(&self, base: &str) -> BTreeMap<String, String> {
        self.widgets
            .iter()
            .map(|(n, p)| {
                (
                    n.clone(),
                    format!("{base}browser/{}/modules/{p}", self.digest),
                )
            })
            .collect()
    }
    /// Every stylesheet's address under `base`.
    pub fn styles(&self, base: &str) -> Vec<String> {
        self.styles
            .keys()
            .map(|p| format!("{base}browser/{}/styles/{p}", self.digest))
            .collect()
    }
    /// The response to `path`, relative to the extension's base: a compiled
    /// module or a stylesheet. `None` when `path` names neither, or an older
    /// digest.
    pub fn response(&self, path: &str) -> Option<Value> {
        let path = path.strip_prefix(&format!("browser/{}/", self.digest))?;
        let (source, mime) = if let Some(name) = path.strip_prefix("modules/") {
            (self.modules.get(name)?, "text/javascript")
        } else {
            let name = path.strip_prefix("styles/")?;
            (self.styles.get(name)?, "text/css")
        };
        Some(json!({"body":source,"mime":mime,"status":200}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_modules_use_the_widget_compiler_and_own_relative_imports() {
        let descriptor = json!({"widgets":{"Tool":"Tool.tsx"},"modules":{
            "Tool.tsx":"import React from 'react'; import {label} from './text'; export default function Tool(){return <div>{label}</div>}",
            "text.ts":"export const label='A tool';"
        },"styles":{"tool.css":".example {color:inherit}"}});
        let browser =
            Browser::new(&descriptor, "/mounted/__devpage/extensions/owner/toy/").unwrap();
        let url = browser.widgets("/mounted/__devpage/extensions/owner/toy/")["Tool"].clone();
        let suffix = url.split("/toy/").nth(1).unwrap();
        let response = browser.response(suffix).unwrap();
        let js = response["body"].as_str().unwrap();
        assert!(js.contains("react/jsx-runtime"));
        assert!(js.contains("/modules/text.ts"));
        assert!(!js.contains(r#"from "./text""#));
        assert_eq!(browser.styles("/base/").len(), 1);
        assert!(browser.response("browser/wrong/modules/Tool.tsx").is_none());
        assert!(Browser::new(&json!({"widgets":{"Tool":"missing.tsx"}}), "/").is_err());
        assert!(
            Browser::new(
                &json!({"modules":{"../outside.ts":"export default 1;"}}),
                "/"
            )
            .is_err()
        );
        assert!(
            Browser::new(
                &json!({"modules":{"Tool.tsx":"import './missing'; export default ()=>null;"}}),
                "/"
            )
            .is_err()
        );
    }
}
