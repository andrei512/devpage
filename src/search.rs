//! Full-text search over the served folder: every in-sight document's pages,
//! chapters and slides, split at their headings into sections, in a tantivy
//! index held in memory. Built at start beside the document index and kept
//! current by the watcher, so a search never reads a file.
//!
//! tantivy, because it is a real full-text engine — BM25 ranking, stemming,
//! near and prefix matches — as a library in pure Rust, with an index in
//! memory: nothing to run beside devpage and nothing written to disk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, parse_document};
use ignore::WalkBuilder;
use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

use crate::document::{Document, Kind};
use crate::index::Index;

/// The most hits one search returns.
pub const MAX_HITS: usize = 50;

/// How much more a word in a page's title counts than one in its body, and a
/// word in a section's heading.
const TITLE_BOOST: f32 = 3.0;
const HEADING_BOOST: f32 = 2.0;

/// What a near match — a typo, or the word being typed — counts, against an
/// exact one.
const NEAR: f32 = 0.5;

/// The tokenizer of the ranked fields: tantivy's default, lowercased, with
/// English stemming. The `*_words` fields keep the words unstemmed, for the
/// last word's prefix.
const STEMMED: &str = "en_stem";
const PLAIN: &str = "default";

/// How many words a snippet shows at most.
const SNIPPET_WORDS: usize = 24;

/// One section found: where it is, a snippet of its text with the matched
/// words in `<mark>`, and the document it belongs to.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hit {
    pub title: String,
    pub heading: String,
    /// The page's address with the heading's anchor.
    pub url: String,
    /// HTML: the section's text, escaped, its matched words in `<mark>`.
    pub snippet: String,
    pub document: Owner,
    /// The page's place in its document's nav: the labels from the top down to it.
    pub trail: Vec<String>,
}

/// The document a hit belongs to: its title and its document file's address.
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
    /// What each indexed document's entries were read from, by its document file.
    reach: Mutex<BTreeMap<PathBuf, Reach>>,
}

#[derive(Clone, Copy)]
struct Fields {
    title: Field,
    heading: Field,
    body: Field,
    title_words: Field,
    heading_words: Field,
    body_words: Field,
    url: Field,
    document: Field,
    document_title: Field,
    document_path: Field,
    trail: Field,
}

/// The files a document's entries were read from: its chapters or slides, or
/// — for a site — every Markdown file in its folder, any of which may be a
/// page or change its nav.
#[derive(Default, Clone)]
struct Reach {
    files: BTreeSet<PathBuf>,
    folder: Option<PathBuf>,
}

/// One section of a Markdown file: its heading's text and id (none for what
/// comes before the first heading), and its text.
#[derive(Debug, PartialEq)]
struct Section {
    heading: String,
    id: Option<String>,
    body: String,
}

/// One entry of the index, before it is a tantivy document.
struct Entry<'a> {
    title: &'a str,
    heading: &'a str,
    body: &'a str,
    url: String,
    trail: &'a [String],
}

