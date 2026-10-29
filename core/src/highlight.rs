//! Syntax highlighting for fenced code blocks, done while the page renders.
//!
//! Code is wrapped in spans whose classes name syntax scopes (`hl-keyword`,
//! `hl-string`…); the stylesheet colors those classes from theme tokens, so
//! highlighting follows the light and dark themes like everything else. Mermaid
//! blocks are left as plain text for `diagrams` to draw.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use comrak::adapters::SyntaxHighlighterAdapter;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::html::escape;

/// The languages bat ships, which include Swift, TypeScript, TOML and YAML
/// besides syntect's defaults. Loaded once, on first use.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

const MERMAID: &str = "mermaid";

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
        match SYNTAXES
            .find_syntax_by_token(lang)
            .filter(|_| lang != MERMAID)
        {
            Some(syntax) => {
                let mut html = ClassedHTMLGenerator::new_with_class_style(
                    syntax,
                    &SYNTAXES,
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
