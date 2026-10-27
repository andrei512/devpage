//! Full-text search over what the served document publishes: the pages its
//! nav reaches, and the chapters and slides of every document those reach,
//! split at their headings into sections, in a tantivy index held in memory.
//! Built at start beside the document index and kept current by the watcher,
//! so a search never reads a file.
//!
//! tantivy, because it is a real full-text engine — BM25 ranking, stemming,
//! phrases, near and prefix matches — as a library in pure Rust, with an
//! index in memory: nothing to run beside devpage and nothing written to disk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, parse_document};
use ignore::WalkBuilder;
use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::query::{
    BooleanQuery, BoostQuery, ConstScoreQuery, DisjunctionMaxQuery, FuzzyTermQuery, Occur,
    PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, WhitespaceTokenizer};
use tantivy::{
    DocAddress, IndexReader, IndexWriter, ReloadPolicy, Score, SegmentReader, TantivyDocument, Term,
};

use crate::document::{Document, Kind};
use crate::index::Index;

/// How many sections a search ranks before grouping them by page, how many
/// pages it answers, and how many of each page's sections it shows.
const RANKED: usize = 200;
const MAX_PAGES: usize = 10;
const SECTIONS_PER_PAGE: usize = 3;

/// What one word counts in each kind of text, against prose. A word scores
/// in its best field only, plus [`TIE_BREAKER`] of what it scores elsewhere.
const TITLE_BOOST: f32 = 4.0;
const HEADING_BOOST: f32 = 2.0;
const PROSE_BOOST: f32 = 1.0;
const CODE_BOOST: f32 = 0.5;
const NOTES_BOOST: f32 = 0.5;
const META_BOOST: f32 = 3.0;
const TIE_BREAKER: f32 = 0.1;

/// What a near match — a typo, or the word being typed — counts, against an
/// exact one.
const NEAR: f32 = 0.5;

/// What the query's words in a row add — in a heading and in prose, exactly
/// and within [`SLOP`] words of each other — and a whole code identifier.
const PHRASE_HEADING: f32 = 2.0;
const PHRASE_PROSE: f32 = 1.5;
const NEAR_PHRASE: f32 = 0.5;
const SLOP: u32 = 3;
const IDENTIFIER: f32 = 3.0;

/// What a query that is a whole heading, or a whole title, adds — and one
/// that is the start of either, its last word being typed.
const WHOLE_HEADING: f32 = 3.0;
const WHOLE_TITLE: f32 = 8.0;
const HEADING_START: f32 = 1.5;

/// The tokenizers: the ranked fields' — tantivy's default, lowercased, with
/// English stemming — their unstemmed twins', for near and prefix matches,
/// code's, and the whole-title fields', which keep a value as one term.
const STEMMED: &str = "en_stem";
const PLAIN: &str = "default";
const CODE: &str = "code";
const RAW: &str = "raw";

/// How many words a snippet shows when it can't end at a sentence's end, and
/// the most it shows when it can.
const SNIPPET_WORDS: usize = 24;
const SENTENCE_WORDS: usize = 40;

/// Words that never have to match: a query of only these still finds them.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "in", "is", "it", "its",
    "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where", "which", "with",
];

/// A search's answer: the pages found, best first, each with its best
/// sections.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Answer {
    pub pages: Vec<PageResult>,
}

/// One page found: where it is, what it is, and its sections that matched.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PageResult {
    pub title: String,
    /// Where the page opens: its top.
    pub url: String,
    /// The page's place in its document's nav: the labels from the top down to it.
    pub trail: Vec<String>,
    pub document: Owner,
    pub kind: PageKind,
    /// Whether the query is the page's title: it comes first, and opens at `url`.
    pub title_match: bool,
    /// The page's first paragraph.
    pub lede: String,
    /// Its best sections, at most [`SECTIONS_PER_PAGE`].
    pub sections: Vec<SectionResult>,
    /// How many more of its sections matched.
    pub more: usize,
}

/// One section found: its heading, where it is, a snippet of its prose with
/// the matched words in `<mark>`, and those words as its text spells them.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SectionResult {
    pub heading: String,
    /// The page's title and the headings down to this one.
    pub path: Vec<String>,
    /// The page's address with the heading's anchor.
    pub url: String,
    /// HTML: a part of the section's prose, escaped, its matched words in `<mark>`.
    pub snippet: String,
    pub words: Vec<String>,
}

/// What a page found is: a site's page, a doc's chapter, or a deck's slide.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    Page,
    Chapter,
    Slide,
}

/// The document a page belongs to: its title and its document file's address.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Owner {
    pub title: String,
    pub path: String,
}

pub struct Search {
    fields: Fields,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    stemmed: TextAnalyzer,
    plain: TextAnalyzer,
    /// The served document's file, where what is published starts.
    entry: PathBuf,
    /// Every page indexed, by its address.
    pages: RwLock<Pages>,
}

#[derive(Clone, Copy)]
struct Fields {
    page: Field,
    url: Field,
    path: Field,
    title: Field,
    heading: Field,
    prose: Field,
    code: Field,
    code_text: Field,
    notes: Field,
    meta: Field,
    title_words: Field,
    heading_words: Field,
    prose_words: Field,
    title_exact: Field,
    heading_exact: Field,
}

/// The page record: what a page result shows besides its sections.
#[derive(Debug, Clone)]
struct Page {
    title: String,
    url: String,
    trail: Vec<String>,
    document: Owner,
    kind: PageKind,
    lede: String,
    /// Its place in the nav, counted from the top in reading order.
    nav: u64,
    /// The stems of its title, and of its front matter's description and
    /// keywords.
    title_stems: BTreeSet<String>,
    meta_stems: BTreeSet<String>,
}

/// Every page indexed, by its address.
type Pages = BTreeMap<String, Page>;

/// A page to index, as the scope found it.
struct Unit {
    file: PathBuf,
    kind: PageKind,
    trail: Vec<String>,
    document: Owner,
    /// The address of what holds it: the page itself, or the doc or deck.
    address: String,
    /// A chapter's name, when its doc has more than one; a slide's number.
    name: Option<String>,
}

/// A section found, as stored: what its result is made of.
struct Found {
    heading: String,
    path: Vec<String>,
    url: String,
    prose: String,
    code: String,
}

/// One section of a Markdown file: its heading's text, id and level (none
/// and 0 for what comes before the first heading), and its text by kind.
#[derive(Debug, PartialEq, Default)]
struct Section {
    heading: String,
    id: Option<String>,
    level: u8,
    prose: String,
    code: String,
}

/// A Markdown file read for search: its sections and its first paragraph.
struct Read {
    sections: Vec<Section>,
    lede: String,
}

/// One section, ready to be a tantivy document.
struct Entry<'a> {
    page: &'a Page,
    heading: &'a str,
    url: String,
    path: Vec<String>,
    depth: u8,
    order: usize,
    prose: &'a str,
    code: &'a str,
    notes: &'a str,
    meta: &'a str,
}