impl Search {
    /// Indexes every document `index` holds.
    pub fn build(index: &Index) -> Self {
        let mut schema = Schema::builder();
        let ranked = |stored: bool| {
            let options = TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer(STEMMED)
                    .set_index_option(IndexRecordOption::WithFreqs),
            );
            if stored {
                options.set_stored()
            } else {
                options
            }
        };
        let plain = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(PLAIN)
                .set_index_option(IndexRecordOption::Basic),
        );
        let fields = Fields {
            title: schema.add_text_field("title", ranked(true)),
            heading: schema.add_text_field("heading", ranked(true)),
            body: schema.add_text_field("body", ranked(true)),
            title_words: schema.add_text_field("title_words", plain.clone()),
            heading_words: schema.add_text_field("heading_words", plain.clone()),
            body_words: schema.add_text_field("body_words", plain),
            url: schema.add_text_field("url", STORED),
            document: schema.add_text_field("document", STRING),
            document_title: schema.add_text_field("document_title", STORED),
            document_path: schema.add_text_field("document_path", STORED),
            trail: schema.add_text_field("trail", STORED),
        };
        let tantivy = tantivy::Index::create_in_ram(schema.build());
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
            reach: Mutex::new(BTreeMap::new()),
        };
        search.update(index, index.documents());
        search
    }

    /// Brings the entries up to date after `paths` changed, appeared or
    /// vanished — after `index` has heard of them. Each document a path
    /// reaches is read again whole: its entries are deleted and, if it is
    /// still an in-sight document, added again.
    pub fn changed(&self, index: &Index, paths: &[PathBuf]) {
        let mut documents = BTreeSet::new();
        {
            let reach = lock(&self.reach);
            for path in paths {
                // A `.gitignore` governs its folder; a folder, or a path that
                // is gone and may have been one, everything under it.
                let gitignore = path.file_name().is_some_and(|name| name == ".gitignore");
                let folder = if gitignore {
                    path.parent().map(Path::to_path_buf)
                } else {
                    (path.is_dir() || !path.exists()).then(|| path.clone())
                };
                let markdown = path.extension().is_some_and(|ext| ext == "md");
                for (file, reached) in reach.iter() {
                    let hit = match &folder {
                        Some(folder) => {
                            file.starts_with(folder)
                                || reached.files.iter().any(|f| f.starts_with(folder))
                                || reached.folder.as_ref().is_some_and(|site| {
                                    site.starts_with(folder) || folder.starts_with(site)
                                })
                        }
                        None => {
                            file == path
                                || reached.files.contains(path)
                                || (markdown
                                    && reached
                                        .folder
                                        .as_ref()
                                        .is_some_and(|site| path.starts_with(site)))
                        }
                    };
                    if hit {
                        documents.insert(file.clone());
                    }
                }
                if index.kind(path).is_some() {
                    documents.insert(path.clone());
                }
                if let Some(folder) = &folder {
                    documents.extend(
                        index
                            .documents()
                            .into_iter()
                            .filter(|file| file.starts_with(folder)),
                    );
                }
            }
        }
        if !documents.is_empty() {
            self.update(index, documents.into_iter().collect());
        }
    }

    /// The sections matching `query`, best first, at most [`MAX_HITS`]. Every
    /// word must match; a word of four letters or more may be one letter off,
    /// of eight or more two; the last word may be the start of one.
    pub fn search(&self, query: &str) -> Vec<Hit> {
        let words = self.words(query);
        let Some(query) = self.query(&words) else {
            return Vec::new();
        };
        let searcher = self.reader.searcher();
        let Ok(found) = searcher.search(&query, &TopDocs::with_limit(MAX_HITS).order_by_score())
        else {
            return Vec::new();
        };
        let fields = self.fields;
        found
            .into_iter()
            .filter_map(|(_, address)| searcher.doc::<TantivyDocument>(address).ok())
            .map(|doc| {
                let text = |field: Field| {
                    doc.get_first(field)
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                Hit {
                    title: text(fields.title),
                    heading: text(fields.heading),
                    url: text(fields.url),
                    snippet: self.snippet(&text(fields.body), &words),
                    document: Owner {
                        title: text(fields.document_title),
                        path: text(fields.document_path),
                    },
                    trail: doc
                        .get_all(fields.trail)
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect(),
                }
            })
            .collect()
    }

    /// The query's words, each lowercased as typed and stemmed.
    fn words(&self, query: &str) -> Vec<Word> {
        let mut plain = self.plain.clone();
        let mut stemmed = self.stemmed.clone();
        let mut words = Vec::new();
        plain.token_stream(query).process(&mut |token| {
            let mut stem = token.text.clone();
            stemmed
                .token_stream(&token.text)
                .process(&mut |token| stem = token.text.clone());
            words.push(Word {
                typed: token.text.clone(),
                stem,
            });
        });
        words
    }

    /// Every word must match one of the fields: exactly as stemmed, near it
    /// when it is long enough, or — the last word — as the start of a word.
    fn query(&self, words: &[Word]) -> Option<BooleanQuery> {
        let fields = self.fields;
        let ranked = [
            (fields.title, fields.title_words, TITLE_BOOST),
            (fields.heading, fields.heading_words, HEADING_BOOST),
            (fields.body, fields.body_words, 1.0),
        ];
        let boosted = |query: Box<dyn Query>, boost: f32| -> (Occur, Box<dyn Query>) {
            (Occur::Should, Box::new(BoostQuery::new(query, boost)))
        };
        let clauses: Vec<(Occur, Box<dyn Query>)> = words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                let last = i + 1 == words.len();
                let distance = word.distance();
                let mut either = Vec::new();
                for (field, words_field, boost) in ranked {
                    let term = Term::from_field_text(field, &word.stem);
                    either.push(boosted(
                        Box::new(TermQuery::new(term.clone(), IndexRecordOption::WithFreqs)),
                        boost,
                    ));
                    if distance > 0 {
                        either.push(boosted(
                            Box::new(FuzzyTermQuery::new(term, distance, true)),
                            boost * NEAR,
                        ));
                    }
                    if last {
                        let start = Term::from_field_text(words_field, &word.typed);
                        either.push(boosted(
                            Box::new(FuzzyTermQuery::new_prefix(start, 0, true)),
                            boost * NEAR,
                        ));
                    }
                }
                let either: Box<dyn Query> = Box::new(BooleanQuery::new(either));
                (Occur::Must, either)
            })
            .collect();
        (!clauses.is_empty()).then(|| BooleanQuery::new(clauses))
    }

    /// The part of `body` with the most matched words, at most
    /// [`SNIPPET_WORDS`] words long, escaped, each matched word in `<mark>`.
    fn snippet(&self, body: &str, words: &[Word]) -> String {
        let mut stemmed = self.stemmed.clone();
        let mut tokens = Vec::new();
        stemmed.token_stream(body).process(&mut |token| {
            let typed = body[token.offset_from..token.offset_to].to_lowercase();
            let matched = words.iter().enumerate().any(|(i, word)| {
                word.stem == token.text
                    || (word.distance() > 0
                        && distance(&word.stem, &token.text) <= word.distance() as usize)
                    || (i + 1 == words.len() && typed.starts_with(&word.typed))
            });
            tokens.push((token.offset_from, token.offset_to, matched));
        });
        if tokens.is_empty() {
            return String::new();
        }
        // The window with the most matches; of equals, the first.
        let width = SNIPPET_WORDS.min(tokens.len());
        let start = (0..=tokens.len() - width)
            .max_by_key(|&start| {
                let count = tokens[start..start + width]
                    .iter()
                    .filter(|(_, _, matched)| *matched)
                    .count();
                (count, std::cmp::Reverse(start))
            })
            .unwrap_or(0);
        let window = &tokens[start..start + width];
        let mut html = String::new();
        // Cut at words, and say so; at either end of the text, keep all of it.
        let mut at = if start == 0 {
            0
        } else {
            html.push('…');
            window[0].0
        };
        for &(begin, end, matched) in window {
            html.push_str(&crate::render::escape(&body[at..begin]));
            let word = crate::render::escape(&body[begin..end]);
            if matched {
                html.push_str(&format!("<mark>{word}</mark>"));
            } else {
                html.push_str(&word);
            }
            at = end;
        }
        if start + width == tokens.len() {
            html.push_str(&crate::render::escape(&body[at..]));
        } else {
            html.push('…');
        }
        html
    }

    /// Deletes the entries of each of `documents` and adds again those still
    /// indexed as in-sight document files, in one commit.
    fn update(&self, index: &Index, documents: Vec<PathBuf>) {
        let mut writer = lock(&self.writer);
        let mut reach = lock(&self.reach);
        for file in documents {
            let key = file.to_string_lossy();
            writer.delete_term(Term::from_field_text(self.fields.document, &key));
            if index.kind(&file).is_none() {
                reach.remove(&file);
                continue;
            }
            let served = index.served();
            match Document::load(&file, served) {
                Ok(document) => {
                    let reached = self.add(&writer, index, &document, &key);
                    reach.insert(file, reached);
                }
                // An invalid document has no entries until it is fixed; what
                // it reached before still brings it back.
                Err(_) => {
                    reach.entry(file.clone()).or_default().files.insert(file);
                }
            }
        }
        if writer.commit().is_ok() {
            let _ = self.reader.reload();
        }
    }

    /// Adds `document`'s entries; returns what they were read from.
    fn add(&self, writer: &IndexWriter, index: &Index, document: &Document, key: &str) -> Reach {
        let owner = Owner {
            title: document.title.clone(),
            path: document.url(&document.path()),
        };
        let add = |entry: Entry| {
            let fields = self.fields;
            let mut doc = TantivyDocument::default();
            for (ranked, words, text) in [
                (fields.title, fields.title_words, entry.title),
                (fields.heading, fields.heading_words, entry.heading),
                (fields.body, fields.body_words, entry.body),
            ] {
                doc.add_text(ranked, text);
                doc.add_text(words, text);
            }
            doc.add_text(fields.url, &entry.url);
            doc.add_text(fields.document, key);
            doc.add_text(fields.document_title, &owner.title);
            doc.add_text(fields.document_path, &owner.path);
            for label in entry.trail {
                doc.add_text(fields.trail, label);
            }
            let _ = writer.add_document(doc);
        };
        let in_sight = |file: &Path| !index.ignored(file, false);
        let mut reach = Reach::default();
        match &document.kind {
            Kind::Site { .. } => {
                let trails = trails(document);
                let site = document.path();
                for page in markdown_files(index, &document.base) {
                    if index.site_for(&page).as_ref() != Some(&site) {
                        continue;
                    }
                    let Ok(source) = std::fs::read_to_string(&page) else {
                        continue;
                    };
                    let title = crate::render::title(&source, &page);
                    let trail = trails.get(&page).cloned().unwrap_or_default();
                    let address = document.url(&page);
                    for section in sections(&source) {
                        add(Entry {
                            title: &title,
                            heading: &section.heading,
                            body: &section.body,
                            url: match &section.id {
                                Some(id) => format!("{address}#{id}"),
                                None => address.clone(),
                            },
                            trail: &trail,
                        });
                    }
                }
                reach.folder = Some(document.base.clone());
            }
            Kind::Doc { chapters, .. } => {
                // As `render::doc` lays them out: with more than one chapter,
                // each one's ids carry its name, and the chapter is an id itself.
                let scoped = chapters.len() > 1;
                let address = document.url(&document.path());
                for chapter in chapters.iter().filter(|chapter| in_sight(chapter)) {
                    reach.files.insert(chapter.clone());
                    let Ok(source) = std::fs::read_to_string(chapter) else {
                        continue;
                    };
                    let title = crate::render::title(&source, chapter);
                    let name = chapter
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let trail = if scoped {
                        vec![title.clone()]
                    } else {
                        Vec::new()
                    };
                    for section in sections(&source) {
                        let anchor = match (&section.id, scoped) {
                            (Some(id), true) => Some(format!("{name}--{id}")),
                            (Some(id), false) => Some(id.clone()),
                            (None, true) => Some(name.clone()),
                            (None, false) => None,
                        };
                        add(Entry {
                            title: &title,
                            heading: &section.heading,
                            body: &section.body,
                            url: match anchor {
                                Some(anchor) => format!("{address}#{anchor}"),
                                None => address.clone(),
                            },
                            trail: &trail,
                        });
                    }
                }
            }
            Kind::Slides { slides, .. } => {
                // One entry per slide, which is addressed by its number: its
                // first heading, its text and every one of its `<Notes>`.
                let address = document.url(&document.path());
                for (n, slide) in slides.iter().enumerate() {
                    if !in_sight(&slide.file) {
                        continue;
                    }
                    reach.files.insert(slide.file.clone());
                    let Ok(source) = std::fs::read_to_string(&slide.file) else {
                        continue;
                    };
                    let title = crate::render::title(&source, &slide.file);
                    let sections = sections(&source);
                    let heading = sections
                        .iter()
                        .find(|section| section.id.is_some())
                        .map(|section| section.heading.clone())
                        .unwrap_or_default();
                    let mut body: Vec<String> =
                        sections.into_iter().map(|section| section.body).collect();
                    body.extend(
                        crate::widgets::notes(&source)
                            .into_iter()
                            .map(|note| note.text),
                    );
                    add(Entry {
                        title: &title,
                        heading: &heading,
                        body: body.join(" ").trim(),
                        url: format!("{address}#{}", n + 1),
                        trail: &[],
                    });
                }
            }
            Kind::Design => {}
        }
        reach
    }
}

