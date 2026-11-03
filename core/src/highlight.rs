//! Syntax highlighting for fenced code blocks, done while the page renders.
//!
//! Code is wrapped in spans whose classes name syntax scopes (`hl-keyword`,
//! `hl-string`…); the stylesheet colors those classes from theme tokens, so
//! highlighting follows the light and dark themes like everything else. Mermaid
//! blocks are left as plain text for `diagrams` to draw.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};

use comrak::adapters::SyntaxHighlighterAdapter;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::html::escape;

/// The languages bat ships, which include Swift, TypeScript, TOML and YAML
/// besides syntect's defaults. Loaded once, on first use.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

const MERMAID: &str = "mermaid";

thread_local! {
    static CUSTOM: std::cell::RefCell<Option<Arc<SyntaxSet>>> = const { std::cell::RefCell::new(None) };
}
pub fn with_grammars<T>(grammars: &[String], render: impl FnOnce() -> T) -> T {
    if grammars.is_empty() {
        return render();
    }
    static REGISTERED: LazyLock<Mutex<std::collections::BTreeMap<Vec<String>, Arc<SyntaxSet>>>> =
        LazyLock::new(Default::default);
    let mut cache = REGISTERED.lock().unwrap();
    let syntaxes = cache
        .entry(grammars.to_vec())
        .or_insert_with(|| {
            let mut builder = SYNTAXES.clone().into_builder();
            for grammar in grammars {
                if let Ok(syntax) =
                    syntect::parsing::SyntaxDefinition::load_from_str(grammar, true, None)
                {
                    builder.add(syntax);
                }
            }
            Arc::new(builder.build())
        })
        .clone();
    drop(cache);
    struct Restore(Option<Arc<SyntaxSet>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CUSTOM.with(|c| {
                c.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(CUSTOM.with(|c| c.replace(Some(syntaxes))));
    render()
}
pub(crate) fn builtin_language(token: &str) -> bool {
    SYNTAXES.find_syntax_by_token(token).is_some()
}
pub struct Highlighter;

/// `code` highlighted as `lang`, as the HTML that goes inside a `<code>`.
pub fn highlight(lang: &str, code: &str) -> String {
    let mut html = String::new();
    let _ = Highlighter.write_highlighted(&mut html, Some(lang), code);
    html
}

impl SyntaxHighlighterAdapter for Highlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let lang = lang
            .and_then(|info| info.split_whitespace().next())
            .unwrap_or("");
        CUSTOM.with(|custom| {
            let custom = custom.borrow();
            let syntaxes = custom.as_deref().unwrap_or(&SYNTAXES);
            match syntaxes
                .find_syntax_by_token(lang)
                .filter(|_| lang != MERMAID)
            {
                Some(syntax) => {
                    let mut html = ClassedHTMLGenerator::new_with_class_style(
                        syntax,
                        syntaxes,
                        ClassStyle::SpacedPrefixed { prefix: "hl-" },
                    );
                    for line in LinesWithEndings::from(code) {
                        if html
                            .parse_html_for_line_which_includes_newline(line)
                            .is_err()
                        {
                            return output.write_str(&escape(code));
                        }
                    }
                    output.write_str(&html.finalize())
                }
                None => output.write_str(&escape(code)),
            }
        })
    }

    /// Only Mermaid blocks keep their language, so `diagrams` can find them; other
    /// blocks carry no language label.
    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        match attributes.get("lang") {
            Some(lang) if lang.split_whitespace().next() == Some(MERMAID) => {
                output.write_str("<pre lang=\"mermaid\">")
            }
            _ => output.write_str("<pre>"),
        }
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        _attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        output.write_str("<code>")
    }
}
