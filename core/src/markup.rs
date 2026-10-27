//! Reading a page's Markdown as text, before it is rendered: its front
//! matter, its title, its `<Notes>`, and the scanner that finds widget tags
//! outside code. A leaf: it depends on nothing else in devpage, so documents,
//! rendering, widgets and checking can all use it.

use std::path::Path;

/// The tag a slide's speaker notes are written in.
pub const NOTES: &str = "Notes";

/// A page's front matter's YAML, and the page after it.
pub fn split_front_matter(source: &str) -> Option<(&str, &str)> {
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

/// A page's title: its first level-one heading, or its file name.
pub fn title(source: &str, path: &Path) -> String {
    split_front_matter(source)
        .map_or(source, |(_, page)| page)
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|heading| heading.trim().to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

/// One `<Notes>` block: the line it starts on, its text, trimmed, as written,
/// and its `delivery` attribute — how the narrator should sound saying it.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub line: usize,
    pub text: String,
    pub delivery: Option<String>,
}

/// Each `<Notes>…</Notes>` block in a page's Markdown, outside code and HTML
/// comments, as found by [`expand`]. A `<Notes>` without its `</Notes>` runs
/// to the end of the page.
pub fn notes(source: &str) -> Vec<Note> {
    let code = code_ranges(source);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let open = format!("<{NOTES}");
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = source[at..].find(&open) {
        let start = at + offset;
        at = start + 1;
        if in_code(start) {
            continue;
        }
        let Some(tag) = parse_open(&source[start..])
            .filter(|tag| tag.name == NOTES)
            .or_else(|| {
                unreadable_notes(&source[start..]).map(|len| OpenTag {
                    name: NOTES.to_string(),
                    props: "{}".to_string(),
                    len,
                    self_closing: false,
                })
            })
        else {
            continue;
        };
        let after_open = start + tag.len;
        let (inner, end) = if tag.self_closing {
            ("", after_open)
        } else {
            match find_close(source, after_open, NOTES, &in_code) {
                Some((close_start, close_end)) => (&source[after_open..close_start], close_end),
                None => (&source[after_open..], source.len()),
            }
        };
        found.push(Note {
            line: source[..start].matches('\n').count() + 1,
            text: inner.trim().to_string(),
            delivery: tag.string_prop("delivery"),
        });
        at = end;
    }
    found
}

pub(crate) struct OpenTag {
    pub(crate) name: String,
    /// The props as a JavaScript object literal.
    pub(crate) props: String,
    pub(crate) len: usize,
    pub(crate) self_closing: bool,
}

/// What is wrong with a `<Notes …>` whose attributes don't parse.
pub(crate) const UNREADABLE_NOTES: &str =
    "a <Notes> whose attributes can't be read: write delivery=\"…\" with no double quotes inside";

/// The length of a `<Notes …>` open tag that [`parse_open`] can't read — an
/// attribute with a stray quote — up to its first `>`; `None` for anything
/// else.
pub(crate) fn unreadable_notes(text: &str) -> Option<usize> {
    let rest = text.strip_prefix(&format!("<{NOTES}"))?;
    if !rest.starts_with(char::is_whitespace) || parse_open(text).is_some() {
        return None;
    }
    text.find('>').map(|end| end + 1)
}

impl OpenTag {
    /// Whether the tag has a `key` attribute, of any form.
    pub(crate) fn has_prop(&self, key: &str) -> bool {
        self.props.contains(&format!("\"{key}\": "))
    }

    /// The string a `key="…"` attribute holds, if the tag has one.
    pub(crate) fn string_prop(&self, key: &str) -> Option<String> {
        let at = self.props.find(&format!("\"{key}\": \""))? + key.len() + 4;
        let mut strings =
            serde_json::Deserializer::from_str(&self.props[at..]).into_iter::<String>();
        strings.next()?.ok()
    }
}

/// Parses `<Name a="x" b={expr} c>` or `<Name … />` at the start of `text`.
pub(crate) fn parse_open(text: &str) -> Option<OpenTag> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 1;
    let mut name = String::new();
    while let Some(&(_, c)) = chars.get(i) {
        if c.is_ascii_alphanumeric() {
            name.push(c);
            i += 1;
        } else {
            break;
        }
    }
    let mut props = Vec::new();
    loop {
        while chars.get(i).is_some_and(|(_, c)| c.is_whitespace()) {
            i += 1;
        }
        let &(_, c) = chars.get(i)?;
        if c == '>' {
            return Some(OpenTag {
                name,
                props: format!("{{{}}}", props.join(", ")),
                len: chars.get(i + 1).map_or(text.len(), |(at, _)| *at),
                self_closing: false,
            });
        }
        if c == '/' && chars.get(i + 1).is_some_and(|(_, c)| *c == '>') {
            return Some(OpenTag {
                name,
                props: format!("{{{}}}", props.join(", ")),
                len: chars.get(i + 2).map_or(text.len(), |(at, _)| *at),
                self_closing: true,
            });
        }
        let mut attr = String::new();
        while let Some(&(_, c)) = chars.get(i) {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                attr.push(c);
                i += 1;
            } else {
                break;
            }
        }
        if attr.is_empty() {
            return None;
        }
        let key = serde_json::to_string(&attr).ok()?;
        if chars.get(i).is_some_and(|(_, c)| *c == '=') {
            i += 1;
            match chars.get(i)?.1 {
                quote @ ('"' | '\'') => {
                    let mut value = String::new();
                    i += 1;
                    while let Some(&(_, c)) = chars.get(i) {
                        i += 1;
                        if c == quote {
                            break;
                        }
                        value.push(c);
                    }
                    props.push(format!("{key}: {}", serde_json::to_string(&value).ok()?));
                }
                '{' => {
                    let mut depth = 0;
                    let mut expr = String::new();
                    let mut string: Option<char> = None;
                    while let Some(&(_, c)) = chars.get(i) {
                        i += 1;
                        if let Some(q) = string {
                            if c == q {
                                string = None;
                            }
                        } else if matches!(c, '"' | '\'' | '`') {
                            string = Some(c);
                        } else if c == '{' {
                            depth += 1;
                            if depth == 1 {
                                continue;
                            }
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        expr.push(c);
                    }
                    props.push(format!("{key}: ({expr})"));
                }
                _ => return None,
            }
        } else {
            props.push(format!("{key}: true"));
        }
    }
}

