//! The tantivy index: its schema and tokenizers, turning each page
//! published into the sections it is searched by, and keeping it current a
//! page at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use tantivy::indexer::NoMergePolicy;
use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions,
};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, WhitespaceTokenizer};
use tantivy::{
    IndexSettings, IndexSortByField, IndexWriter, Order, ReloadPolicy, TantivyDocument, Term,
};

use crate::folder::Folder;
use crate::widgets::Widgets;

use super::*;

/// The tokenizers: the ranked fields' — tantivy's default, lowercased, with
/// English stemming — their unstemmed twins', for near and prefix matches,
/// code's, and the whole-title fields', which keep a value as one term.
pub(super) const STEMMED: &str = "en_stem";
pub(super) const PLAIN: &str = "default";
pub(super) const CODE: &str = "code";
pub(super) const RAW: &str = "raw";

/// The fast field the index is sorted by: a section's place in what is
/// published, in reading order — its page's place in the nav, then its own
/// on the page.
const PLACE: &str = "place";

#[derive(Clone, Copy)]
pub(super) struct Fields {
    pub(super) page: Field,
    pub(super) url: Field,
    pub(super) path: Field,
    pub(super) title: Field,
    pub(super) heading: Field,
    pub(super) prose: Field,
    pub(super) code: Field,
    pub(super) code_text: Field,
    pub(super) notes: Field,
    pub(super) meta: Field,
    pub(super) title_words: Field,
    pub(super) heading_words: Field,
    pub(super) prose_words: Field,
    pub(super) title_exact: Field,
    pub(super) heading_exact: Field,
}

/// The index's one writer, and what was published when it last indexed —
/// every page, by address, read or not — once it has.
pub(super) struct Indexer {
    writer: IndexWriter,
    published: Option<Published>,
}

/// One section, ready to be a tantivy document.
pub(super) struct Entry<'a> {
    pub(super) page: &'a Page,
    pub(super) heading: &'a str,
    pub(super) url: String,
    pub(super) path: Vec<String>,
    pub(super) depth: u8,
    pub(super) order: usize,
    pub(super) prose: &'a str,
    pub(super) code: &'a str,
    pub(super) notes: &'a str,
    pub(super) meta: &'a str,
}

impl Search {
    /// Indexes what the document `entry` publishes, among the documents of `folder`.
    pub fn build(folder: &Folder, entry: &Path) -> Self {
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
            // Indexed whole, so a page's sections are deleted by its address.
            page: schema.add_text_field("page", STRING | STORED),
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
        for name in ["depth", "order", "nav", PLACE] {
            schema.add_u64_field(name, FAST);
        }
        // Sections stay in reading order, however their pages were indexed,
        // so sections that score alike rank as they read.
        let tantivy = tantivy::Index::builder()
            .schema(schema.build())
            .settings(IndexSettings {
                sort_by_field: Some(IndexSortByField {
                    field: PLACE.to_string(),
                    order: Order::Asc,
                }),
                ..IndexSettings::default()
            })
            .create_in_ram()
            .expect("an index in memory can always be made");
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
        let writer: IndexWriter = tantivy
            .writer_with_num_threads(1, 15_000_000)
            .expect("an index in memory has no lock to contend for");
        // It merges only when told, after each change (`compact`).
        writer.set_merge_policy(Box::new(NoMergePolicy));
        let reader = tantivy
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .expect("an index in memory can always be read");
        let searcher = reader.searcher();
        let search = Self {
            fields,
            reader,
            indexer: Mutex::new(Some(Indexer {
                writer,
                published: None,
            })),
            stemmed,
            plain,
            entry: entry.to_path_buf(),
            snapshot: RwLock::new(Arc::new(Snapshot::new(searcher, BTreeMap::new()))),
        };
        search.index(folder, &[]);
        search
    }

    /// Indexes what `entry` publishes, commits it once and lets the writer
    /// go, threads and all, with what it kept between saves: for files that
    /// never change, such as a published version's.
    pub fn frozen(folder: &Folder, entry: &Path) -> Self {
        let search = Self::build(folder, entry);
        if let Some(indexer) = lock(&search.indexer).take() {
            let _ = indexer.writer.wait_merging_threads();
        }
        search
    }