/// One word of a query: as typed, lowercased, and stemmed as the index is.
struct Word {
    typed: String,
    stem: String,
}

impl Word {
    /// How many letters a near match may differ by: none below four letters,
    /// one from four, two from eight.
    fn distance(&self) -> u8 {
        match self.typed.chars().count() {
            0..4 => 0,
            4..8 => 1,
            _ => 2,
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
/// to it; a page listed twice keeps its first place.
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
            out.entry(entry.file.clone()).or_insert(trail);
        }
    }
    let mut out = BTreeMap::new();
    if let Kind::Site { root } = &document.kind {
        out.insert(document.base.join(root), Vec::new());
    }
    walk(&crate::nav::tree(document), &[], &mut out);
    out
}

/// A Markdown file split at its headings, each heading with the id the
/// rendered page gives it (`render::markdown`).
fn sections(source: &str) -> Vec<Section> {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &crate::render::options());
    let mut anchorizer = Anchorizer::new();
    let mut sections = vec![Section {
        heading: String::new(),
        id: None,
        body: String::new(),
    }];
    let mut stack: Vec<&AstNode> = root.reverse_children().collect();
    while let Some(node) = stack.pop() {
        if let NodeValue::Heading(_) = node.data().value {
            let text = node.collect_text();
            let mut id = anchorizer.anchorize(&text);
            if crate::render::FRAME_IDS.contains(&id.as_str()) {
                id.push_str("-heading");
            }
            sections.push(Section {
                heading: text.trim().to_string(),
                id: Some(id),
                body: String::new(),
            });
            continue;
        }
        let current = sections.last_mut().expect("there is always a section");
        match &node.data().value {
            NodeValue::Text(text) => current.body.push_str(text),
            NodeValue::Code(code) => current.body.push_str(&code.literal),
            NodeValue::CodeBlock(block) => {
                current.body.push_str(&block.literal);
                current.body.push(' ');
            }
            // A slide's `<Notes>` are not shown on the page; a deck adds them
            // to its slide's text itself.
            NodeValue::HtmlBlock(block) if !block.literal.trim_start().starts_with("<Notes") => {
                current.body.push_str(&strip_tags(&block.literal));
                current.body.push(' ');
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => current.body.push(' '),
            NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => {}
            _ => {
                if node.data().value.block() {
                    current.body.push(' ');
                }
                stack.extend(node.reverse_children());
            }
        }
    }
    for section in &mut sections {
        section.body = section
            .body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
    }
    // What comes before the first heading is a section only if it has text.
    if sections[0].body.is_empty() {
        sections.remove(0);
    }
    sections
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

    /// A served folder: a site with a nav, a doc of two chapters, a deck with
    /// notes, and a page in a folder `.gitignore` excludes.
    pub(crate) fn served(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/guide")).unwrap();
        std::fs::create_dir_all(dir.join("docs/build")).unwrap();
        std::fs::create_dir_all(dir.join("plan")).unwrap();
        std::fs::create_dir_all(dir.join("talk")).unwrap();
        let write = |path: &str, text: &str| std::fs::write(dir.join(path), text).unwrap();
        write(
            "docs/devpage.yml",
            "kind: site\ntitle: Handbook\nroot: README.md\n",
        );
        write(
            "docs/README.md",
            "# Handbook\n\nWelcome.\n\n## Topics\n\n- [Guide](guide/README.md)\n",
        );
        write(
            "docs/guide/README.md",
            "# Guide\n\nHow to read it.\n\n## Topics\n\n- [Layout](layout.md)\n",
        );
        write(
            "docs/guide/layout.md",
            "# Layout\n\nThe page has a header.\n\n## The sidebar\n\nThe sidebar lists every page, and the reader opens a branch to see deeper.\n\n## Navigation keys\n\nArrows move between pages.\n",
        );
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
            "# Results\n\nNumbers.\n\n<Notes>\nSay that the pelican flew home.\n</Notes>\n",
        );
        dir.canonicalize().unwrap()
    }

    fn index(dir: &Path) -> Index {
        Index::build(dir).with_search()
    }

    fn urls(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.url.as_str()).collect()
    }

    #[test]
    fn a_section_is_found_by_a_word_in_its_body() {
        let dir = served("body");
        let index = index(&dir);
        let hits = index.search().unwrap().search("branch");
        assert_eq!(urls(&hits), ["/docs/guide/layout.md#the-sidebar"]);
        let hit = &hits[0];
        assert_eq!(hit.title, "Layout");
        assert_eq!(hit.heading, "The sidebar");
        assert_eq!(
            hit.document,
            Owner {
                title: "Handbook".into(),
                path: "/docs/devpage.yml".into()
            }
        );
        assert_eq!(hit.trail, ["Guide", "Layout"]);
        assert!(
            hit.snippet.contains("<mark>branch</mark>"),
            "{}",
            hit.snippet
        );
    }

    #[test]
    fn a_word_in_the_title_beats_the_same_word_in_a_body() {
        let dir = served("title");
        std::fs::write(
            dir.join("docs/guide/README.md"),
            "# Guide\n\nHow to read the layout of it.\n\n## Topics\n\n- [Layout](layout.md)\n",
        )
        .unwrap();
        let index = index(&dir);
        let hits = index.search().unwrap().search("layout");
        assert_eq!(hits[0].title, "Layout", "{hits:#?}");
        assert!(
            urls(&hits).contains(&"/docs/guide/README.md#guide"),
            "{hits:#?}"
        );
    }

    #[test]
    fn a_typo_still_finds_the_word() {
        let dir = served("typo");
        let index = index(&dir);
        let hits = index.search().unwrap().search("sidbar");
        assert!(
            urls(&hits).contains(&"/docs/guide/layout.md#the-sidebar"),
            "{hits:#?}"
        );
        assert!(
            hits[0].snippet.contains("<mark>sidebar</mark>"),
            "{hits:#?}"
        );
        // Short words must be exact: three letters may not be one off.
        assert!(index.search().unwrap().search("hox").is_empty());
    }

    #[test]
    fn the_last_word_is_matched_while_it_is_typed() {
        let dir = served("prefix");
        let index = index(&dir);
        let search = index.search().unwrap();
        let hits = search.search("navig");
        assert_eq!(urls(&hits), ["/docs/guide/layout.md#navigation-keys"]);
        // Matched by its heading: the snippet is the body, nothing marked.
        assert_eq!(hits[0].snippet, "Arrows move between pages.");
        assert_eq!(
            urls(&search.search("navigati")),
            ["/docs/guide/layout.md#navigation-keys"]
        );
        // Only the last word: an earlier one must be whole (or near it).
        assert!(search.search("navigati keys").is_empty());
        assert_eq!(
            urls(&search.search("arrows mov")),
            ["/docs/guide/layout.md#navigation-keys"]
        );
    }

    #[test]
    fn a_doc_and_a_deck_are_searched_at_their_own_addresses() {
        let dir = served("kinds");
        let index = index(&dir);
        let search = index.search().unwrap();
        // Two chapters: each heading's id carries its chapter's name, as rendered.
        let hits = search.search("quokka");
        assert_eq!(urls(&hits), ["/plan/plan.yml#how--context"]);
        assert_eq!(hits[0].trail, ["How"]);
        assert_eq!(hits[0].document.path, "/plan/plan.yml");
        let html = crate::render::doc(
            &Document::load(&dir.join("plan/plan.yml"), &dir).unwrap(),
            &[dir.join("plan/why.md"), dir.join("plan/how.md")],
            &Default::default(),
            false,
            true,
        );
        assert!(html.contains("id=\"how--context\""), "{html}");
        // A slide's notes are searchable, and lead to its slide.
        let hits = search.search("pelican");
        assert_eq!(urls(&hits), ["/talk/talk.yml#2"]);
        assert_eq!(hits[0].heading, "Results");
        assert_eq!(hits[0].document.title, "The talk");
    }

    #[test]
    fn an_ignored_file_is_never_indexed() {
        let dir = served("ignored");
        let index = index(&dir);
        assert!(index.search().unwrap().search("zebra").is_empty());
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let dir = served("empty");
        let index = index(&dir);
        assert!(index.search().unwrap().search("").is_empty());
        assert!(index.search().unwrap().search("  \t ").is_empty());
    }

    #[test]
    fn changes_keep_it_current() {
        let dir = served("changes");
        let index = index(&dir);
        let search = index.search().unwrap();
        let layout = dir.join("docs/guide/layout.md");
        std::fs::write(
            &layout,
            "# Layout\n\n## The sidebar\n\nA heron stands still.\n",
        )
        .unwrap();
        index.changed(&layout);
        search.changed(&index, std::slice::from_ref(&layout));
        assert_eq!(
            urls(&search.search("heron")),
            ["/docs/guide/layout.md#the-sidebar"]
        );
        assert!(search.search("branch").is_empty());

        // A chapter of the doc goes away: the doc is invalid, with no entries.
        let how = dir.join("plan/how.md");
        std::fs::remove_file(&how).unwrap();
        index.changed(&how);
        search.changed(&index, std::slice::from_ref(&how));
        assert!(search.search("quokka").is_empty());
        assert!(search.search("forever").is_empty());

        // A new rule excludes the guide: its pages leave the index.
        let rule = dir.join("docs/.gitignore");
        std::fs::write(&rule, "build/\nguide/\n").unwrap();
        index.changed(&rule);
        search.changed(&index, std::slice::from_ref(&rule));
        assert!(search.search("heron").is_empty());
        assert_eq!(
            urls(&search.search("welcome")).len(),
            2,
            "the root and a slide"
        );
    }

    #[test]
    fn a_section_split_keeps_rendered_ids() {
        let source = "Intro text.\n\n# Comments\n\nx\n\n## Two `words`\n\ny\n\n## Two words\n\n```\ncode here\n```\n";
        let found = sections(source);
        let ids: Vec<_> = found.iter().map(|s| s.id.clone()).collect();
        let html = crate::render::markdown(source);
        for id in ids.iter().flatten() {
            assert!(html.contains(&format!("id=\"{id}\"")), "{id} in {html}");
        }
        assert_eq!(found[0].body, "Intro text.");
        assert_eq!(found[2].heading, "Two words");
        assert_eq!(found[3].body, "code here");
    }

    #[test]
    fn near_matches_count_a_swap_as_one_edit() {
        assert_eq!(distance("sidebar", "sidbar"), 1);
        assert_eq!(distance("sidebar", "sideabr"), 1);
        assert_eq!(distance("layout", "layuot"), 1);
        assert_eq!(distance("", "abc"), 3);
    }
}
