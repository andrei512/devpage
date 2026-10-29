//! Sections ranked and shown: the best sections by score and structure, a
//! page's title as the query, and the snippet with the words it matched.

use std::collections::BTreeSet;

use tantivy::collector::TopDocs;
use tantivy::{DocAddress, Score, SegmentReader};

use super::*;

/// How many sections a search ranks before grouping them by page.
pub(super) const RANKED: usize = 200;

/// How many words a snippet shows when it can't end at a sentence's end, and
/// the most it shows when it can.
pub(super) const SNIPPET_WORDS: usize = 24;
pub(super) const SENTENCE_WORDS: usize = 40;

/// How much a section's place adds to its score: a little for a shallower
/// heading, a place earlier on its page, and a page earlier in the nav.
pub(super) fn structure(depth: u64, order: u64, nav: u64) -> Score {
    let depth = depth.max(1) as Score;
    1.0 + 0.06 / depth + 0.04 / (1.0 + order as Score) + 0.04 / (1.0 + nav as Score / 8.0)
}

/// How well a query is a page's title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TitleMatch {
    Whole,
    Start,
    None,
}

/// Where the sentence holding `at` starts.
pub(super) fn sentence_start(text: &str, at: usize) -> usize {
    let before = &text[..at];
    before
        .char_indices()
        .rev()
        .find(|&(i, c)| {
            matches!(c, '.' | '!' | '?')
                && before[i + 1..].starts_with(char::is_whitespace)
                && i + 1 < at
        })
        .map(|(i, _)| {
            let rest = &text[i + 1..];
            i + 1 + (rest.len() - rest.trim_start().len())
        })
        .unwrap_or(0)
}

