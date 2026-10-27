//! Widget tags in Markdown: each known widget's tag becomes a placeholder
//! element carrying its name, its props' source and its content, which comrak
//! renders as Markdown; code is left alone, `<Notes>` taken out, and every
//! unknown or misplaced tag reported. Text in, text out.

use crate::markup::{
    NOTES, UNREADABLE_NOTES, code_ranges, find_close, parse_open, unreadable_notes,
};

/// System widgets that show their own source: they also get it highlighted
/// as Markdown, as the `sourceHtml` prop.
const SHOWS_SOURCE: &[&str] = &["Example"];

/// Replaces the tags of the widgets named in `known` in Markdown with placeholder elements,
/// leaving code alone, and takes `<Notes>` out. An unknown capitalized tag,
/// and a `Tab` outside a `Tabs`, are reported; an unknown tag is replaced by
/// an error in its place rather than passed through as HTML.
pub fn expand(source: &str, known: &[String], problems: &mut Vec<String>) -> String {
    expand_in(source, known, problems, None, 0)
}

/// [`expand`] for content inside the widget `parent`, whose first line is line
/// `lines_before + 1` of the page.
fn expand_in(
    source: &str,
    known: &[String],
    problems: &mut Vec<String>,
    parent: Option<&str>,
    lines_before: usize,
) -> String {
    let code = code_ranges(source);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    while let Some(offset) = source[at..].find('<') {
        let start = at + offset;
        out.push_str(&source[at..start]);
        at = start + 1;
        let is_open = bytes.get(start + 1).is_some_and(|b| b.is_ascii_uppercase());
        if !is_open || in_code(start) {
            out.push('<');
            continue;
        }
        let line = lines_before + source[..start].matches('\n').count() + 1;
        let Some(tag) = parse_open(&source[start..]) else {
            // A `<Notes>` whose attributes don't parse still never reaches the
            // slide: it is reported and taken out up to its `</Notes>`.
            if let Some(len) = unreadable_notes(&source[start..]) {
                problems.push(format!("{line}: {UNREADABLE_NOTES}"));
                at = find_close(source, start + len, NOTES, &in_code)
                    .map_or(source.len(), |(_, end)| end);
                continue;
            }
            out.push('<');
            continue;
        };
        let after_open = start + tag.len;
        let (inner, end, closed) = if tag.self_closing {
            ("", after_open, true)
        } else {
            match find_close(source, after_open, &tag.name, &in_code) {
                Some((close_start, close_end)) => {
                    (&source[after_open..close_start], close_end, true)
                }
                None => ("", after_open, false),
            }
        };
        // A widget is a block when it has its lines to itself: nothing before
        // its open tag and nothing after its close tag. Anywhere else it is
        // inline, part of a paragraph.
        let starts_line = source[..start]
            .rsplit('\n')
            .next()
            .is_some_and(|before| before.trim().is_empty());
        let ends_line = source[end..]
            .split('\n')
            .next()
            .is_some_and(|after| after.trim().is_empty());
        let block = starts_line && ends_line;
        let element = if block { "div" } else { "span" };
        // A slide's notes are never shown: [`notes`] reads them. Unclosed,
        // they run to the end, so none of their text reaches the slide.
        if tag.name == NOTES {
            if !closed {
                problems.push(format!("{line}: a <{NOTES}> without its </{NOTES}>"));
            }
            if tag.has_prop("delivery") && tag.string_prop("delivery").is_none() {
                problems.push(format!(
                    "{line}: a <{NOTES}> delivery must be a quoted string"
                ));
            }
            at = if closed { end } else { source.len() };
            continue;
        }
        if !known.contains(&tag.name) {
            let message = format!("<{}> is not a widget", tag.name);
            problems.push(format!("{line}: {message}"));
            out.push_str(&format!(
                r#"<{element} class="widget-error">{}</{element}>"#,
                escape(&message)
            ));
            at = end;
            continue;
        }
        if tag.name == "Tab" && parent != Some("Tabs") {
            problems.push(format!("{line}: <Tab> is outside a <Tabs>"));
        }
        let inner_lines = lines_before + source[..after_open].matches('\n').count();
        let icons;
        let inner = if tag.name == "Cards" {
            icons = card_icons(inner, inner_lines, problems);
            icons.as_str()
        } else {
            inner
        };
        let props = escape(&tag.props);
        let source_html = if SHOWS_SOURCE.contains(&tag.name.as_str()) {
            format!(
                r#" data-widget-source-html="{}""#,
                escape(&crate::highlight::highlight(
                    "markdown",
                    inner.trim_matches('\n')
                ))
            )
        } else {
            String::new()
        };
        let open = format!(
            r#"<{element} data-widget="{name}" data-props="{props}" data-widget-source="{src}"{source_html}>"#,
            name = tag.name,
            src = escape(inner.trim_matches('\n')),
        );
        // Widgets inside the content are expanded too, then left for comrak to
        // render as Markdown: blank lines around a block widget's content
        // make it Markdown rather than raw HTML.
        let inner = expand_in(inner, known, problems, Some(&tag.name), inner_lines);
        if block {
            out.push_str(&format!(
                "{open}\n\n{}\n\n</{element}>",
                inner.trim_matches('\n')
            ));
        } else {
            out.push_str(&format!("{open}{inner}</{element}>"));
        }
        at = end;
    }
    out.push_str(&source[at..]);
    out
}

