//! The corpus: documents generated from a seed — the same seed, the same
//! documents — each as the zip a publish sends and the pages a reader opens.

use std::io::Write;

/// A seeded generator: xorshift, so the corpus needs no dependency and never
/// changes under one.
pub struct Seeded(u64);

impl Seeded {
    pub fn new(seed: u64) -> Self {
        Seeded(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// A number around `median` with a long tail: up to eight times it.
    pub fn around(&mut self, median: u64) -> u64 {
        let spread = [1, 1, 1, 2, 2, 3, 4, 8][self.below(8) as usize];
        (median / 2 + self.below(median)) * spread / 2 + 1
    }
}

/// One generated document.
pub struct Generated {
    /// The document file, at the top of the zip.
    pub entry: String,
    pub zip: Vec<u8>,
    /// What a reader opens, relative to the document's link.
    pub pages: Vec<String>,
}

const WORDS: &[&str] = &[
    "the", "server", "keeps", "every", "version", "a", "reader", "opens", "link", "page",
    "publish", "cache", "bucket", "comment", "design", "slide", "render", "check", "store", "nav",
];

fn sentence(rng: &mut Seeded, words: u64) -> String {
    (0..words)
        .map(|_| WORDS[rng.below(WORDS.len() as u64) as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// A page's Markdown: headings, paragraphs, a code fence, and links to its
/// siblings — some to one of their headings.
fn page(rng: &mut Seeded, title: &str, siblings: &[String]) -> String {
    let mut out = format!("# {title}\n\n");
    for section in 0..rng.around(4) {
        out += &format!("## Section {section}\n\n");
        for _ in 0..rng.around(3) {
            let words = rng.around(40);
            out += &sentence(rng, words);
            out += ".\n\n";
        }
        if !siblings.is_empty() {
            let to = &siblings[rng.below(siblings.len() as u64) as usize];
            match rng.below(2) {
                0 => out += &format!("See [{to}]({to}).\n\n"),
                _ => out += &format!("See [{to}]({to}#section-0).\n\n"),
            }
        }
        if rng.below(3) == 0 {
            out += "```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n";
        }
    }
    out
}

fn zip(files: &[(String, String)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, text) in files {
        writer
            .start_file(name.as_str(), options)
            .expect("a zip in memory");
        writer.write_all(text.as_bytes()).expect("a zip in memory");
    }
    writer.finish().expect("a zip in memory").into_inner()
}

/// Document `n` of the corpus seeded `seed`: a site of linked pages or a doc
/// of chapters.
pub fn document(seed: u64, n: u64) -> Generated {
    let mut rng = Seeded::new(seed ^ n.wrapping_mul(0x2545_F491_4F6C_DD1D));
    let pages: Vec<String> = (0..rng.around(8)).map(|p| format!("page-{p}.md")).collect();
    let mut files: Vec<(String, String)> = pages
        .iter()
        .map(|name| (name.clone(), page(&mut rng, name, &pages)))
        .collect();
    let site = rng.below(3) > 0;
    let yml = if site {
        format!(
            "kind: site\ntitle: Site {n}\nroot: {}\n",
            pages.first().map(String::as_str).unwrap_or("page-0.md")
        )
    } else {
        let chapters: String = pages.iter().map(|p| format!("  - {p}\n")).collect();
        format!("kind: doc\ntitle: Doc {n}\nchapters:\n{chapters}")
    };
    files.insert(0, ("devpage.yml".to_string(), yml));
    Generated {
        entry: "devpage.yml".to_string(),
        zip: zip(&files),
        pages: if site {
            pages
        } else {
            vec!["devpage.yml".to_string()]
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_corpus() {
        assert_eq!(document(7, 3).zip, document(7, 3).zip);
        assert_ne!(document(7, 3).zip, document(8, 3).zip);
    }

    #[test]
    fn a_document_zip_holds_its_entry_and_pages() {
        let doc = document(1, 1);
        let archive = zip::ZipArchive::new(std::io::Cursor::new(doc.zip)).unwrap();
        let names: Vec<&str> = archive.file_names().collect();
        assert!(names.contains(&"devpage.yml"));
        assert!(names.contains(&"page-0.md"));
    }
}