/// Where the sentence holding `at` ends, its stop included.
pub(super) fn sentence_end(text: &str, at: usize) -> usize {
    let after = &text[at..];
    after
        .char_indices()
        .find(|&(i, c)| {
            matches!(c, '.' | '!' | '?')
                && after[i + 1..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map(|(i, c)| at + i + c.len_utf8())
        .unwrap_or(text.len())
}

pub(super) fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(['.', '!', '?'])
}

impl Search {
    /// The best [`RANKED`] sections for `parsed`: every required word must
    /// match — or, as a `fallback`, any — then structure breaks near-ties.
    pub(super) fn ranked(
        &self,
        parsed: &Parsed,
        now: &Snapshot,
        fallback: bool,
    ) -> Vec<(Score, DocAddress)> {
        let Some(query) = self.query(parsed, &now.pages, fallback) else {
            return Vec::new();
        };
        let collector = TopDocs::with_limit(RANKED).tweak_score(move |segment: &SegmentReader| {
            let column = |name: &str| {
                segment
                    .fast_fields()
                    .u64(name)
                    .expect("the schema has this fast field")
                    .first_or_default_col(0)
            };
            let (depth, order, nav) = (column("depth"), column("order"), column("nav"));
            move |doc, score: Score| {
                score * structure(depth.get_val(doc), order.get_val(doc), nav.get_val(doc))
            }
        });
        now.searcher
            .search_with_statistics_provider(&query, &collector, now)
            .unwrap_or_default()
    }

    /// Whether the query is `title`: the same words but stopwords, the last
    /// perhaps still being typed.
    pub(super) fn title_match(&self, parsed: &Parsed, title: &str) -> TitleMatch {
        let query: Vec<&Word> = parsed
            .words
            .iter()
            .chain(parsed.phrases.iter().flatten())
            .filter(|word| !word.stop)
            .collect();
        let title: Vec<Word> = self
            .words(title, false)
            .into_iter()
            .filter(|word| !word.stop)
            .collect();
        if query.is_empty() || query.len() != title.len() {
            return TitleMatch::None;
        }
        let n = query.len() - 1;
        if query[..n]
            .iter()
            .zip(&title[..n])
            .any(|(q, t)| q.stem != t.stem)
        {
            return TitleMatch::None;
        }
        if query[n].stem == title[n].stem {
            TitleMatch::Whole
        } else if query[n].last && title[n].typed.starts_with(&query[n].typed) {
            TitleMatch::Start
        } else {
            TitleMatch::None
        }
    }

    /// Each token of `text` with the query word it matches, if any.
    pub(super) fn tokens(
        &self,
        text: &str,
        wanted: &[&Word],
    ) -> Vec<(usize, usize, Option<usize>)> {
        let mut stemmed = self.stemmed.clone();
        let mut tokens = Vec::new();
        stemmed.token_stream(text).process(&mut |token| {
            let typed = text[token.offset_from..token.offset_to].to_lowercase();
            let matched = wanted
                .iter()
                .position(|word| word.matches(&typed, &token.text));
            tokens.push((token.offset_from, token.offset_to, matched));
        });
        tokens
    }

    /// How many of the `wanted` words `text` holds.
    pub(super) fn covered(&self, text: &str, wanted: &[&Word]) -> usize {
        let found: BTreeSet<usize> = self
            .tokens(text, wanted)
            .into_iter()
            .filter_map(|(_, _, matched)| matched)
            .collect();
        found.len()
    }

    /// The words of `texts` that match, as each text spells them, once each.
    pub(super) fn spelled(&self, texts: &[&str], wanted: &[&Word]) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        for text in texts {
            for (from, to, matched) in self.tokens(text, wanted) {
                let word = &text[from..to];
                if matched.is_some() && !words.iter().any(|seen| seen == word) {
                    words.push(word.to_string());
                }
            }
        }
        words
    }

    /// The part of `prose` where the query's words cluster densest, cut at
    /// sentence bounds, escaped, each matched word in `<mark>`; the first
    /// sentence when no word matches it.
    pub(super) fn snippet(&self, prose: &str, wanted: &[&Word]) -> String {
        let tokens = self.tokens(prose, wanted);
        if tokens.is_empty() {
            return String::new();
        }
        let width = SNIPPET_WORDS.min(tokens.len());
        // The window holding the most different words, then the most
        // matches; of equals, the first.
        let start = (0..=tokens.len() - width)
            .max_by_key(|&start| {
                let window = &tokens[start..start + width];
                let distinct: BTreeSet<usize> = window
                    .iter()
                    .filter_map(|(_, _, matched)| *matched)
                    .collect();
                let count = window.iter().filter(|(_, _, m)| m.is_some()).count();
                (distinct.len(), count, std::cmp::Reverse(start))
            })
            .unwrap_or(0);
        let window = &tokens[start..start + width];
        let first = window.iter().position(|(_, _, m)| m.is_some());
        let last = window.iter().rposition(|(_, _, m)| m.is_some());
        let (from, to) = match (first, last) {
            (Some(first), Some(last)) => (
                sentence_start(prose, window[first].0),
                sentence_end(prose, window[last].1),
            ),
            _ => (0, sentence_end(prose, 0)),
        };
        let inside: Vec<_> = tokens
            .iter()
            .filter(|(begin, end, _)| *begin >= from && *end <= to)
            .collect();
        // A sentence too long to show whole: the window, cut at words.
        let (from, to) = if inside.len() > SENTENCE_WORDS {
            let begin = match first {
                Some(first) => (start + first).saturating_sub(4),
                None => 0,
            };
            let end = (begin + SNIPPET_WORDS).min(tokens.len());
            let from = if begin == 0 { 0 } else { tokens[begin].0 };
            let to = if end == tokens.len() {
                prose.len()
            } else {
                tokens[end - 1].1
            };
            (from, to)
        } else {
            (from, to)
        };
        let mut html = String::new();
        if from > 0 && !ends_sentence(&prose[..from]) {
            html.push('…');
        }
        let mut at = from;
        for &(begin, end, matched) in tokens.iter().filter(|(b, e, _)| *b >= from && *e <= to) {
            html.push_str(&crate::html::escape(&prose[at..begin]));
            let word = crate::html::escape(&prose[begin..end]);
            if matched.is_some() {
                html.push_str(&format!("<mark>{word}</mark>"));
            } else {
                html.push_str(&word);
            }
            at = end;
        }
        html.push_str(&crate::html::escape(&prose[at..to]));
        if to < prose.len() && !ends_sentence(&prose[..to]) {
            html.push('…');
        }
        html.trim().to_string()
    }
}