/// A `Cards` list item may start with a Lucide `(icon)` before its link:
/// drawn here, on the server, as inline SVG the widget picks up. An unknown
/// name is a problem and is dropped. `lines_before` counts the page's lines
/// before the one the content starts on.
fn card_icons(inner: &str, lines_before: usize, problems: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(inner.len());
    for (i, line) in inner.split_inclusive('\n').enumerate() {
        let body = line.trim_start();
        let rest = body
            .strip_prefix("- ")
            .or_else(|| body.strip_prefix("* "))
            .or_else(|| body.strip_prefix("+ "));
        let icon = rest.and_then(|rest| {
            let name = rest.strip_prefix('(')?.split_once(')')?.0;
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            valid.then_some(name)
        });
        let Some(name) = icon else {
            out.push_str(line);
            continue;
        };
        let indent = &line[..line.len() - body.len()];
        let after = &body[2 + name.len() + 2..];
        let drawn = match crate::icons::svg(name) {
            Some(svg) => format!("{svg} "),
            None => {
                problems.push(format!("{}: unknown icon ({name})", lines_before + i + 1));
                String::new()
            }
        };
        out.push_str(&format!(
            "{indent}{}{drawn}{}",
            &body[..2],
            after.trim_start()
        ));
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        // An attribute stays on one line, or a blank line in it would end the
        // HTML block before the widget's content.
        .replace('\n', "&#10;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of a widget set holding `names`.
    fn set(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn expands_a_block_widget_with_markdown_content() {
        let mut problems = Vec::new();
        let out = expand(
            "<Callout kind=\"warning\">\nSome **markdown**\n</Callout>\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(
            out.contains(
                r#"<div data-widget="Callout" data-props="{&quot;kind&quot;: &quot;warning&quot;}""#
            ),
            "{out}"
        );
        assert!(out.contains("\n\nSome **markdown**\n\n</div>"), "{out}");
        assert!(problems.is_empty());
        let html = crate::render::markdown(&out);
        assert!(html.contains("<strong>markdown</strong>"), "{html}");
    }

    #[test]
    fn parses_expression_bare_and_self_closing_props() {
        let mut problems = Vec::new();
        let out = expand(
            "<Chart values={[3, {a: \"}\"}]} stacked title='T' />",
            &set(&["Chart"]),
            &mut problems,
        );
        assert!(out.contains(r#"data-props="{&quot;values&quot;: ([3, {a: &quot;}&quot;}]), &quot;stacked&quot;: true, &quot;title&quot;: &quot;T&quot;}""#), "{out}");
    }

    #[test]
    fn leaves_code_alone_and_reports_unknown_tags() {
        let mut problems = Vec::new();
        let out = expand(
            "`<Callout>` and\n\n```\n<Callout>x</Callout>\n```\n\n<Nope />\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(!out.contains("data-widget"), "{out}");
        assert_eq!(problems, vec!["7: <Nope> is not a widget".to_string()]);
    }

    #[test]
    fn leaves_indented_code_and_html_comments_alone() {
        let mut problems = Vec::new();
        let source = "Code:\n\n    let v: Vec<String> = Vec::new();\n    <Callout>shown as code</Callout>\n\nAfter.\n\n<!-- old: <Todo> -->\n<!--\n<Callout>draft</Callout>\n-->\n";
        let out = expand(source, &set(&["Callout"]), &mut problems);
        assert_eq!(out, source, "{out}");
        assert!(problems.is_empty(), "{problems:?}");
        let html = crate::render::markdown(&out);
        assert!(
            html.contains("let v: Vec&lt;String&gt; = Vec::new();"),
            "{html}"
        );
        assert!(
            html.contains("&lt;Callout&gt;shown as code&lt;/Callout&gt;"),
            "{html}"
        );
    }

    #[test]
    fn indented_lines_in_a_list_are_not_code() {
        let mut problems = Vec::new();
        let out = expand(
            "- item\n\n    more of the item with <Callout>x</Callout>\n",
            &set(&["Callout"]),
            &mut problems,
        );
        assert!(out.contains("data-widget=\"Callout\""), "{out}");
    }

    #[test]
    fn an_unknown_tag_is_replaced_by_an_error_not_passed_through() {
        let mut problems = Vec::new();
        let out = expand(
            "Before.\n\n<Nope>hello</Nope>\n\nSay <Gone /> inline.\n",
            &set(&["Callout"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(!html.contains("<nope>"), "{html}");
        assert!(!html.contains("hello"), "{html}");
        assert!(!html.contains("<gone"), "{html}");
        assert!(
            html.contains(r#"<div class="widget-error">&lt;Nope&gt; is not a widget</div>"#),
            "{html}"
        );
        assert!(
            html.contains(
                r#"Say <span class="widget-error">&lt;Gone&gt; is not a widget</span> inline."#
            ),
            "{html}"
        );
        assert_eq!(
            problems,
            vec![
                "3: <Nope> is not a widget".to_string(),
                "5: <Gone> is not a widget".to_string()
            ]
        );
    }

    #[test]
    fn a_tab_outside_a_tabs_is_a_problem_with_its_page_line() {
        let mut problems = Vec::new();
        let widgets = set(&["Tabs", "Tab", "Callout"]);
        expand(
            "# T\n\n<Tabs>\n<Tab title=\"a\">\nfine\n</Tab>\n</Tabs>\n\n<Callout>\n\n<Tab title=\"b\">\nlost\n</Tab>\n\n</Callout>\n\n<Tab title=\"c\">x</Tab>\n",
            &widgets,
            &mut problems,
        );
        assert_eq!(
            problems,
            vec![
                "11: <Tab> is outside a <Tabs>".to_string(),
                "17: <Tab> is outside a <Tabs>".to_string()
            ]
        );
    }

    #[test]
    fn nests_widgets_and_keeps_inline_ones_inline() {
        let mut problems = Vec::new();
        let out = expand(
            "<Tabs>\n<Tab title=\"a\">\n\nPress <Keys>⌘ P</Keys>.\n\n</Tab>\n</Tabs>\n",
            &set(&["Tabs", "Tab", "Keys"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(html.contains(r#"<div data-widget="Tabs""#), "{html}");
        assert!(html.contains(r#"<div data-widget="Tab""#), "{html}");
        assert!(
            html.contains(
                r#"<span data-widget="Keys" data-props="{}" data-widget-source="⌘ P">⌘ P</span>"#
            ),
            "{html}"
        );
        // `data-source` names a page's Markdown file, which comments are
        // asked for; a widget's own source is not one.
        assert!(!html.contains("data-source"), "{html}");
    }

    #[test]
    fn an_inline_widget_at_the_start_of_a_line_stays_in_its_paragraph() {
        let mut problems = Vec::new();
        let out = expand(
            "<Keys>⌘ P</Keys> to **print**, then <Keys>⏎</Keys>.\n",
            &set(&["Keys"]),
            &mut problems,
        );
        let html = crate::render::markdown(&out);
        assert!(html.starts_with("<p><span data-widget=\"Keys\""), "{html}");
        assert!(html.contains("<strong>print</strong>"), "{html}");
        assert_eq!(html.matches("data-widget=\"Keys\"").count(), 2, "{html}");
    }

    #[test]
    fn an_example_gets_its_source_highlighted() {
        let mut problems = Vec::new();
        let out = expand(
            "<Example>\nA **word**\n</Example>\n",
            &set(&["Example"]),
            &mut problems,
        );
        assert!(out.contains("data-widget-source-html=\""), "{out}");
        assert!(out.contains("hl-"), "{out}");
        let callout = expand("<Callout>x</Callout>\n", &set(&["Callout"]), &mut problems);
        assert!(!callout.contains("data-widget-source-html"), "{callout}");
    }

    #[test]
    fn a_cards_icon_is_drawn_and_an_unknown_one_is_a_problem() {
        let mut problems = Vec::new();
        let out = expand(
            "# T\n\n<Cards>\n\n- (rocket) [Start](a.md) — begin here\n- [Plain](b.md) — no icon\n- (no-such-icon) [Odd](c.md)\n\n</Cards>\n",
            &set(&["Cards"]),
            &mut problems,
        );
        assert!(out.contains(r#"<svg class="icon""#), "{out}");
        assert!(!out.contains("(rocket)"), "{out}");
        assert!(out.contains("[Plain](b.md)"), "{out}");
        assert_eq!(problems, vec!["7: unknown icon (no-such-icon)".to_string()]);
    }

    #[test]
    fn notes_are_taken_out_of_the_page_and_their_text_kept() {
        let source = "# Title\n\n<Notes>\n  Say **this**, word for word.\n</Notes>\n\nAfter.\n";
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&["Callout"]), &mut problems));
        assert!(!html.contains("word for word"), "{html}");
        assert!(!html.contains("widget-error"), "{html}");
        assert!(html.contains("<p>After.</p>"), "{html}");
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn unclosed_notes_are_a_problem_and_never_shown() {
        let source = "# Title\n\nShown.\n\n<Notes>\nSay this.\n</Note>\n\nAfter.\n";
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
        assert!(html.contains("<p>Shown.</p>"), "{html}");
        assert!(!html.contains("Say this"), "{html}");
        assert!(!html.contains("After."), "{html}");
        assert_eq!(
            problems,
            vec!["5: a <Notes> without its </Notes>".to_string()]
        );
    }

    #[test]
    fn a_notes_delivery_is_kept_and_never_shown() {
        let source = "<Notes delivery=\"Honest and a little teasing: you caught them.\">\nSay this.\n</Notes>\n";
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(!html.contains("teasing"), "{html}");
    }

    #[test]
    fn a_notes_with_unreadable_attributes_is_a_problem_and_never_shown() {
        for source in [
            "<Notes delivery=\"the \"aha\" moment\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
            "<Notes delivery=\"say \\\"wow\\\" now\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
        ] {
            let mut problems = Vec::new();
            let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
            assert!(
                !html.contains("SAYTHIS") && !html.contains("Notes"),
                "{html}"
            );
            assert!(html.contains("<p>After.</p>"), "{html}");
            assert_eq!(problems, vec![format!("1: {UNREADABLE_NOTES}")]);
        }
    }

    #[test]
    fn a_notes_delivery_that_is_not_a_quoted_string_is_a_problem() {
        let source = "<Notes delivery={\"warm\"}>\nSay this.\n</Notes>\n";
        let mut problems = Vec::new();
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut problems));
        assert!(!html.contains("Say this"), "{html}");
        assert_eq!(
            problems,
            vec!["1: a <Notes> delivery must be a quoted string".to_string()]
        );
    }

    #[test]
    fn notes_inside_code_and_comments_are_not_notes() {
        let source = "`<Notes>x</Notes>`\n\n```\n<Notes>fenced</Notes>\n```\n\n    <Notes>indented</Notes>\n\n<!-- <Notes>hidden</Notes> -->\n";
        let html = crate::render::markdown(&expand(source, &set(&[]), &mut Vec::new()));
        assert!(
            html.contains("fenced") && html.contains("indented"),
            "{html}"
        );
    }
}
