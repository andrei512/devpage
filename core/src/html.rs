//! HTML as devpage reads and writes it: text escaped into it and back, a
//! tag's attributes, every value one attribute takes in a page, and what a
//! link's address names.

/// `text` safe in HTML: in an element's text and in a double-quoted
/// attribute value.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text `escape` was given back.
pub(crate) fn unescape(html: &str) -> String {
    html.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The length of the tag `html` starts with: up to and including its closing
/// `>`, skipping any `>` inside a quoted attribute value.
pub(crate) fn tag_end(html: &str) -> usize {
    let mut quote = None;
    for (i, c) in html.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return i + 1,
            _ => {}
        }
    }
    html.len()
}

/// A tag's attributes, from just after its name up to its closing `>`: each
/// name with its value — double-quoted, single-quoted, unquoted, or empty for
/// a bare name — separated by any whitespace. Values are as written, still
/// escaped.
pub(crate) fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let mut chars = tag.char_indices().peekable();
    loop {
        while chars
            .next_if(|(_, c)| c.is_ascii_whitespace() || *c == '/')
            .is_some()
        {}
        let Some(&(start, c)) = chars.peek() else {
            break;
        };
        if c == '>' {
            break;
        }
        let mut end = start;
        while let Some((i, c)) =
            chars.next_if(|(_, c)| !c.is_ascii_whitespace() && !matches!(c, '=' | '>' | '/'))
        {
            end = i + c.len_utf8();
        }
        let name = tag[start..end].to_string();
        while chars.next_if(|(_, c)| c.is_ascii_whitespace()).is_some() {}
        let mut value = String::new();
        if chars.next_if(|(_, c)| *c == '=').is_some() {
            while chars.next_if(|(_, c)| c.is_ascii_whitespace()).is_some() {}
            match chars.peek() {
                Some(&(_, quote @ ('"' | '\''))) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == quote {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some((_, c)) =
                        chars.next_if(|(_, c)| !c.is_ascii_whitespace() && *c != '>')
                    {
                        value.push(c);
                    }
                }
            }
        }
        if name.is_empty() {
            // Not a name at all (a stray `=`): skip one character and go on.
            chars.next();
            continue;
        }
        attributes.push((name, value));
    }
    attributes
}

/// Every value the attribute `name` takes in `html`'s tags, in order and
/// unescaped, its name matched in any case. Comments hold no tags, and text
/// holds none either: escaped, it has no `<`.
pub(crate) fn values(html: &str, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let named = rest[1..].len()
            - rest[1..]
                .trim_start_matches(|c: char| c.is_ascii_alphanumeric())
                .len();
        if named == 0 {
            rest = &rest[1..];
            continue;
        }
        let end = tag_end(rest);
        values.extend(
            attributes(&rest[1 + named..end])
                .into_iter()
                .filter(|(attribute, _)| attribute.eq_ignore_ascii_case(name))
                .map(|(_, value)| unescape(&value)),
        );
        rest = &rest[end..];
    }
    values
}

/// What a link's address names, read from how it is written.
#[derive(Debug, PartialEq)]
pub(crate) enum Link<'a> {
    /// An address on the web, `http:` or `https:`, as written.
    Web(&'a str),
    /// An address with any other scheme — `mailto:`, `tel:`, `data:` —
    /// which devpage neither follows nor rewrites.
    Elsewhere,
    /// A file, by its path from where the link is written and the fragment
    /// after any `#`. An empty path is the page itself.
    File { path: &'a str, fragment: &'a str },
}

impl<'a> Link<'a> {
    /// What `address` names. A scheme is letters, digits, `+`, `-` and `.`
    /// starting with a letter, before a `:` that comes ahead of any `/`, `?`
    /// or `#`; anything else is a path.
    pub(crate) fn classify(address: &'a str) -> Link<'a> {
        let scheme = address
            .split_once(':')
            .map(|(scheme, _)| scheme)
            .filter(|scheme| {
                scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            });
        match scheme {
            Some(scheme)
                if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
            {
                Link::Web(address)
            }
            Some(_) => Link::Elsewhere,
            None => {
                let (path, fragment) = address.split_once('#').unwrap_or((address, ""));
                Link::File { path, fragment }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_text_is_safe_in_text_and_in_a_quoted_attribute_and_comes_back() {
        let text = r#"<a href="x">Tom & "Jerry"</a>"#;
        let escaped = escape(text);
        assert_eq!(
            escaped,
            "&lt;a href=&quot;x&quot;&gt;Tom &amp; &quot;Jerry&quot;&lt;/a&gt;"
        );
        assert_eq!(unescape(&escaped), text);
        // An escaped `&lt;` stays text, not a tag.
        assert_eq!(unescape(&escape("&lt;")), "&lt;");
    }

    #[test]
    fn a_tags_attributes_are_read_however_they_are_quoted_and_spaced() {
        assert_eq!(
            attributes(r#" SRC = 'a b.png' alt="x > y" width=10 hidden/>"#),
            [
                ("SRC".to_string(), "a b.png".to_string()),
                ("alt".to_string(), "x > y".to_string()),
                ("width".to_string(), "10".to_string()),
                ("hidden".to_string(), String::new()),
            ]
        );
        assert_eq!(tag_end(r#"<a title="a>b">rest"#), 15);
    }

    #[test]
    fn every_value_of_an_attribute_is_found_in_tags_only() {
        let html = concat!(
            r#"<p><a href="one.md">1</a> <a class="x" HREF='two.md?a=1&amp;b=2'>2</a></p>"#,
            r#"<!-- <a href="commented.md"> -->"#,
            r#"<pre><code>&lt;a href=&quot;code.md&quot;&gt;</code></pre>"#,
            r#"<p>a < b and href="text.md"</p>"#,
            r#"<img src="pic.png"><svg><a xlink:href="drawn.md"></a></svg>"#,
        );
        assert_eq!(values(html, "href"), ["one.md", "two.md?a=1&b=2"]);
        assert_eq!(values(html, "src"), ["pic.png"]);
    }

    #[test]
    fn an_address_is_on_the_web_elsewhere_or_a_file() {
        for web in ["http://example.com", "https://example.com/a#b", "HTTPS://x"] {
            assert_eq!(Link::classify(web), Link::Web(web));
        }
        for elsewhere in [
            "mailto:a@b.c",
            "tel:+1",
            "data:image/png;base64,AA",
            "ftp://x",
        ] {
            assert_eq!(Link::classify(elsewhere), Link::Elsewhere, "{elsewhere}");
        }
        for (address, path, fragment) in [
            ("b.md", "b.md", ""),
            ("../b.md#two", "../b.md", "two"),
            ("#top", "", "top"),
            ("", "", ""),
            ("notes/a:b.md", "notes/a:b.md", ""),
            ("b.md#at:10", "b.md", "at:10"),
            ("/root.md", "/root.md", ""),
        ] {
            assert_eq!(
                Link::classify(address),
                Link::File { path, fragment },
                "{address}"
            );
        }
    }
}
