//! Carrying a thread onto a new version of its page: the page's text as the
//! page script reads it, and where a thread's quote is in it now. Pure — the
//! server hands it the new version's HTML and stores what it answers.

use super::Anchor;

/// How much text either side of a quote an anchor keeps: the page script's
/// `CONTEXT`.
const CONTEXT: usize = 32;

/// The text of rendered HTML as the page script's text model reads it: the
/// text between the tags, entities decoded, every run of white space one
/// space.
pub fn text(html: &str) -> String {
    let mut raw = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        raw.push_str(&rest[..start]);
        let tag = &rest[start..];
        // What a script or a style holds is never on the page.
        let skipped = ["script", "style"].iter().find_map(|name| {
            let open = tag.get(1..1 + name.len())?;
            let ends = tag.as_bytes().get(1 + name.len())?;
            (open.eq_ignore_ascii_case(name) && (*ends == b'>' || ends.is_ascii_whitespace()))
                .then(|| format!("</{name}>"))
        });
        rest = match skipped {
            Some(close) => tag
                .to_ascii_lowercase()
                .find(&close)
                .map_or("", |end| &tag[end + close.len()..]),
            None => tag.find('>').map_or("", |end| &tag[end + 1..]),
        };
    }
    raw.push_str(rest);
    squeeze(&decode(&raw))
}

/// Where `anchor`'s quote is in `text` now: the anchor itself when its quote
/// is still there with its context, a new one with the context it has now
/// when only the quote is, and `None` when the quote is gone — the thread is
/// detached.
pub fn reanchor(anchor: &Anchor, text: &str) -> Option<Anchor> {
    let quote = squeeze(&anchor.quote);
    if quote.trim().is_empty() {
        return None;
    }
    let whole = format!(
        "{}{quote}{}",
        squeeze(&anchor.prefix),
        squeeze(&anchor.suffix)
    );
    if text.contains(&whole) {
        return Some(anchor.clone());
    }
    let at = text.find(&quote)?;
    let end = at + quote.len();
    let before: String = {
        let chars: Vec<char> = text[..at].chars().collect();
        chars[chars.len().saturating_sub(CONTEXT)..]
            .iter()
            .collect()
    };
    let after: String = text[end..].chars().take(CONTEXT).collect();
    Some(Anchor {
        quote: anchor.quote.clone(),
        prefix: before,
        suffix: after,
    })
}

/// Every run of white space as one space, as the page script's model has it.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// HTML's character references, as the browser decodes them into text.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let decoded = tail.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &tail[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(prefix: &str, quote: &str, suffix: &str) -> Anchor {
        Anchor {
            quote: quote.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
        }
    }

    #[test]
    fn the_text_is_what_the_page_shows() {
        let html = "<h1>Rollout</h1>\n<p>The client <strong>re</strong>tries every\n  write &amp; read.</p><script>let x = '<p>no</p>';</script><style>p{}</style><p>&#x41;&#66;</p>";
        assert_eq!(
            text(html),
            "Rollout The client retries every write & read.AB"
        );
    }

    #[test]
    fn a_quote_with_its_context_keeps_its_anchor() {
        let kept = anchor("The client ", "retries every write", " until");
        let text = "Intro. The client retries every write until it is acknowledged.";
        assert_eq!(reanchor(&kept, text), Some(kept.clone()));
    }

    #[test]
    fn a_quote_whose_context_changed_takes_its_new_context() {
        let moved = anchor("The client ", "retries every write", " until");
        let text = "Rewritten: now each client retries every write, capped at five.";
        let found = reanchor(&moved, text).unwrap();
        assert_eq!(found.quote, "retries every write");
        assert_eq!(found.prefix, "Rewritten: now each client ");
        assert_eq!(found.suffix, ", capped at five.");
        // What the page script then finds: the quote with its new context.
        assert!(text.contains(&format!("{}{}{}", found.prefix, found.quote, found.suffix)));
    }

    #[test]
    fn a_quote_that_is_gone_is_detached() {
        let gone = anchor("", "retries are capped at five", "");
        assert_eq!(reanchor(&gone, "Retries are unlimited now."), None);
        assert_eq!(reanchor(&anchor("", "  ", ""), "  "), None);
    }

    #[test]
    fn context_is_counted_in_characters() {
        let text = format!("{}é quote ü{}", "ä".repeat(40), "ö".repeat(40));
        let found = reanchor(&anchor("x", "quote", "y"), &text).unwrap();
        assert_eq!(found.prefix.chars().count(), CONTEXT);
        assert_eq!(found.suffix.chars().count(), CONTEXT);
    }
}
