//! A query read into words — stemmed, near, being typed, code identifiers
//! split into their parts — and the tantivy query that ranks sections by
//! them.

use std::collections::BTreeSet;

use tantivy::Term;
use tantivy::query::{
    BooleanQuery, BoostQuery, ConstScoreQuery, DisjunctionMaxQuery, FuzzyTermQuery, Occur,
    PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption};

use super::*;

/// What one word counts in each kind of text, against prose. A word scores
/// in its best field only, plus [`TIE_BREAKER`] of what it scores elsewhere.
pub(super) const TITLE_BOOST: f32 = 4.0;
pub(super) const HEADING_BOOST: f32 = 2.0;
pub(super) const PROSE_BOOST: f32 = 1.0;
pub(super) const CODE_BOOST: f32 = 0.5;
pub(super) const NOTES_BOOST: f32 = 0.5;
pub(super) const META_BOOST: f32 = 3.0;
pub(super) const TIE_BREAKER: f32 = 0.1;

/// What a near match — a typo, or the word being typed — counts, against an
/// exact one.
pub(super) const NEAR: f32 = 0.5;

/// What the query's words in a row add — in a heading and in prose, exactly
/// and within [`SLOP`] words of each other — and a whole code identifier.
pub(super) const PHRASE_HEADING: f32 = 2.0;
pub(super) const PHRASE_PROSE: f32 = 1.5;
pub(super) const NEAR_PHRASE: f32 = 0.5;
pub(super) const SLOP: u32 = 3;
pub(super) const IDENTIFIER: f32 = 3.0;

/// What a query that is a whole heading, or a whole title, adds — and one
/// that is the start of either, its last word being typed.
pub(super) const WHOLE_HEADING: f32 = 3.0;
pub(super) const WHOLE_TITLE: f32 = 8.0;
pub(super) const HEADING_START: f32 = 1.5;

/// Words that never have to match: a query of only these still finds them.
pub(super) const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "in", "is", "it", "its",
    "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where", "which", "with",
];

/// A query, read: its words, phrases, excluded words, and code identifiers.
#[derive(Default)]
pub(super) struct Parsed {
    pub(super) words: Vec<Word>,
    pub(super) phrases: Vec<Vec<Word>>,
    pub(super) excluded: Vec<Word>,
    pub(super) identifiers: Vec<String>,
}

impl Parsed {
    /// How many words and phrases must match.
    pub(super) fn required(&self) -> usize {
        self.words.iter().filter(|word| !word.stop).count() + self.phrases.len()
    }

    /// The words a section's text is marked with: every word and phrase
    /// word but stopwords.
    pub(super) fn matched(&self) -> Vec<&Word> {
        self.words
            .iter()
            .chain(self.phrases.iter().flatten())
            .filter(|word| !word.stop)
            .collect()
    }

    /// The query as a whole heading would hold it, stemmed and as typed.
    pub(super) fn whole(&self) -> (String, String) {
        let words: Vec<Word> = self
            .words
            .iter()
            .chain(self.phrases.iter().flatten())
            .cloned()
            .collect();
        let kept = kept(&words);
        (
            joined(&kept, |word| &word.stem),
            joined(&kept, |word| &word.typed),
        )
    }
}

/// One word of a query: as typed, lowercased, and stemmed as the index is.
#[derive(Debug, Clone)]
pub(super) struct Word {
    pub(super) typed: String,
    pub(super) stem: String,
    /// A stopword need not match.
    pub(super) stop: bool,
    /// The last word, still being typed: the start of a word matches it.
    pub(super) last: bool,
    /// How many letters a near match may differ by; 0 when it must be exact.
    pub(super) near: u8,
}

impl Word {
    /// Whether a token of the text, as typed and stemmed, is this word.
    pub(super) fn matches(&self, typed: &str, stem: &str) -> bool {
        self.stem == stem
            || (self.near > 0 && distance(&self.typed, typed) <= usize::from(self.near))
            || (self.last && typed.starts_with(&self.typed))
    }
}

/// The words of a title or heading as whole-title matching compares them:
/// without stopwords, unless they are all it has.
pub(super) fn kept(words: &[Word]) -> Vec<&Word> {
    let content: Vec<&Word> = words.iter().filter(|word| !word.stop).collect();
    if content.is_empty() {
        words.iter().collect()
    } else {
        content
    }
}

