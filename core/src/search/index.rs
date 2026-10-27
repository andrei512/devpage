//! The tantivy index: its schema and tokenizers, and turning each page
//! published into the sections it is searched by.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, RwLock};

use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions,
};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, WhitespaceTokenizer};
use tantivy::{IndexWriter, ReloadPolicy, TantivyDocument};

use crate::index::Index;

use super::*;

/// The tokenizers: the ranked fields' — tantivy's default, lowercased, with
/// English stemming — their unstemmed twins', for near and prefix matches,
/// code's, and the whole-title fields', which keep a value as one term.
pub(super) const STEMMED: &str = "en_stem";
pub(super) const PLAIN: &str = "default";
pub(super) const CODE: &str = "code";
pub(super) const RAW: &str = "raw";

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

    /// Reads everything published again, into a fresh index, in one commit.
    pub(super) fn rebuild(&self, index: &Index) {
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
    pub(super) fn add(&self, writer: &IndexWriter, unit: &Unit, nav: u64, pages: &mut Pages) {
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
        let _ = writer.add_document(doc);
    }
}
