//! Icons by name, drawn as inline SVG in the text color: the Lucide set
//! (`assets/vendor/lucide.json`, ISC) and the few brand marks Lucide doesn't
//! carry, from Simple Icons (CC0).

use std::collections::HashMap;
use std::sync::LazyLock;

/// One SVG element of an icon: its tag and its attributes.
type Node = (String, Vec<(String, String)>);

/// One element as Lucide's JSON writes it: `[tag, {attribute: value}]`.
type RawNode = (String, serde_json::Map<String, serde_json::Value>);

/// Lucide's icons, each a list of SVG elements.
static LUCIDE: LazyLock<HashMap<String, Vec<Node>>> = LazyLock::new(|| {
    let raw: HashMap<String, Vec<RawNode>> =
        serde_json::from_str(include_str!("../assets/vendor/lucide.json")).unwrap_or_default();
    raw.into_iter()
        .map(|(name, nodes)| {
            let nodes = nodes
                .into_iter()
                .map(|(tag, attributes)| {
                    let attributes = attributes
                        .into_iter()
                        .map(|(key, value)| {
                            let value = match value {
                                serde_json::Value::String(text) => text,
                                other => other.to_string(),
                            };
                            (key, value)
                        })
                        .collect();
                    (tag, attributes)
                })
                .collect();
            (name, nodes)
        })
        .collect()
});

/// Brand marks Lucide leaves out, as one filled path on a 24 × 24 grid.
const BRANDS: &[(&str, &str)] = &[(
    "github",
    "M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12",
)];

/// Whether `name` is an icon devpage can draw.
pub fn exists(name: &str) -> bool {
    BRANDS.iter().any(|(brand, _)| *brand == name) || LUCIDE.contains_key(name)
}

/// The icon as inline SVG, sized by the text around it, or `None` when there
/// is no such icon.
pub fn svg(name: &str) -> Option<String> {
    if let Some((_, path)) = BRANDS.iter().find(|(brand, _)| *brand == name) {
        return Some(format!(
            r#"<svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="{path}"/></svg>"#
        ));
    }
    let nodes = LUCIDE.get(name)?;
    let mut body = String::new();
    for (tag, attributes) in nodes {
        body.push('<');
        body.push_str(tag);
        for (key, value) in attributes {
            body.push_str(&format!(r#" {key}="{}""#, value.replace('"', "&quot;")));
        }
        body.push_str("/>");
    }
    Some(format!(
        r#"<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{body}</svg>"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_lucide_icons_and_brand_marks_and_knows_unknown_names() {
        let book = svg("book-open").unwrap();
        assert!(book.starts_with("<svg class=\"icon\""), "{book}");
        assert!(book.contains("<path d="), "{book}");
        assert!(svg("github").unwrap().contains("fill=\"currentColor\""));
        assert!(exists("presentation") && exists("github"));
        assert!(!exists("no-such-icon"));
        assert_eq!(svg("no-such-icon"), None);
    }
}
