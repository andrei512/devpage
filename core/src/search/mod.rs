//! Full-text search over what the served document publishes: the pages its
//! nav reaches, and the chapters and slides of every document those reach,
//! split at their headings into sections, in a tantivy index held in memory.
//! Built at start beside the served folder's document index and kept current
//! by the watcher a page at a time, so a search never reads a file.
//!
//! tantivy, because it is a real full-text engine — BM25 ranking, stemming,
//! phrases, near and prefix matches — as a library in pure Rust, with an
//! index in memory: nothing to run beside devpage and nothing written to disk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use tantivy::query::Bm25StatisticsProvider;
use tantivy::schema::{Field, Value};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::{IndexReader, Score, Searcher, TantivyDocument, Term};

use crate::folder::Folder;

mod gather;
mod index;
mod query;
mod rank;

use gather::*;
use index::*;
use query::*;
use rank::*;

/// How many pages a search answers, and how many of each page's sections
/// it shows.
const MAX_PAGES: usize = 10;
const SECTIONS_PER_PAGE: usize = 3;

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
    indexer: Mutex<Indexer>,
    stemmed: TextAnalyzer,
    plain: TextAnalyzer,
    /// The served document's file, where what is published starts.
    entry: PathBuf,
    /// What a search reads, as of the last commit.
    snapshot: RwLock<Arc<Snapshot>>,
}

/// The index as one commit left it, read together: its sections, every
/// page's record, and the statistics BM25 scores by.
struct Snapshot {
    searcher: Searcher,
    /// Every page indexed, by its address.
    pages: Pages,
    /// Each field's length over every section, by field id: the sum of its
    /// sections' lengths as the index keeps them.
    tokens: Vec<u64>,
}

impl Snapshot {
    fn new(searcher: Searcher, pages: Pages) -> Self {
        let tokens = searcher
            .schema()
            .fields()
            .map(|(field, _)| {
                searcher
                    .segment_readers()
                    .iter()
                    .filter_map(|segment| {
                        let norms = segment.fieldnorms_readers().get_field(field).ok()??;
                        Some(
                            segment
                                .doc_ids_alive()
                                .map(|doc| u64::from(norms.fieldnorm(doc)))
                                .sum::<u64>(),
                        )
                    })
                    .sum()
            })
            .collect();
        Self {
            searcher,
            pages,
            tokens,
        }
    }
}

/// BM25's statistics are those of the sections the snapshot holds, however
/// they came to be indexed. tantivy's own statistics count a field's length
/// exactly when a segment is written, but estimate it from the sections'
/// lengths when a merge leaves deleted sections out; summing those lengths
/// always makes an index kept current a page at a time score as one built
/// fresh.
impl Bm25StatisticsProvider for Snapshot {
    fn total_num_tokens(&self, field: tantivy::schema::Field) -> tantivy::Result<u64> {
        Ok(self
            .tokens
            .get(field.field_id() as usize)
            .copied()
            .unwrap_or(0))
    }

    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(self.searcher.num_docs())
    }

    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        self.searcher.doc_freq(term)
    }
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

/// A section found, as stored: what its result is made of.
struct Found {
    heading: String,
    path: Vec<String>,
    url: String,
    prose: String,
    code: String,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Search {
    /// Brings the index up to date after `paths` changed, appeared or
    /// vanished — after `folder` has heard of them. Any of them can change
    /// what is published — a page's Topics, a document file, a
    /// `.gitignore`, a folder — and a widget can change which of a page's
    /// tags are widgets, so a burst that holds a Markdown file, a YAML
    /// file, a widget's `.tsx`, a `.gitignore`, a folder or a path that is
    /// gone is indexed. What is published is kept from the last burst, and
    /// found again only when the burst can change it — a Markdown file's
    /// footprint changed (its Topics, a chapter's title, an unlisted page's
    /// `search`), or a file came or went, or is a document file or a
    /// `.gitignore`. Then only the pages new to it, moved in it, or among
    /// `paths` — or whose document's widgets folder holds one of them — are
    /// read again.
    pub fn changed(&self, folder: &Folder, paths: &[PathBuf]) {
        let matters = |path: &PathBuf| {
            path.is_dir()
                || !path.exists()
                || path.file_name().is_some_and(|name| name == ".gitignore")
                || path
                    .extension()
                    .is_some_and(|ext| ext == "md" || ext == "yml" || ext == "yaml" || ext == "tsx")
        };
        if paths.iter().any(matters) {
            self.index(folder, paths);
        }
    }

    /// The pages matching `query`, best first, at most [`MAX_PAGES`], each
    /// with its best sections.
    pub fn search(&self, query: &str) -> Answer {
        let now = Arc::clone(&self.snapshot.read().unwrap_or_else(|p| p.into_inner()));
        let (searcher, pages) = (&now.searcher, &now.pages);
        let mut parsed = self.parse(query);
        self.mark_near(&mut parsed, searcher);
        let mut found = self.ranked(&parsed, &now, false);
        let fallback = found.is_empty() && parsed.required() > 1;
        if fallback {
            found = self.ranked(&parsed, &now, true);
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
}

#[cfg(test)]
mod tests;