impl Search {
    /// Indexes what the document `entry` publishes, among those `index` holds.
    pub fn build(index: &Index, entry: &Path) -> Self {
        let mut schema = Schema::builder();
        let text = |tokenizer: &str, record: IndexRecordOption, stored: bool| {
            let options = TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(tokenizer)
                    .set_index_option(record),
            );
            if stored {
                options.set_stored()
            } else {
                options
            }
        };
        let ranked = |stored| text(STEMMED, IndexRecordOption::WithFreqsAndPositions, stored);
        let plain = text(PLAIN, IndexRecordOption::Basic, false);
        let raw = text(RAW, IndexRecordOption::Basic, false);
        let fields = Fields {
            page: schema.add_text_field("page", STORED),
            url: schema.add_text_field("url", STORED),
            path: schema.add_text_field("path", STORED),
            title: schema.add_text_field("title", ranked(true)),
            heading: schema.add_text_field("heading", ranked(true)),
            prose: schema.add_text_field("prose", ranked(true)),
            code: schema.add_text_field("code", text(CODE, IndexRecordOption::WithFreqs, false)),
            code_text: schema.add_text_field("code_text", STORED),
            notes: schema.add_text_field("notes", ranked(true)),
            meta: schema.add_text_field("meta", ranked(true)),
            title_words: schema.add_text_field("title_words", plain.clone()),
            heading_words: schema.add_text_field("heading_words", plain.clone()),
            prose_words: schema.add_text_field("prose_words", plain),
            title_exact: schema.add_text_field("title_exact", raw.clone()),
            heading_exact: schema.add_text_field("heading_exact", raw),
        };
        for name in ["depth", "order", "nav"] {
            schema.add_u64_field(name, FAST);
        }
        let tantivy = tantivy::Index::create_in_ram(schema.build());
        tantivy.tokenizers().register(
            CODE,
            TextAnalyzer::builder(WhitespaceTokenizer::default())
                .filter(LowerCaser)
                .build(),
        );
        let analyzer = |name: &str| {
            tantivy
                .tokenizers()
                .get(name)
                .expect("tantivy registers its default and en_stem tokenizers")
        };
        let (stemmed, plain) = (analyzer(STEMMED), analyzer(PLAIN));
        // One thread at the smallest budget tantivy allows, 15 MB: a docs
        // folder is small, and the index lives in memory.
        let writer = tantivy
            .writer_with_num_threads(1, 15_000_000)
            .expect("an index in memory has no lock to contend for");
        let reader = tantivy
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .expect("an index in memory can always be read");
        let search = Self {
            fields,
            reader,
            writer: Mutex::new(writer),
            stemmed,
            plain,
            entry: entry.to_path_buf(),
            pages: RwLock::new(BTreeMap::new()),
        };
        search.rebuild(index);
        search
    }

    /// Brings the index up to date after `paths` changed, appeared or
    /// vanished — after `index` has heard of them. Any of them can change
    /// what is published — a page's Topics, a document file, a
    /// `.gitignore`, a folder — so a burst that holds a Markdown file, a
    /// YAML file, a `.gitignore`, a folder or a path that is gone reads
    /// everything published again.
    pub fn changed(&self, index: &Index, paths: &[PathBuf]) {
        let matters = |path: &PathBuf| {
            path.is_dir()
                || !path.exists()
                || path.file_name().is_some_and(|name| name == ".gitignore")
                || path
                    .extension()
                    .is_some_and(|ext| ext == "md" || ext == "yml" || ext == "yaml")
        };
        if paths.iter().any(matters) {
            self.rebuild(index);
        }
    }

    /// The pages matching `query`, best first, at most [`MAX_PAGES`], each
    /// with its best sections.
    pub fn search(&self, query: &str) -> Answer {
        let searcher = self.reader.searcher();
        let pages = self.pages.read().unwrap_or_else(|p| p.into_inner());
        let mut parsed = self.parse(query);
        self.mark_near(&mut parsed, &searcher);
        let mut found = self.ranked(&parsed, &searcher, &pages, false);
        let fallback = found.is_empty() && parsed.required() > 1;
        if fallback {
            found = self.ranked(&parsed, &searcher, &pages, true);
        }
        let wanted = parsed.matched();
        // Each section found, with its page, scored.
        let mut sections: Vec<(Score, &Page, Found)> = Vec::new();
        for (score, address) in found {
            let Ok(doc) = searcher.doc::<TantivyDocument>(address) else {
                continue;
            };
            let text = |field: Field| {
                doc.get_first(field)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let Some(page) = pages.get(&text(self.fields.page)) else {
                continue;
            };
            let (heading, prose, code) = (
                text(self.fields.heading),
                text(self.fields.prose),
                text(self.fields.code_text),
            );
            let mut score = score;
            if fallback {
                let all = [
                    page.title.as_str(),
                    &heading,
                    &prose,
                    &code,
                    &text(self.fields.notes),
                    &text(self.fields.meta),
                ]
                .join(" ");
                let covered = self.covered(&all, &wanted);
                let coverage = covered as f32 / wanted.len().max(1) as f32;
                score *= coverage * coverage;
            }
            sections.push((
                score,
                page,
                Found {
                    heading,
                    path: doc
                        .get_all(self.fields.path)
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect(),
                    url: text(self.fields.url),
                    prose,
                    code,
                },
            ));
        }
        sections.sort_by(|a, b| b.0.total_cmp(&a.0));
        // Grouped by page: a page scores as its best section.
        let mut grouped: Vec<(Score, &Page, Vec<Found>)> = Vec::new();
        for (score, page, section) in sections {
            match grouped.iter_mut().find(|(_, p, _)| p.url == page.url) {
                Some((_, _, list)) => list.push(section),
                None => grouped.push((score, page, vec![section])),
            }
        }
        // A page whose title the query is comes first: a whole title before
        // one being typed, then in the nav's order.
        let titled = |page: &Page| self.title_match(&parsed, &page.title);
        grouped.sort_by(|a, b| {
            let (ta, tb) = (titled(a.1), titled(b.1));
            ta.cmp(&tb)
                .then_with(|| match ta {
                    TitleMatch::None => b.0.total_cmp(&a.0),
                    _ => a.1.nav.cmp(&b.1.nav),
                })
                .then_with(|| b.0.total_cmp(&a.0))
        });
        Answer {
            pages: grouped
                .into_iter()
                .take(MAX_PAGES)
                .map(|(_, page, sections)| {
                    let more = sections.len().saturating_sub(SECTIONS_PER_PAGE);
                    // Snippets and words for the sections shown only.
                    let sections = sections
                        .into_iter()
                        .take(SECTIONS_PER_PAGE)
                        .map(|found| SectionResult {
                            snippet: self.snippet(&found.prose, &wanted),
                            words: self
                                .spelled(&[&found.heading, &found.prose, &found.code], &wanted),
                            heading: found.heading,
                            path: found.path,
                            url: found.url,
                        })
                        .collect();
                    PageResult {
                        title: page.title.clone(),
                        url: page.url.clone(),
                        trail: page.trail.clone(),
                        document: page.document.clone(),
                        kind: page.kind,
                        title_match: titled(page) != TitleMatch::None,
                        lede: page.lede.clone(),
                        sections,
                        more,
                    }
                })
                .collect(),
        }
    }

    /// The best [`RANKED`] sections for `parsed`: every required word must
    /// match — or, as a `fallback`, any — then structure breaks near-ties.
    fn ranked(
        &self,
        parsed: &Parsed,
        searcher: &tantivy::Searcher,
        pages: &Pages,
        fallback: bool,
    ) -> Vec<(Score, DocAddress)> {
        let Some(query) = self.query(parsed, pages, fallback) else {
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
        searcher.search(&query, &collector).unwrap_or_default()
    }

    /// Splits a query into its words, its `"quoted"` phrases and its
    /// `-excluded` words, each word lowercased as typed and stemmed.
    fn parse(&self, query: &str) -> Parsed {
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
    fn words(&self, text: &str, identifier: bool) -> Vec<Word> {
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
    fn stems(&self, text: &str) -> BTreeSet<String> {
        self.words(text, false)
            .into_iter()
            .map(|word| word.stem)
            .collect()
    }

    fn stem(&self, word: &str) -> String {
        let mut stemmed = self.stemmed.clone();
        let mut stem = word.to_string();
        stemmed
            .token_stream(word)
            .process(&mut |token| stem = token.text.clone());
        stem
    }

    /// Lets a word the index doesn't hold match near it: one letter off from
    /// four letters, two from eight.
    fn mark_near(&self, parsed: &mut Parsed, searcher: &tantivy::Searcher) {
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
    fn query(&self, parsed: &Parsed, pages: &Pages, fallback: bool) -> Option<BooleanQuery> {
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
    fn word_query(&self, word: &Word, pages: &Pages) -> Box<dyn Query> {
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
    fn any_field(&self, word: &Word) -> Box<dyn Query> {
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

    /// Whether the query is `title`: the same words but stopwords, the last
    /// perhaps still being typed.
    fn title_match(&self, parsed: &Parsed, title: &str) -> TitleMatch {
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
    fn tokens(&self, text: &str, wanted: &[&Word]) -> Vec<(usize, usize, Option<usize>)> {
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
    fn covered(&self, text: &str, wanted: &[&Word]) -> usize {
        let found: BTreeSet<usize> = self
            .tokens(text, wanted)
            .into_iter()
            .filter_map(|(_, _, matched)| matched)
            .collect();
        found.len()
    }

    /// The words of `texts` that match, as each text spells them, once each.
    fn spelled(&self, texts: &[&str], wanted: &[&Word]) -> Vec<String> {
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
    fn snippet(&self, prose: &str, wanted: &[&Word]) -> String {
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
            html.push_str(&crate::render::escape(&prose[at..begin]));
            let word = crate::render::escape(&prose[begin..end]);
            if matched.is_some() {
                html.push_str(&format!("<mark>{word}</mark>"));
            } else {
                html.push_str(&word);
            }
            at = end;
        }
        html.push_str(&crate::render::escape(&prose[at..to]));
        if to < prose.len() && !ends_sentence(&prose[..to]) {
            html.push('…');
        }
        html.trim().to_string()
    }

    /// Reads everything published again, into a fresh index, in one commit.
    fn rebuild(&self, index: &Index) {
        let mut writer = lock(&self.writer);
        let _ = writer.delete_all_documents();
        let mut pages = BTreeMap::new();
        for (nav, unit) in scope(index, &self.entry).into_iter().enumerate() {
            self.add(&writer, &unit, nav as u64, &mut pages);
        }
        if writer.commit().is_ok() {
            let _ = self.reader.reload();
        }
        *self.pages.write().unwrap_or_else(|p| p.into_inner()) = pages;
    }

    /// Adds the sections of one page, and its record.
    fn add(&self, writer: &IndexWriter, unit: &Unit, nav: u64, pages: &mut Pages) {
        let Ok(source) = std::fs::read_to_string(&unit.file) else {
            return;
        };
        let front = FrontMatter::read(&source);
        if front.search == Some(false) {
            return;
        }
        let read = read(&source);
        let title = match unit.kind {
            PageKind::Slide => slide_title(&source, &read, unit.name.as_deref().unwrap_or("")),
            _ => crate::markup::title(&source, &unit.file),
        };
        let url = match &unit.name {
            Some(name) => format!("{}#{name}", unit.address),
            None => unit.address.clone(),
        };
        let page = Page {
            title,
            url,
            trail: unit.trail.clone(),
            document: unit.document.clone(),
            kind: unit.kind,
            lede: read.lede.clone(),
            nav,
            title_stems: BTreeSet::new(),
            meta_stems: self.stems(&front.meta),
        };
        let page = Page {
            title_stems: self.stems(&page.title),
            ..page
        };
        let add = |entry: Entry| self.add_entry(writer, entry);
        match unit.kind {
            // A slide is one section, at its number: all its text, and its notes.
            PageKind::Slide => {
                let prose: Vec<&str> = read
                    .sections
                    .iter()
                    .flat_map(|s| [s.heading.as_str(), s.prose.as_str()])
                    .collect();
                let code: Vec<&str> = read.sections.iter().map(|s| s.code.as_str()).collect();
                let notes: Vec<String> = crate::markup::notes(&source)
                    .into_iter()
                    .map(|note| note.text)
                    .collect();
                add(Entry {
                    page: &page,
                    heading: &page.title,
                    url: page.url.clone(),
                    path: vec![page.title.clone()],
                    depth: 1,
                    order: 0,
                    prose: &squeeze(&prose.join(" ")),
                    code: &squeeze(&code.join(" ")),
                    notes: &squeeze(&notes.join(" ")),
                    meta: &front.meta,
                });
            }
            PageKind::Page | PageKind::Chapter => {
                let mut above: Vec<(u8, String)> = Vec::new();
                for (order, section) in read.sections.iter().enumerate() {
                    above.retain(|(level, _)| *level < section.level || section.level == 0);
                    if section.level > 0 {
                        above.push((section.level, section.heading.clone()));
                    }
                    let mut path: Vec<String> = above.iter().map(|(_, h)| h.clone()).collect();
                    if path.first() != Some(&page.title) {
                        path.insert(0, page.title.clone());
                    }
                    // As `render::doc` lays them out: with more than one
                    // chapter, each one's ids carry its name, and the chapter
                    // is an id itself.
                    let anchor = match (&section.id, &unit.name) {
                        (Some(id), Some(name)) => Some(format!("{name}--{id}")),
                        (Some(id), None) => Some(id.clone()),
                        (None, Some(name)) => Some(name.clone()),
                        (None, None) => None,
                    };
                    add(Entry {
                        page: &page,
                        heading: &section.heading,
                        url: match anchor {
                            Some(anchor) => format!("{}#{anchor}", unit.address),
                            None => unit.address.clone(),
                        },
                        path,
                        depth: section.level,
                        order,
                        prose: &section.prose,
                        code: &section.code,
                        notes: "",
                        meta: &front.meta,
                    });
                }
            }
        }
        pages.insert(page.url.clone(), page);
    }

    fn add_entry(&self, writer: &IndexWriter, entry: Entry) {
        let fields = self.fields;
        let schema = writer.index().schema();
        let fast = |name: &str| schema.get_field(name).expect("a fast field of the schema");
        let mut doc = TantivyDocument::default();
        doc.add_text(fields.page, &entry.page.url);
        doc.add_text(fields.url, &entry.url);
        for label in &entry.path {
            doc.add_text(fields.path, label);
        }
        for (ranked, words, text) in [
            (
                fields.title,
                Some(fields.title_words),
                entry.page.title.as_str(),
            ),
            (fields.heading, Some(fields.heading_words), entry.heading),
            (fields.prose, Some(fields.prose_words), entry.prose),
            (fields.notes, None, entry.notes),
            (fields.meta, None, entry.meta),
        ] {
            doc.add_text(ranked, text);
            if let Some(words) = words {
                doc.add_text(words, text);
            }
        }
        doc.add_text(fields.code, expand_code(entry.code));
        doc.add_text(fields.code_text, entry.code);
        for (field, text) in [
            (fields.title_exact, entry.page.title.as_str()),
            (fields.heading_exact, entry.heading),
        ] {
            let words = self.words(text, false);
            let kept = kept(&words);
            doc.add_text(field, joined(&kept, |word| &word.stem));
            doc.add_text(field, joined(&kept, |word| &word.typed));
        }
        doc.add_u64(fast("depth"), u64::from(entry.depth));
        doc.add_u64(fast("order"), entry.order as u64);
        doc.add_u64(fast("nav"), entry.page.nav);
        let _ = writer.add_document(doc);
    }
}

/// How much a section's place adds to its score: a little for a shallower
/// heading, a place earlier on its page, and a page earlier in the nav.
fn structure(depth: u64, order: u64, nav: u64) -> Score {
    let depth = depth.max(1) as Score;
    1.0 + 0.06 / depth + 0.04 / (1.0 + order as Score) + 0.04 / (1.0 + nav as Score / 8.0)
}

/// How well a query is a page's title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TitleMatch {
    Whole,
    Start,
    None,
}

/// A query, read: its words, phrases, excluded words, and code identifiers.
#[derive(Default)]
struct Parsed {
    words: Vec<Word>,
    phrases: Vec<Vec<Word>>,
    excluded: Vec<Word>,
    identifiers: Vec<String>,
}

impl Parsed {
    /// How many words and phrases must match.
    fn required(&self) -> usize {
        self.words.iter().filter(|word| !word.stop).count() + self.phrases.len()
    }

    /// The words a section's text is marked with: every word and phrase
    /// word but stopwords.
    fn matched(&self) -> Vec<&Word> {
        self.words
            .iter()
            .chain(self.phrases.iter().flatten())
            .filter(|word| !word.stop)
            .collect()
    }

    /// The query as a whole heading would hold it, stemmed and as typed.
    fn whole(&self) -> (String, String) {
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
struct Word {
    typed: String,
    stem: String,
    /// A stopword need not match.
    stop: bool,
    /// The last word, still being typed: the start of a word matches it.
    last: bool,
    /// How many letters a near match may differ by; 0 when it must be exact.
    near: u8,
}

impl Word {
    /// Whether a token of the text, as typed and stemmed, is this word.
    fn matches(&self, typed: &str, stem: &str) -> bool {
        self.stem == stem
            || (self.near > 0 && distance(&self.typed, typed) <= usize::from(self.near))
            || (self.last && typed.starts_with(&self.typed))
    }
}

/// The words of a title or heading as whole-title matching compares them:
/// without stopwords, unless they are all it has.
fn kept(words: &[Word]) -> Vec<&Word> {
    let content: Vec<&Word> = words.iter().filter(|word| !word.stop).collect();
    if content.is_empty() {
        words.iter().collect()
    } else {
        content
    }
}

fn joined(words: &[&Word], part: impl Fn(&Word) -> &String) -> String {
    words
        .iter()
        .map(|word| part(word).as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn term_query(term: Term) -> Box<dyn Query> {
    Box::new(TermQuery::new(term, IndexRecordOption::WithFreqs))
}

fn boosted(query: Box<dyn Query>, boost: f32) -> Box<dyn Query> {
    Box::new(BoostQuery::new(query, boost))
}

/// `stems` in a row in `field`, within `slop` words; one stem is a term.
fn phrase_query(field: Field, stems: &[&str], slop: u32) -> Box<dyn Query> {
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
fn is_identifier(chunk: &str) -> bool {
    let chars: Vec<char> = chunk.chars().collect();
    chunk.contains("::")
        || chunk.trim_matches('_').contains('_')
        || chars
            .windows(2)
            .any(|pair| pair[0].is_lowercase() && pair[1].is_uppercase())
}

fn is_code_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

/// Code as the `code` field indexes it: each identifier whole, lowercased,
/// and its parts — split at `_`, `::` and a change to upper case.
fn expand_code(code: &str) -> String {
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

fn identifier_parts(identifier: &str) -> Vec<String> {
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

/// Where the sentence holding `at` starts.
fn sentence_start(text: &str, at: usize) -> usize {
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
fn sentence_end(text: &str, at: usize) -> usize {
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

fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(['.', '!', '?'])
}

/// A page's front matter, as search reads it: whether it is searched, and
/// its `description` and `keywords`.
#[derive(Default)]
struct FrontMatter {
    search: Option<bool>,
    meta: String,
}

impl FrontMatter {
    fn read(source: &str) -> Self {
        let Some(yaml) = crate::render::front_matter(source) else {
            return Self::default();
        };
        let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str(yaml) else {
            return Self::default();
        };
        let text = |key: &str| match map.get(key) {
            Some(serde_yaml::Value::String(text)) => text.clone(),
            Some(serde_yaml::Value::Sequence(items)) => items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        Self {
            search: map.get("search").and_then(serde_yaml::Value::as_bool),
            meta: squeeze(&format!("{} {}", text("description"), text("keywords"))),
        }
    }
}

/// What is published from the document file `entry`: the pages its nav
/// reaches — its Topics tree, then its navbar — and the chapters and slides
/// of every document those reach, in reading order. A file that is both a
/// page and a chapter or slide is a chapter or slide only. A page out of
/// reach whose front matter says `search: true` comes last.
fn scope(index: &Index, entry: &Path) -> Vec<Unit> {
    let mut walk = Walk {
        index,
        documents: BTreeSet::new(),
        units: Vec::new(),
    };
    walk.document(entry);
    let parts: BTreeSet<PathBuf> = walk
        .units
        .iter()
        .filter(|unit| unit.kind != PageKind::Page)
        .map(|unit| unit.file.clone())
        .collect();
    let mut seen = BTreeSet::new();
    let mut units: Vec<Unit> = walk
        .units
        .into_iter()
        .filter(|unit| unit.kind != PageKind::Page || !parts.contains(&unit.file))
        .filter(|unit| unit.kind != PageKind::Page || seen.insert(unit.file.clone()))
        .collect();
    for file in markdown_files(index, index.served()) {
        if seen.contains(&file) || parts.contains(&file) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        if FrontMatter::read(&source).search != Some(true) {
            continue;
        }
        let site = index
            .site_for(&file)
            .and_then(|site| Document::load(&site, index.served()).ok());
        let address = format!(
            "/{}",
            file.strip_prefix(index.served()).unwrap_or(&file).display()
        );
        let document = match site {
            Some(site) => Owner {
                title: site.title.clone(),
                path: site.url(&site.path()),
            },
            None => Owner {
                title: crate::markup::title(&source, &file),
                path: address.clone(),
            },
        };
        units.push(Unit {
            file,
            kind: PageKind::Page,
            trail: Vec::new(),
            document,
            address,
            name: None,
        });
    }
    units
}

/// The walk from the served document through what it reaches.
struct Walk<'a> {
    index: &'a Index,
    documents: BTreeSet<PathBuf>,
    units: Vec<Unit>,
}

impl Walk<'_> {
    fn in_sight(&self, file: &Path) -> bool {
        !self.index.ignored(file, false)
    }

    fn document(&mut self, file: &Path) {
        if !self.documents.insert(file.to_path_buf()) || !self.in_sight(file) {
            return;
        }
        let Ok(document) = Document::load(file, self.index.served()) else {
            return;
        };
        let owner = Owner {
            title: document.title.clone(),
            path: document.url(&document.path()),
        };
        match &document.kind {
            Kind::Site { root } => {
                let trails = trails(&document);
                let page = |walk: &mut Self, file: &Path| {
                    if walk.in_sight(file) {
                        walk.units.push(Unit {
                            file: file.to_path_buf(),
                            kind: PageKind::Page,
                            trail: trails.get(file).cloned().unwrap_or_default(),
                            document: owner.clone(),
                            address: document.url(file),
                            name: None,
                        });
                    }
                };
                page(self, &document.base.join(root));
                fn listed(groups: &[crate::nav::Group], out: &mut Vec<PathBuf>) {
                    for entry in groups.iter().flat_map(|group| &group.entries) {
                        out.push(entry.file.clone());
                        listed(&entry.children, out);
                    }
                }
                let mut files = Vec::new();
                listed(&crate::nav::tree(&document), &mut files);
                for file in files {
                    if file.extension().is_some_and(|ext| ext == "md") {
                        page(self, &file);
                    } else {
                        self.document(&file);
                    }
                }
                for file in crate::navbar::reached(&document).0 {
                    if file.extension().is_some_and(|ext| ext == "md") {
                        if file.starts_with(&document.base) {
                            page(self, &file);
                        }
                    } else if crate::document::kind_of(&file).is_some() {
                        self.document(&file);
                    }
                }
            }
            Kind::Doc { chapters, .. } => {
                let scoped = chapters.len() > 1;
                for chapter in chapters {
                    if !self.in_sight(chapter) {
                        continue;
                    }
                    let name = chapter
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let trail = if scoped {
                        std::fs::read_to_string(chapter)
                            .map(|source| vec![crate::markup::title(&source, chapter)])
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    self.units.push(Unit {
                        file: chapter.clone(),
                        kind: PageKind::Chapter,
                        trail,
                        document: owner.clone(),
                        address: owner.path.clone(),
                        name: scoped.then_some(name),
                    });
                }
            }
            Kind::Slides { slides, .. } => {
                for (n, slide) in slides.iter().enumerate() {
                    if !self.in_sight(&slide.file) {
                        continue;
                    }
                    self.units.push(Unit {
                        file: slide.file.clone(),
                        kind: PageKind::Slide,
                        trail: Vec::new(),
                        document: owner.clone(),
                        address: owner.path.clone(),
                        name: Some((n + 1).to_string()),
                    });
                }
            }
            Kind::Design => {}
        }
    }
}

/// Every in-sight Markdown file under `folder`. The walk honors the
/// `.gitignore` files under `folder`, as the index's does, so it never
/// descends into a build folder; the index judges what those above it say.
fn markdown_files(index: &Index, folder: &Path) -> Vec<PathBuf> {
    let walk = WalkBuilder::new(folder)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != ".devpage")
        .build();
    let mut files: Vec<PathBuf> = walk
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter(|path| !index.ignored(path, false))
        .collect();
    files.sort();
    files
}

/// Each page in a site's nav, with the labels from the top of the tree down
/// to it; a page listed twice has the trail to its position.
fn trails(document: &Document) -> BTreeMap<PathBuf, Vec<String>> {
    fn walk(
        groups: &[crate::nav::Group],
        above: &[String],
        out: &mut BTreeMap<PathBuf, Vec<String>>,
    ) {
        for entry in groups.iter().flat_map(|group| &group.entries) {
            let mut trail = above.to_vec();
            trail.push(entry.label.clone());
            walk(&entry.children, &trail, out);
            if entry.home {
                out.entry(entry.file.clone()).or_insert(trail);
            }
        }
    }
    let mut out = BTreeMap::new();
    if let Kind::Site { root } = &document.kind {
        out.insert(document.base.join(root), Vec::new());
    }
    walk(&crate::nav::tree(document), &[], &mut out);
    out
}

/// A slide's title: its first `# ` heading, else its first `title="…"`
/// attribute — a `<Slide>`'s or a `<Hero>`'s — else its number.
fn slide_title(source: &str, read: &Read, number: &str) -> String {
    if let Some(section) = read.sections.iter().find(|s| s.level == 1) {
        return section.heading.clone();
    }
    source
        .split("title=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Slide {number}"))
}

/// A Markdown file split at its headings, each heading with the id the
/// rendered page gives it (`render::markdown`), and its text sorted by kind:
/// prose — with a diagram's labels and a widget's text — and code.
fn read(source: &str) -> Read {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &crate::render::options());
    let lede = root
        .children()
        .find(|node| matches!(node.data().value, NodeValue::Paragraph))
        .map(|node| squeeze(&node.collect_text()))
        .unwrap_or_default();
    let mut anchorizer = Anchorizer::new();
    let mut sections = vec![Section::default()];
    let mut stack: Vec<&AstNode> = root.reverse_children().collect();
    while let Some(node) = stack.pop() {
        if let NodeValue::Heading(heading) = node.data().value {
            let text = node.collect_text();
            let mut id = anchorizer.anchorize(&text);
            if crate::render::FRAME_IDS.contains(&id.as_str()) {
                id.push_str("-heading");
            }
            sections.push(Section {
                heading: text.trim().to_string(),
                id: Some(id),
                level: heading.level,
                ..Section::default()
            });
            continue;
        }
        let current = sections.last_mut().expect("there is always a section");
        match &node.data().value {
            NodeValue::Text(text) => current.prose.push_str(text),
            // Inline code reads as part of its sentence, and is code too.
            NodeValue::Code(code) => {
                current.prose.push_str(&code.literal);
                current.code.push(' ');
                current.code.push_str(&code.literal);
            }
            NodeValue::CodeBlock(block) => {
                if block.info.split_whitespace().next() == Some("mermaid") {
                    current.prose.push(' ');
                    current.prose.push_str(&diagram_labels(&block.literal));
                    current.prose.push(' ');
                } else {
                    current.code.push(' ');
                    current.code.push_str(&block.literal);
                }
            }
            // A slide's `<Notes>` are not shown on the page; a deck adds them
            // to its slide's text itself.
            NodeValue::HtmlBlock(block) if !block.literal.trim_start().starts_with("<Notes") => {
                current.prose.push(' ');
                current.prose.push_str(&widget_text(&block.literal));
                current.prose.push(' ');
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => current.prose.push(' '),
            NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) | NodeValue::FrontMatter(_) => {}
            _ => {
                if node.data().value.block() {
                    current.prose.push(' ');
                }
                stack.extend(node.reverse_children());
            }
        }
    }
    for section in &mut sections {
        section.prose = squeeze(&section.prose);
        section.code = squeeze(&section.code);
    }
    // What comes before the first heading is a section only if it has text.
    if sections[0].prose.is_empty() && sections[0].code.is_empty() {
        sections.remove(0);
    }
    Read { sections, lede }
}

/// `text` with every run of whitespace one space, and none at the ends.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A Mermaid diagram's labels — what its nodes, edges and messages say —
/// without its source: no ids, arrows, styles or `click` lines.
fn diagram_labels(source: &str) -> String {
    const SKIPPED: &[&str] = &[
        "click",
        "classDef",
        "class",
        "style",
        "linkStyle",
        "flowchart",
        "graph",
        "sequenceDiagram",
        "stateDiagram",
        "stateDiagram-v2",
        "classDiagram",
        "erDiagram",
        "direction",
        "end",
        "autonumber",
    ];
    let mut labels = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let first = line.split_whitespace().next().unwrap_or("");
        if line.is_empty() || line.starts_with("%%") || SKIPPED.contains(&first) {
            continue;
        }
        if matches!(first, "participant" | "actor") {
            let name = line
                .split_once(" as ")
                .map_or(&line[first.len()..], |(_, as_)| as_);
            labels.push(name.trim().to_string());
            continue;
        }
        // A message, or a note: the text after its colon.
        if (line.contains("->") || first == "Note" || first == "note")
            && let Some((_, text)) = line.split_once(':')
        {
            labels.push(text.trim().to_string());
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let close = match chars[i] {
                '[' => ']',
                '(' => ')',
                '{' => '}',
                '|' => '|',
                '"' => '"',
                _ => {
                    i += 1;
                    continue;
                }
            };
            let open = chars[i];
            let mut depth = 1;
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == close && (open == close || depth == 1) {
                    break;
                }
                if open != close && chars[j] == open {
                    depth += 1;
                } else if open != close && chars[j] == close {
                    depth -= 1;
                }
                j += 1;
            }
            let label: String = chars[i + 1..j.min(chars.len())].iter().collect();
            labels.push(label);
            i = j + 1;
        }
    }
    let text = labels
        .iter()
        .map(|label| {
            strip_tags(label)
                .trim_matches(|c: char| "\"'()[]{}/\\ ".contains(c))
                .to_string()
        })
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    squeeze(&text)
}

/// The attributes of a widget whose values are its own, not text.
const NOT_TEXT: &[&str] = &[
    "href",
    "src",
    "to",
    "class",
    "className",
    "id",
    "style",
    "icon",
    "logo",
    "width",
    "height",
    "size",
    "columns",
    "kind",
    "variant",
    "type",
    "lang",
    "language",
    "delivery",
    "target",
    "rel",
];

/// The text of an HTML block: what is between its tags, and its tags' text
/// attributes — never its markup, an expression `{…}`, or an address.
fn widget_text(html: &str) -> String {
    let chars: Vec<char> = html.chars().collect();
    let mut text = String::new();
    let mut i = 0;
    let starts = |i: usize, s: &str| chars[i..].iter().take(s.len()).copied().eq(s.chars());
    while i < chars.len() {
        if starts(i, "<!--") {
            i = (i..chars.len())
                .find(|&j| starts(j, "-->"))
                .map_or(chars.len(), |j| j + 3);
            continue;
        }
        let tag = chars[i] == '<'
            && chars
                .get(i + 1)
                .is_some_and(|c| c.is_alphabetic() || *c == '/' || *c == '!');
        if !tag {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        // Inside a tag: its name, then its attributes, up to its `>`.
        i += 1;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
            i += 1;
        }
        let mut name = String::new();
        while i < chars.len() && chars[i] != '>' {
            let c = chars[i];
            if c.is_alphanumeric() || c == '-' || c == '_' {
                name.push(c);
                i += 1;
                continue;
            }
            if c == '=' {
                i += 1;
                continue;
            }
            if c == '"' || c == '\'' {
                let end = (i + 1..chars.len())
                    .find(|&j| chars[j] == c)
                    .unwrap_or(chars.len());
                let value: String = chars[i + 1..end].iter().collect();
                if !NOT_TEXT.contains(&name.as_str()) && !value.contains("://") {
                    text.push(' ');
                    text.push_str(&value);
                    text.push(' ');
                }
                name.clear();
                i = end + 1;
                continue;
            }
            if c == '{' {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
                name.clear();
                continue;
            }
            name.clear();
            i += 1;
        }
        text.push(' ');
        i += 1;
    }
    squeeze(&text)
}

fn strip_tags(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}

/// The edit distance between two words, a swap of neighbours counting as
/// one edit, as tantivy's near matches count it.
fn distance(a: &str, b: &str) -> usize {
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

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A served folder: a site whose Topics list a guide, a doc of two
    /// chapters and a deck with notes; a doc of one chapter whose chapter
    /// the site lists as a page too; a page no nav lists; and a page in a
    /// folder `.gitignore` excludes.
    pub(crate) fn served(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["docs/guide", "docs/build", "docs/features", "plan", "talk"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let write = |path: &str, text: &str| std::fs::write(dir.join(path), text).unwrap();
        write(
            "docs/devpage.yml",
            "kind: site\ntitle: Handbook\nroot: README.md\n",
        );
        write(
            "docs/README.md",
            "# Handbook\n\nWelcome.\n\n## Topics\n\n- [Guide](guide/README.md)\n- [The plan](../plan/plan.yml)\n- [The talk](../talk/talk.yml)\n- [Features](features/features.yml)\n- [Tour](features/tour.md)\n",
        );
        write(
            "docs/guide/README.md",
            "# Guide\n\nHow to read it.\n\n## Topics\n\n- [Layout](layout.md)\n",
        );
        write(
            "docs/guide/layout.md",
            "# Layout\n\nThe page has a header.\n\n## The sidebar\n\nThe sidebar lists every page, and the reader opens a branch to see deeper.\n\n## Navigation keys\n\nArrows move between pages.\n",
        );
        write(
            "docs/features/features.yml",
            "kind: doc\ntitle: Features\nchapters: [tour.md]\n",
        );
        write(
            "docs/features/tour.md",
            "# Tour\n\nA walrus tours it all.\n",
        );
        write("docs/orphan.md", "# Orphan\n\nA lonely heron.\n");
        write("docs/.gitignore", "build/\n");
        write("docs/build/out.md", "# Out\n\nGenerated zebra text.\n");
        write(
            "plan/plan.yml",
            "kind: doc\ntitle: The plan\nchapters: [why.md, how.md]\n",
        );
        write(
            "plan/why.md",
            "# Why\n\n## Context\n\nUsers scroll forever.\n",
        );
        write(
            "plan/how.md",
            "# How\n\n## Context\n\nA quokka index answers.\n",
        );
        write(
            "talk/talk.yml",
            "kind: slides\ntitle: The talk\nslides: [one.md, two.md]\n",
        );
        write("talk/one.md", "# Hello\n\nWelcome, everyone.\n");
        write(
            "talk/two.md",
            "<Slide title=\"Results\">\n\nNumbers.\n\n</Slide>\n\n<Notes>\nSay that the pelican flew home.\n</Notes>\n",
        );
        dir.canonicalize().unwrap()
    }

    fn index(dir: &Path) -> Index {
        Index::build(dir).with_search(&dir.join("docs/devpage.yml"))
    }

    fn find(index: &Index, query: &str) -> Answer {
        index.search().unwrap().search(query)
    }

    fn urls(answer: &Answer) -> Vec<&str> {
        answer.pages.iter().map(|page| page.url.as_str()).collect()
    }

    fn section_urls(answer: &Answer) -> Vec<&str> {
        answer
            .pages
            .iter()
            .flat_map(|page| &page.sections)
            .map(|section| section.url.as_str())
            .collect()
    }

    #[test]
    fn a_section_is_found_by_a_word_in_its_body() {
        let dir = served("body");
        let index = index(&dir);
        let answer = find(&index, "branch");
        assert_eq!(urls(&answer), ["/docs/guide/layout.md"]);
        let page = &answer.pages[0];
        assert_eq!(page.title, "Layout");
        assert_eq!(page.kind, PageKind::Page);
        assert!(!page.title_match);
        assert_eq!(page.lede, "The page has a header.");
        assert_eq!(
            page.document,
            Owner {
                title: "Handbook".into(),
                path: "/docs/devpage.yml".into()
            }
        );
        assert_eq!(page.trail, ["Guide", "Layout"]);
        assert_eq!(page.more, 0);
        let section = &page.sections[0];
        assert_eq!(section.url, "/docs/guide/layout.md#the-sidebar");
        assert_eq!(section.heading, "The sidebar");
        assert_eq!(section.path, ["Layout", "The sidebar"]);
        assert_eq!(section.words, ["branch"]);
        assert_eq!(
            section.snippet,
            "The sidebar lists every page, and the reader opens a <mark>branch</mark> to see deeper."
        );
    }

    #[test]
    fn only_what_is_published_is_searched() {
        let dir = served("scope");
        let index = index(&dir);
        // A page no nav lists, and a page out of sight.
        assert!(find(&index, "heron").pages.is_empty());
        assert!(find(&index, "zebra").pages.is_empty());
        // Front matter puts an unlisted page in, and takes a listed one out.
        std::fs::write(
            dir.join("docs/orphan.md"),
            "---\nsearch: true\n---\n\n# Orphan\n\nA lonely heron.\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/guide/layout.md"),
            "---\nsearch: false\n---\n# Layout\n\nA branch.\n",
        )
        .unwrap();
        let index = super::tests::index(&dir);
        let answer = find(&index, "heron");
        assert_eq!(urls(&answer), ["/docs/orphan.md"]);
        assert_eq!(answer.pages[0].title, "Orphan");
        assert_eq!(answer.pages[0].document.path, "/docs/devpage.yml");
        assert!(find(&index, "branch").pages.is_empty());
    }

    #[test]
    fn a_chapter_listed_as_a_page_too_is_found_once_as_a_chapter() {
        let dir = served("once");
        let index = index(&dir);
        let answer = find(&index, "walrus");
        assert_eq!(urls(&answer), ["/docs/features/features.yml"]);
        assert_eq!(answer.pages[0].kind, PageKind::Chapter);
        assert_eq!(section_urls(&answer), ["/docs/features/features.yml#tour"]);
    }

    #[test]
    fn a_doc_and_a_deck_are_searched_at_their_own_addresses() {
        let dir = served("kinds");
        let index = index(&dir);
        // Two chapters: each heading's id carries its chapter's name, as rendered.
        let answer = find(&index, "quokka");
        assert_eq!(urls(&answer), ["/plan/plan.yml#how"]);
        assert_eq!(section_urls(&answer), ["/plan/plan.yml#how--context"]);
        let page = &answer.pages[0];
        assert_eq!(page.kind, PageKind::Chapter);
        assert_eq!(page.trail, ["How"]);
        assert_eq!(page.document.path, "/plan/plan.yml");
        let html = crate::render::doc(
            &Document::load(&dir.join("plan/plan.yml"), &dir).unwrap(),
            &[dir.join("plan/why.md"), dir.join("plan/how.md")],
            &Default::default(),
            false,
            true,
            &[],
        );
        assert!(html.contains("id=\"how--context\""), "{html}");
        assert!(html.contains("id=\"how\""), "{html}");
        // A slide's notes are searchable, and lead to its slide.
        let answer = find(&index, "pelican");
        assert_eq!(urls(&answer), ["/talk/talk.yml#2"]);
        assert_eq!(section_urls(&answer), ["/talk/talk.yml#2"]);
        let page = &answer.pages[0];
        assert_eq!(page.kind, PageKind::Slide);
        assert_eq!(page.title, "Results");
        assert_eq!(page.document.title, "The talk");
    }

    #[test]
    fn a_page_whose_title_is_the_query_comes_first() {
        let dir = served("title");
        std::fs::write(
            dir.join("docs/guide/README.md"),
            "# Guide\n\nThe layout, the layout and the layout of it.\n\n## Topics\n\n- [Layout](layout.md)\n",
        )
        .unwrap();
        let index = index(&dir);
        for query in ["layout", "Layout", "layo"] {
            let answer = find(&index, query);
            assert_eq!(answer.pages[0].url, "/docs/guide/layout.md", "{query}");
            assert!(answer.pages[0].title_match, "{query}");
            assert!(urls(&answer).contains(&"/docs/guide/README.md"));
        }
        assert!(!find(&index, "layout keys").pages[0].title_match);
    }

    #[test]
    fn a_typo_still_finds_a_word_the_index_lacks() {
        let dir = served("typo");
        let index = index(&dir);
        let answer = find(&index, "sidbar");
        assert!(
            section_urls(&answer).contains(&"/docs/guide/layout.md#the-sidebar"),
            "{answer:#?}"
        );
        let section = &answer.pages[0].sections[0];
        assert!(
            section.snippet.contains("<mark>sidebar</mark>"),
            "{section:#?}"
        );
        assert_eq!(section.words, ["sidebar"]);
        // Short words must be exact: three letters may not be one off.
        assert!(find(&index, "hox").pages.is_empty());
        // A word the index holds is never taken for a near one.
        assert!(
            find(&index, "pages")
                .pages
                .iter()
                .all(|page| page.url != "/talk/talk.yml#1")
        );
    }

    #[test]
    fn the_last_word_is_matched_while_it_is_typed() {
        let dir = served("prefix");
        let index = index(&dir);
        let answer = find(&index, "navig");
        assert_eq!(
            section_urls(&answer),
            ["/docs/guide/layout.md#navigation-keys"]
        );
        // Matched by its heading: the snippet is the first sentence, nothing marked.
        assert_eq!(
            answer.pages[0].sections[0].snippet,
            "Arrows move between pages."
        );
        assert_eq!(answer.pages[0].sections[0].words, ["Navigation"]);
        assert_eq!(
            section_urls(&find(&index, "arrows mov")),
            ["/docs/guide/layout.md#navigation-keys"]
        );
        // A word followed by a space is whole.
        assert!(!find(&index, "mov").pages.is_empty());
        assert!(find(&index, "mov ").pages.is_empty());
    }

    #[test]
    fn every_word_must_match_unless_none_do_together() {
        let dir = served("words");
        let index = index(&dir);
        // Both words are in one section only.
        assert_eq!(
            section_urls(&find(&index, "reader branch")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
        // No section holds both: any word, the more of them the better.
        let answer = find(&index, "quokka pelican");
        assert_eq!(answer.pages.len(), 2, "{answer:#?}");
        // A stopword need not match; a query of only stopwords finds them.
        assert_eq!(
            section_urls(&find(&index, "the branch")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
        assert!(!find(&index, "the").pages.is_empty());
    }

    #[test]
    fn a_quoted_phrase_must_match_in_a_row_and_a_minus_word_never() {
        let dir = served("phrase");
        let index = index(&dir);
        assert_eq!(
            section_urls(&find(&index, "\"reader opens\"")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
        assert!(find(&index, "\"opens reader\"").pages.is_empty());
        assert!(
            section_urls(&find(&index, "pages")).contains(&"/docs/guide/layout.md#navigation-keys")
        );
        let without = find(&index, "pages -arrows");
        assert!(
            !section_urls(&without).contains(&"/docs/guide/layout.md#navigation-keys"),
            "{without:#?}"
        );
        assert!(find(&index, "-arrows").pages.is_empty());
    }

    #[test]
    fn a_page_shows_three_sections_and_counts_the_rest() {
        let dir = served("group");
        let mut page = String::from("# Otters\n\nAn otter page.\n");
        for n in 1..=5 {
            page.push_str(&format!("\n## Part {n}\n\nAn otter swims.\n"));
        }
        std::fs::write(dir.join("docs/guide/layout.md"), page).unwrap();
        let index = index(&dir);
        let answer = find(&index, "otter");
        assert_eq!(answer.pages.len(), 1);
        assert_eq!(answer.pages[0].sections.len(), 3);
        assert_eq!(answer.pages[0].more, 3);
        assert!(answer.pages[0].title_match, "Otters is the title");
    }

    #[test]
    fn at_most_ten_pages_answer() {
        let dir = served("ten");
        let mut topics = String::from("# Guide\n\n## Topics\n\n");
        for n in 0..12 {
            topics.push_str(&format!("- [P{n}](p{n}.md)\n"));
            std::fs::write(
                dir.join(format!("docs/guide/p{n}.md")),
                format!("# P{n}\n\nA marmot.\n"),
            )
            .unwrap();
        }
        std::fs::write(dir.join("docs/guide/README.md"), topics).unwrap();
        let index = index(&dir);
        assert_eq!(find(&index, "marmot").pages.len(), MAX_PAGES);
    }

    #[test]
    fn code_and_diagrams_are_their_own_kinds_of_text() {
        let dir = served("code");
        std::fs::write(
            dir.join("docs/guide/layout.md"),
            "# Layout\n\nThe engine draws it.\n```mermaid\nflowchart LR\n  a[\"Quick fox\"] -->|jumps| lazyDog\n  click a href \"x.md\"\n```\n\n## Calls\n\nCall `Index::with_search` once.\n\n```rust\nlet tweakScore = build_index();\n```\n\n<Callout title=\"Heads up\" icon=\"rocket\">\nMind the step.\n</Callout>\n",
        )
        .unwrap();
        let index = index(&dir);
        // A diagram's labels are prose; its source is not text at all.
        let answer = find(&index, "fox");
        let section = &answer.pages[0].sections[0];
        assert!(
            section.snippet.contains("Quick <mark>fox</mark>"),
            "{section:#?}"
        );
        assert!(!section.snippet.contains("flowchart"), "{section:#?}");
        assert!(!section.snippet.contains("draws it.Quick"), "{section:#?}");
        assert!(find(&index, "lazydog").pages.is_empty());
        assert!(find(&index, "flowchart").pages.is_empty());
        // Code is found by its identifiers whole and by their parts, and
        // never shows in a snippet.
        for query in [
            "Index::with_search",
            "with_search",
            "tweakScore",
            "tweak",
            "build_index",
        ] {
            assert_eq!(
                section_urls(&find(&index, query)),
                ["/docs/guide/layout.md#calls"],
                "{query}"
            );
        }
        let answer = find(&index, "tweak");
        assert!(!answer.pages[0].sections[0].snippet.contains("tweak"));
        // As the text spells it: the word the query's start matched.
        assert_eq!(answer.pages[0].sections[0].words, ["tweakScore"]);
        // A widget's text attributes are prose; its others are not.
        assert_eq!(
            section_urls(&find(&index, "heads")),
            ["/docs/guide/layout.md#calls"]
        );
        assert!(find(&index, "rocket").pages.is_empty());
    }

    #[test]
    fn front_matter_is_read_and_never_shown() {
        let source = "---\nsearch: true\ndescription: All about otters.\nkeywords: [river, swim]\n---\n# Otters\n\nText.\n";
        let front = FrontMatter::read(source);
        assert_eq!(front.search, Some(true));
        assert_eq!(front.meta, "All about otters. river swim");
        assert_eq!(crate::markup::title(source, Path::new("x.md")), "Otters");
        let html = crate::render::markdown(source);
        assert!(!html.contains("search"), "{html}");
        assert!(html.contains("<h1"), "{html}");
        let dir = served("meta");
        std::fs::write(
            dir.join("docs/guide/layout.md"),
            format!("{}\n## More\n\nNothing here.\n", source),
        )
        .unwrap();
        let index = index(&dir);
        assert_eq!(urls(&find(&index, "river")), ["/docs/guide/layout.md"]);
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let dir = served("empty");
        let index = index(&dir);
        assert!(find(&index, "").pages.is_empty());
        assert!(find(&index, "  \t ").pages.is_empty());
        assert!(find(&index, "\"\"").pages.is_empty());
    }

    #[test]
    fn an_odd_query_never_fails() {
        let dir = served("odd");
        let index = index(&dir);
        let long = "sidebar ".repeat(100);
        for query in [
            "\"",
            "-",
            "--",
            "\"unclosed phrase",
            "::",
            "_",
            "a_",
            "ÄÖü ß",
            "-sidebar",
            "\"the\"",
            "sidebar\"branch\"",
            "a::b::",
            "😀 sidebar",
            &long,
        ] {
            let answer = find(&index, query);
            assert!(answer.pages.len() <= MAX_PAGES, "{query}");
        }
        assert_eq!(
            section_urls(&find(&index, "sidebar\"branch\"")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
    }

    #[test]
    fn changes_keep_it_current() {
        let dir = served("changes");
        let index = index(&dir);
        let search = index.search().unwrap();
        let changed = |path: &Path| {
            index.changed(path);
            search.changed(&index, &[path.to_path_buf()]);
        };
        let layout = dir.join("docs/guide/layout.md");
        std::fs::write(
            &layout,
            "# Layout\n\n## The sidebar\n\nA heron stands still.\n",
        )
        .unwrap();
        changed(&layout);
        assert_eq!(
            section_urls(&search.search("heron")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
        assert!(search.search("branch").pages.is_empty());

        // The guide no longer lists the page: it leaves the index.
        let guide = dir.join("docs/guide/README.md");
        std::fs::write(&guide, "# Guide\n\nHow to read it.\n").unwrap();
        changed(&guide);
        assert!(search.search("heron").pages.is_empty());

        // A chapter of the doc goes away: the doc is invalid, with no entries.
        let how = dir.join("plan/how.md");
        std::fs::remove_file(&how).unwrap();
        changed(&how);
        assert!(search.search("quokka").pages.is_empty());
        assert!(search.search("forever").pages.is_empty());

        // A new rule excludes the talk: its slides leave the index.
        let rule = dir.join(".gitignore");
        std::fs::write(&rule, "talk/\n").unwrap();
        changed(&rule);
        assert!(search.search("pelican").pages.is_empty());
        assert_eq!(urls(&search.search("welcome")), ["/docs/README.md"]);
    }

    #[test]
    fn a_section_split_keeps_rendered_ids_and_sorts_text() {
        let source = "Intro text.\n\n# Comments\n\nx\n\n## Two `words`\n\ny\n\n## Two words\n\nBefore.\n```\ncode here\n```\n";
        let found = read(source).sections;
        let ids: Vec<_> = found.iter().map(|s| s.id.clone()).collect();
        let html = crate::render::markdown(source);
        for id in ids.iter().flatten() {
            assert!(html.contains(&format!("id=\"{id}\"")), "{id} in {html}");
        }
        assert_eq!(found[0].prose, "Intro text.");
        assert_eq!(found[2].heading, "Two words");
        assert_eq!(found[2].level, 2);
        assert_eq!(found[3].prose, "Before.");
        assert_eq!(found[3].code, "code here");
    }

    #[test]
    fn a_snippet_is_cut_at_sentences() {
        let dir = served("snippet");
        let index = index(&dir);
        let search = index.search().unwrap();
        let wanted = search.parse("otter");
        let words = wanted.matched();
        let prose = "Rivers run. An otter swims fast. Fish hide.";
        assert_eq!(
            search.snippet(prose, &words),
            "An <mark>otter</mark> swims fast."
        );
        assert_eq!(
            search.snippet("No match here. None.", &words),
            "No match here."
        );
        let long = format!("{} otter {}.", "word ".repeat(60), "word ".repeat(60));
        let snippet = search.snippet(&long, &words);
        assert!(
            snippet.starts_with('…') && snippet.ends_with('…'),
            "{snippet}"
        );
        assert!(snippet.contains("<mark>otter</mark>"), "{snippet}");
    }

    #[test]
    fn code_splits_into_identifiers_and_their_parts() {
        assert_eq!(
            expand_code("Search::changed(tweakScore, HTMLParser) x_1"),
            "search::changed search changed tweakscore tweak score htmlparser html parser x_1 x 1"
        );
        assert!(is_identifier("with_search"));
        assert!(is_identifier("Index::build"));
        assert!(is_identifier("camelCase"));
        assert!(!is_identifier("plain"));
        assert!(!is_identifier("x-forwarded-email"));
    }

    #[test]
    fn near_matches_count_a_swap_as_one_edit() {
        assert_eq!(distance("sidebar", "sidbar"), 1);
        assert_eq!(distance("sidebar", "sideabr"), 1);
        assert_eq!(distance("layout", "layuot"), 1);
        assert_eq!(distance("", "abc"), 3);
    }
}