pub(super) fn joined(words: &[&Word], part: impl Fn(&Word) -> &String) -> String {
    words
        .iter()
        .map(|word| part(word).as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn term_query(term: Term) -> Box<dyn Query> {
    Box::new(TermQuery::new(term, IndexRecordOption::WithFreqs))
}

pub(super) fn boosted(query: Box<dyn Query>, boost: f32) -> Box<dyn Query> {
    Box::new(BoostQuery::new(query, boost))
}

/// `stems` in a row in `field`, within `slop` words; one stem is a term.
pub(super) fn phrase_query(field: Field, stems: &[&str], slop: u32) -> Box<dyn Query> {
    let terms: Vec<(usize, Term)> = stems
        .iter()
        .enumerate()
        .map(|(i, stem)| (i, Term::from_field_text(field, stem)))
        .collect();
    if terms.len() == 1 {
        let (_, term) = terms.into_iter().next().expect("one term");
        return term_query(term);
    }
    Box::new(PhraseQuery::new_with_offset_and_slop(terms, slop))
}

/// Whether a chunk of a query is a code identifier: `snake_case`,
/// `camelCase` or a `::` path.
pub(super) fn is_identifier(chunk: &str) -> bool {
    let chars: Vec<char> = chunk.chars().collect();
    chunk.contains("::")
        || chunk.trim_matches('_').contains('_')
        || chars
            .windows(2)
            .any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase())
}

pub(super) fn is_code_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

/// Code as the `code` field indexes it: each identifier whole, lowercased,
/// and its parts — split at `_`, `::` and a change to upper case.
pub(super) fn expand_code(code: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for chunk in code.split(|c: char| !is_code_char(c)) {
        let chunk = chunk.trim_matches(':');
        if chunk.is_empty() {
            continue;
        }
        let parts = identifier_parts(chunk);
        if parts.len() > 1 {
            out.push(chunk.to_lowercase());
        }
        out.extend(parts);
    }
    out.join(" ")
}

pub(super) fn identifier_parts(identifier: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for piece in identifier.split(['_', ':']).filter(|p| !p.is_empty()) {
        let chars: Vec<char> = piece.chars().collect();
        let mut part = String::new();
        for (i, &c) in chars.iter().enumerate() {
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let split = i > 0
                && c.is_uppercase()
                && (chars[i - 1].is_lowercase()
                    || chars[i - 1].is_ascii_digit()
                    || (chars[i - 1].is_uppercase() && next_lower));
            if split && !part.is_empty() {
                parts.push(std::mem::take(&mut part).to_lowercase());
            }
            part.push(c);
        }
        if !part.is_empty() {
            parts.push(part.to_lowercase());
        }
    }
    parts
}

/// The edit distance between two words, a swap of neighbours counting as
/// one edit, as tantivy's near matches count it.
pub(super) fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    rows[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

impl Search {
    /// Splits a query into its words, its `"quoted"` phrases and its
    /// `-excluded` words, each word lowercased as typed and stemmed.
    pub(super) fn parse(&self, query: &str) -> Parsed {
        let mut parsed = Parsed::default();
        let mut rest = query;
        let mut last_plain = false;
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            if let Some(quoted) = rest.strip_prefix('"') {
                let end = quoted.find('"').unwrap_or(quoted.len());
                let words = self.words(&quoted[..end], false);
                if !words.is_empty() {
                    parsed.phrases.push(words);
                }
                rest = quoted.get(end + 1..).unwrap_or("");
                last_plain = false;
                continue;
            }
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '"')
                .unwrap_or(rest.len());
            let chunk = &rest[..end];
            rest = &rest[end..];
            match chunk.strip_prefix('-').filter(|word| !word.is_empty()) {
                Some(excluded) => {
                    parsed.excluded.extend(self.words(excluded, false));
                    last_plain = false;
                }
                None => {
                    let identifier = is_identifier(chunk);
                    if identifier {
                        parsed.identifiers.push(
                            chunk
                                .trim_matches(|c: char| !is_code_char(c))
                                .to_lowercase(),
                        );
                    }
                    let words = self.words(chunk, identifier);
                    last_plain = !words.is_empty() || last_plain;
                    parsed.words.extend(words);
                }
            }
        }
        if last_plain
            && rest.is_empty()
            && !query.ends_with(char::is_whitespace)
            && let Some(word) = parsed.words.last_mut()
        {
            word.last = true;
        }
        // A query of only stopwords needs them.
        if parsed.words.iter().all(|word| word.stop) {
            for word in &mut parsed.words {
                word.stop = false;
            }
        }
        parsed
    }

    /// The words of `text`, lowercased as typed and stemmed. A word of a
    /// code identifier is never a stopword.
    pub(super) fn words(&self, text: &str, identifier: bool) -> Vec<Word> {
        let mut plain = self.plain.clone();
        let mut words = Vec::new();
        plain.token_stream(text).process(&mut |token| {
            words.push(Word {
                stop: !identifier && STOPWORDS.contains(&token.text.as_str()),
                stem: self.stem(&token.text),
                typed: token.text.clone(),
                last: false,
                near: 0,
            });
        });
        words
    }

    /// The stems of the words of `text`.
    pub(super) fn stems(&self, text: &str) -> BTreeSet<String> {
        self.words(text, false)
            .into_iter()
            .map(|word| word.stem)
            .collect()
    }

    pub(super) fn stem(&self, word: &str) -> String {
        let mut stemmed = self.stemmed.clone();
        let mut stem = word.to_string();
        stemmed
            .token_stream(word)
            .process(&mut |token| stem = token.text.clone());
        stem
    }

    /// Lets a word the index doesn't hold match near it: one letter off from
    /// four letters, two from eight.
    pub(super) fn mark_near(&self, parsed: &mut Parsed, searcher: &tantivy::Searcher) {
        let fields = self.fields;
        for word in parsed.words.iter_mut().filter(|word| !word.stop) {
            let allowed = match word.typed.chars().count() {
                0..4 => 0,
                4..8 => 1,
                _ => 2,
            };
            if allowed == 0 {
                continue;
            }
            let stemmed = [
                fields.title,
                fields.heading,
                fields.prose,
                fields.notes,
                fields.meta,
            ];
            let known = stemmed
                .iter()
                .map(|&field| Term::from_field_text(field, &word.stem))
                .chain([Term::from_field_text(fields.code, &word.typed)])
                .any(|term| searcher.doc_freq(&term).is_ok_and(|n| n > 0));
            if !known {
                word.near = allowed;
            }
        }
    }

    /// The query for `parsed`; `None` when it has nothing to find.
    pub(super) fn query(
        &self,
        parsed: &Parsed,
        pages: &Pages,
        fallback: bool,
    ) -> Option<BooleanQuery> {
        let fields = self.fields;
        let need = if fallback { Occur::Should } else { Occur::Must };
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for word in &parsed.words {
            let occur = if word.stop { Occur::Should } else { need };
            clauses.push((occur, self.word_query(word, pages)));
        }
        for phrase in &parsed.phrases {
            let stems: Vec<&str> = phrase.iter().map(|word| word.stem.as_str()).collect();
            let either: Vec<Box<dyn Query>> = [
                (fields.title, TITLE_BOOST),
                (fields.heading, HEADING_BOOST),
                (fields.prose, PROSE_BOOST),
                (fields.notes, NOTES_BOOST),
            ]
            .into_iter()
            .map(|(field, boost)| boosted(phrase_query(field, &stems, 0), boost))
            .collect();
            clauses.push((
                need,
                Box::new(DisjunctionMaxQuery::with_tie_breaker(either, TIE_BREAKER)),
            ));
        }
        if clauses.is_empty() {
            return None;
        }
        // The words in a row, in a heading or in prose.
        if parsed.words.len() > 1 {
            let stems: Vec<&str> = parsed.words.iter().map(|w| w.stem.as_str()).collect();
            for (field, boost) in [
                (fields.heading, PHRASE_HEADING),
                (fields.prose, PHRASE_PROSE),
            ] {
                clauses.push((
                    Occur::Should,
                    boosted(phrase_query(field, &stems, 0), boost),
                ));
                clauses.push((
                    Occur::Should,
                    boosted(phrase_query(field, &stems, SLOP), boost * NEAR_PHRASE),
                ));
            }
        }
        for identifier in &parsed.identifiers {
            let term = Term::from_field_text(fields.code, identifier);
            clauses.push((Occur::Should, boosted(term_query(term), IDENTIFIER)));
        }
        // The query as a whole heading or title, or the start of a
        // heading: the best of them.
        let (stems, typed) = parsed.whole();
        if !stems.is_empty() {
            let whole = |field: Field, text: &str, score: f32| -> Box<dyn Query> {
                let term = Term::from_field_text(field, text);
                Box::new(ConstScoreQuery::new(term_query(term), score))
            };
            let mut either = vec![
                whole(fields.heading_exact, &stems, WHOLE_HEADING),
                whole(fields.title_exact, &stems, WHOLE_TITLE),
            ];
            if parsed.words.last().is_some_and(|word| word.last) {
                let term = Term::from_field_text(fields.heading_exact, &typed);
                either.push(Box::new(ConstScoreQuery::new(
                    Box::new(FuzzyTermQuery::new_prefix(term, 0, true)),
                    HEADING_START,
                )));
            }
            clauses.push((Occur::Should, Box::new(DisjunctionMaxQuery::new(either))));
        }
        for word in &parsed.excluded {
            clauses.push((Occur::MustNot, self.any_field(word)));
        }
        Some(BooleanQuery::new(clauses))
    }

    /// One word in its best field: exactly once stemmed, near it when the
    /// index doesn't hold it, as the start of a word when it is being typed.
    /// A page's title and front matter are on each of its sections, so a
    /// word in them scores by how rare it is among pages, not sections.
    pub(super) fn word_query(&self, word: &Word, pages: &Pages) -> Box<dyn Query> {
        let fields = self.fields;
        let rarity = |of: fn(&Page) -> &BTreeSet<String>| {
            let n = pages
                .values()
                .filter(|page| of(page).contains(&word.stem))
                .count() as f32;
            let total = pages.len() as f32;
            (1.0 + (total - n + 0.5) / (n + 0.5)).ln()
        };
        let mut either = Vec::new();
        for (field, words, boost, per_page) in [
            (
                fields.title,
                Some(fields.title_words),
                TITLE_BOOST,
                Some(rarity(|p| &p.title_stems)),
            ),
            (
                fields.heading,
                Some(fields.heading_words),
                HEADING_BOOST,
                None,
            ),
            (fields.prose, Some(fields.prose_words), PROSE_BOOST, None),
            (fields.notes, None, NOTES_BOOST, None),
            (
                fields.meta,
                None,
                META_BOOST,
                Some(rarity(|p| &p.meta_stems)),
            ),
        ] {
            let term = term_query(Term::from_field_text(field, &word.stem));
            either.push(match per_page {
                Some(rarity) => Box::new(ConstScoreQuery::new(term, boost * rarity)),
                None => boosted(term, boost),
            });
            let Some(words) = words else { continue };
            let typed = Term::from_field_text(words, &word.typed);
            if word.near > 0 {
                either.push(boosted(
                    Box::new(FuzzyTermQuery::new(typed.clone(), word.near, true)),
                    boost * NEAR,
                ));
            }
            if word.last {
                either.push(boosted(
                    Box::new(FuzzyTermQuery::new_prefix(typed, 0, true)),
                    boost * NEAR,
                ));
            }
        }
        either.push(boosted(
            term_query(Term::from_field_text(fields.code, &word.typed)),
            CODE_BOOST,
        ));
        Box::new(DisjunctionMaxQuery::with_tie_breaker(either, TIE_BREAKER))
    }

    /// A word in any field, for leaving out what holds it.
    pub(super) fn any_field(&self, word: &Word) -> Box<dyn Query> {
        let fields = self.fields;
        let mut terms: Vec<(Occur, Box<dyn Query>)> = [
            fields.title,
            fields.heading,
            fields.prose,
            fields.notes,
            fields.meta,
        ]
        .into_iter()
        .map(|field| {
            (
                Occur::Should,
                term_query(Term::from_field_text(field, &word.stem)),
            )
        })
        .collect();
        terms.push((
            Occur::Should,
            term_query(Term::from_field_text(fields.code, &word.typed)),
        ));
        Box::new(BooleanQuery::new(terms))
    }
}