/// Finds `</Name>` matching an open tag, counting nested ones of the same
/// name and skipping code.
pub(crate) fn find_close(
    source: &str,
    from: usize,
    name: &str,
    in_code: &dyn Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut depth = 1;
    let mut at = from;
    loop {
        let next_close = source[at..].find(&close).map(|o| at + o)?;
        let next_open = source[at..]
            .find(&open)
            .map(|o| at + o)
            .filter(|&o| o < next_close);
        if let Some(o) = next_open {
            at = o + open.len();
            let boundary = source[at..].starts_with(|c: char| c.is_whitespace() || c == '>');
            if !in_code(o) && boundary {
                depth += 1;
            }
            continue;
        }
        at = next_close + close.len();
        if in_code(next_close) {
            continue;
        }
        depth -= 1;
        if depth == 0 {
            return Some((next_close, at));
        }
    }
}

/// Byte ranges of fenced code blocks and inline code spans.
pub(crate) fn code_ranges(source: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(usize, String)> = None;
    let mut offset = 0;
    // An indented code block starts after a blank line, outside a list, and
    // runs while lines stay indented by four spaces or a tab (or are blank).
    let mut previous_blank = true;
    let mut in_list = false;
    let mut indented: Option<usize> = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let blank = trimmed.is_empty();
        let is_indented = line.starts_with("    ") || line.starts_with('\t');
        if fence.is_none() {
            if let Some(start) = indented {
                if !blank && !is_indented {
                    ranges.push(start..offset);
                    indented = None;
                }
            } else if is_indented && !blank && previous_blank && !in_list {
                indented = Some(offset);
            }
            if indented.is_some() {
                previous_blank = blank;
                offset += line.len();
                continue;
            }
            if !blank && !is_indented {
                in_list = is_list_item(trimmed);
            }
        }
        previous_blank = blank;
        let marker: String = trimmed
            .chars()
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        match &fence {
            Some((start, open)) => {
                if marker.len() >= open.len() && marker.starts_with(&open[..1]) {
                    ranges.push(*start..offset + line.len());
                    fence = None;
                }
            }
            None if marker.len() >= 3 => fence = Some((offset, marker)),
            None => {
                // Inline code spans on this line.
                let mut i = 0;
                let b = line.as_bytes();
                while i < b.len() {
                    if b[i] == b'`' {
                        let run = b[i..].iter().take_while(|c| **c == b'`').count();
                        let ticks = "`".repeat(run);
                        if let Some(end) = line[i + run..].find(&ticks) {
                            let close = i + run + end + run;
                            ranges.push(offset + i..offset + close);
                            i = close;
                            continue;
                        }
                        i += run;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        offset += line.len();
    }
    if let Some((start, _)) = fence {
        ranges.push(start..source.len());
    }
    if let Some(start) = indented {
        ranges.push(start..source.len());
    }
    // HTML comments, outside code, hide whatever they hold.
    let mut from = 0;
    while let Some(open) = source[from..].find("<!--") {
        let open = from + open;
        if ranges.iter().any(|r| r.contains(&open)) {
            from = open + 4;
            continue;
        }
        let close = source[open + 4..]
            .find("-->")
            .map_or(source.len(), |end| open + 4 + end + 3);
        ranges.push(open..close);
        from = close;
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// Whether a line, without its indentation, starts a Markdown list item.
pub(crate) fn is_list_item(trimmed: &str) -> bool {
    let bullet =
        trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ");
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    bullet || (digits > 0 && matches!(trimmed[digits..].chars().next(), Some('.' | ')')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_keep_their_text_without_its_indentation() {
        let source = "# Title\n\n<Notes>\n  Say **this**, word for word.\n</Notes>\n\nAfter.\n";
        assert_eq!(
            notes(source),
            vec![Note {
                line: 3,
                text: "Say **this**, word for word.".to_string(),
                delivery: None
            }]
        );
    }

    #[test]
    fn unclosed_notes_run_to_the_end_of_the_page() {
        let source = "# Title\n\nShown.\n\n<Notes>\nSay this.\n</Note>\n\nAfter.\n";
        assert_eq!(
            notes(source)[0].text,
            "Say this.\n</Note>\n\nAfter.".to_string()
        );
    }

    #[test]
    fn a_notes_delivery_is_kept() {
        let source = "<Notes delivery=\"Honest and a little teasing: you caught them.\">\nSay this.\n</Notes>\n";
        assert_eq!(
            notes(source)[0].delivery.as_deref(),
            Some("Honest and a little teasing: you caught them.")
        );
    }

    #[test]
    fn a_notes_with_unreadable_attributes_keeps_its_text_and_no_delivery() {
        for source in [
            "<Notes delivery=\"the \"aha\" moment\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
            "<Notes delivery=\"say \\\"wow\\\" now\">\nSAYTHIS\n</Notes>\n\nAfter.\n",
            "<Notes delivery={\"warm\"}>\nSAYTHIS\n</Notes>\n",
        ] {
            assert_eq!(notes(source)[0].text, "SAYTHIS");
            assert_eq!(notes(source)[0].delivery, None);
        }
    }

    #[test]
    fn notes_inside_code_and_comments_are_not_notes() {
        let source = "`<Notes>x</Notes>`\n\n```\n<Notes>fenced</Notes>\n```\n\n    <Notes>indented</Notes>\n\n<!-- <Notes>hidden</Notes> -->\n";
        assert!(notes(source).is_empty(), "{:?}", notes(source));
    }

    #[test]
    fn every_notes_block_is_found_with_its_line() {
        let found = notes("<Notes>one</Notes>\n\ntext\n\n<Notes>\ntwo\n</Notes>\n<Notes/>\n");
        let found: Vec<(usize, String)> = found.into_iter().map(|n| (n.line, n.text)).collect();
        assert_eq!(
            found,
            vec![
                (1, "one".to_string()),
                (5, "two".to_string()),
                (8, String::new())
            ]
        );
    }
}