    /// Brings the index up to what is published now, a page at a time. When
    /// `paths` can change what is published ([`Published::moved_by`]), it is
    /// found again, whole: a page no longer published leaves; a page newly
    /// published, or whose unit — kind, trail, document, address, place in
    /// the nav — differs, or whose file is one of `paths` or under one, is
    /// read again. Otherwise what is published is as it was, and only the
    /// pages `paths` reach are read again. Every other page stays as it was
    /// indexed. All in one commit, then compacted, so the index answers as
    /// one built fresh would. A frozen index has no writer, and stays as it
    /// is.
    pub(super) fn index(&self, folder: &Folder, paths: &[PathBuf]) {
        let mut indexer = lock(&self.indexer);
        let Some(Indexer { writer, published }) = indexer.as_mut() else {
            return;
        };
        let read = |unit: &Unit| paths.iter().any(|path| unit.reached(path));
        let mut pages = self
            .snapshot
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .pages
            .clone();
        let remove = |url: &str, pages: &mut Pages| {
            writer.delete_term(Term::from_field_text(self.fields.page, url));
            pages.remove(url);
        };
        // Each document is loaded once per pass, and its widgets read once,
        // through the pass's own cache: what a page's tags expand to is what
        // its page shows now.
        let (found, mut pass, again): (Option<Published>, Pass, Vec<Unit>) = match published {
            Some(was) if !was.moved_by(paths) => {
                let again: Vec<Unit> = was
                    .units
                    .values()
                    .filter(|unit| read(unit))
                    .cloned()
                    .collect();
                for unit in &again {
                    remove(&unit.url(), &mut pages);
                }
                let documents = std::mem::take(&mut was.documents);
                (None, Pass::with(folder, documents), again)
            }
            _ => {
                let mut pass = Pass::new(folder);
                let now = Published::find(&mut pass, &self.entry);
                let none = BTreeMap::new();
                let was = published.as_ref().map_or(&none, |was| &was.units);
                for (url, unit) in was {
                    if now
                        .units
                        .get(url)
                        .is_none_or(|now| now != unit || read(now))
                    {
                        remove(url, &mut pages);
                    }
                }
                let again = now
                    .units
                    .iter()
                    .filter(|(url, unit)| {
                        was.get(*url).is_none_or(|was| was != *unit || read(unit))
                    })
                    .map(|(_, unit)| unit.clone())
                    .collect();
                (Some(now), pass, again)
            }
        };
        for unit in &again {
            let Some(shown) = &unit.shown_in else {
                continue;
            };
            if let Some(set) = pass.widgets(shown) {
                self.add(writer, unit, set, &mut pages);
            }
        }
        let documents = pass.into_documents();
        let committed = writer.commit().is_ok();
        match (found, published.as_mut()) {
            (Some(mut now), _) if committed => {
                now.documents = documents;
                *published = Some(now);
            }
            (None, Some(was)) => was.documents = documents,
            _ => {}
        }
        if !committed {
            // Back to the last commit, which `published` and `pages` describe.
            let _ = writer.rollback();
            return;
        }
        compact(writer);
        let _ = self.reader.reload();
        *self.snapshot.write().unwrap_or_else(|p| p.into_inner()) =
            Arc::new(Snapshot::new(self.reader.searcher(), pages));
    }

    /// Every page published as the index last found it, by address.
    #[cfg(test)]
    pub(super) fn units(&self) -> BTreeMap<String, Unit> {
        lock(&self.indexer)
            .as_ref()
            .and_then(|indexer| indexer.published.as_ref())
            .map(|published| published.units.clone())
            .unwrap_or_default()
    }

    /// Adds the sections of one page, its content rendered as its page
    /// shows it, with the tags of `widgets` expanded, and its record.
    pub(super) fn add(
        &self,
        writer: &IndexWriter,
        unit: &Unit,
        widgets: &Widgets,
        pages: &mut Pages,
    ) {
        let Ok(source) = std::fs::read_to_string(&unit.file) else {
            return;
        };
        let front = FrontMatter::read(&source);
        if front.search == Some(false) {
            return;
        }
        let read = read(&crate::page::content(widgets, &unit.file, source.clone()));
        let title = match unit.kind {
            PageKind::Slide => slide_title(&source, &read, unit.name.as_deref().unwrap_or("")),
            _ => crate::markup::title(&source, &unit.file),
        };
        let page = Page {
            title,
            url: unit.url(),
            trail: unit.trail.clone(),
            document: unit.document.clone(),
            kind: unit.kind,
            lede: read.lede.clone(),
            nav: unit.nav,
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
                    // As `page::doc` lays them out: with more than one
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

    pub(super) fn add_entry(&self, writer: &IndexWriter, entry: Entry) {
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
        doc.add_u64(fast(PLACE), entry.page.nav << 32 | entry.order as u64);
        let _ = writer.add_document(doc);
    }
}

/// Merges the index into one segment, leaving out the sections deleted. A
/// deleted section still counts in how many sections hold a word until its
/// segment is merged — in a word's BM25 score, and in whether the index
/// holds it at all — so without this a page saved would rank, and
/// near-match, otherwise than in an index built fresh.
fn compact(writer: &mut IndexWriter) {
    let Ok(segments) = writer.index().searchable_segment_metas() else {
        return;
    };
    if segments.len() > 1 || segments.iter().any(|segment| segment.has_deletes()) {
        let ids: Vec<_> = segments.iter().map(|segment| segment.id()).collect();
        let _ = writer.merge(&ids).wait();
    }
}
