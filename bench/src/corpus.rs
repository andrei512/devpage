//! The corpus: documents generated from a seed — the same seed, the same
//! documents — each version as the zip a publish sends and the pages a
//! reader opens. Docs, sites, slides and designs, in the proportions teams
//! write them, with sizes spread around a median and a long tail.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

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

    /// A number in `0.0..1.0`.
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A number around `median` with a long tail: up to eight times it.
    pub fn around(&mut self, median: u64) -> u64 {
        let spread = [1, 1, 1, 2, 2, 3, 4, 8][self.below(8) as usize];
        (median / 2 + self.below(median)) * spread / 2 + 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Doc,
    Site,
    Slides,
    Design,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Doc, Kind::Site, Kind::Slides, Kind::Design];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Doc => "doc",
            Kind::Site => "site",
            Kind::Slides => "slides",
            Kind::Design => "design",
        }
    }

    /// Whether its versions have a download: a PDF or a PNG.
    pub fn downloads(self) -> bool {
        self != Kind::Site
    }
}

/// One version of one document.
pub struct Version {
    /// The document file, at the top of the zip.
    pub entry: String,
    pub zip: Vec<u8>,
    /// What a reader opens, relative to the document's link.
    pub pages: Vec<String>,
}

/// Document `n` of a corpus: its kind — half docs, then sites, slides and a
/// few designs — and how many versions it has: five on average, one to nine.
pub fn shape(seed: u64, n: u64) -> (Kind, u64) {
    let mut rng = Seeded::new(seed ^ n.wrapping_mul(0x2545_F491_4F6C_DD1D));
    let kind = match rng.below(20) {
        0..10 => Kind::Doc,
        10..16 => Kind::Site,
        16..19 => Kind::Slides,
        _ => Kind::Design,
    };
    (kind, 1 + rng.below(9))
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

/// A page's Markdown: headings, paragraphs, code fences, now and then a
/// diagram, and links to its siblings — some to one of their headings.
fn page(rng: &mut Seeded, title: &str, siblings: &[String], sections: u64) -> String {
    let mut out = format!("# {title}\n\n");
    for section in 0..rng.around(sections) {
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
        match rng.below(6) {
            0 | 1 => out += "```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n",
            2 => {
                out += "```mermaid\nflowchart LR\n  a[Publish] --> b[Bucket]\n  b --> c[Cache]\n```\n\n"
            }
            _ => {}
        }
    }
    out
}

fn design(rng: &mut Seeded, n: u64) -> String {
    let mut out = format!(
        "<Frame name=\"Screen {n}\" width={{390}} height={{844}} fill=\"#ffffff\" layout=\"column\" gap={{12}} padding={{24}}>\n"
    );
    for row in 0..rng.around(6) {
        let words = rng.around(4);
        out += &format!(
            "  <Frame name=\"Row {row}\" width=\"fill\" layout=\"row\" gap={{8}} padding={{12}} radius={{8}} fill=\"#f4f4f5\">\n    <Text size={{16}} color=\"#18181b\">{}</Text>\n  </Frame>\n",
            sentence(rng, words)
        );
    }
    out + "</Frame>\n"
}

fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in files {
        writer
            .start_file(name.as_str(), options)
            .expect("a zip in memory");
        writer.write_all(bytes).expect("a zip in memory");
    }
    writer.finish().expect("a zip in memory").into_inner()
}

/// The pages of document `n`, the same in every version: what changes from
/// one version to the next is what they say.
fn names(seed: u64, n: u64, kind: Kind) -> Vec<String> {
    let mut rng = Seeded::new(seed ^ n.wrapping_mul(0x5851_F42D_4C95_7F2D));
    let count = match kind {
        Kind::Site => rng.around(8),
        Kind::Doc => rng.around(4),
        Kind::Slides => rng.around(10),
        Kind::Design => return vec![format!("screen-{n}.design")],
    };
    (0..count).map(|p| format!("page-{p}.md")).collect()
}

/// What a reader opens of document `n`, relative to its link: a site's
/// pages, a doc's or a deck's document file, a design's canvas.
pub fn pages(seed: u64, n: u64) -> Vec<String> {
    let (kind, _) = shape(seed, n);
    match kind {
        Kind::Site | Kind::Design => names(seed, n, kind),
        Kind::Doc | Kind::Slides => vec!["devpage.yml".to_string()],
    }
}

/// Version `v` (from 1) of document `n` of the corpus seeded `seed`.
pub fn version(seed: u64, n: u64, v: u64) -> Version {
    let (kind, _) = shape(seed, n);
    let names = names(seed, n, kind);
    let mut rng =
        Seeded::new(seed ^ n.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ v.wrapping_mul(0x9E37_79B9));
    if kind == Kind::Design {
        let name = names[0].clone();
        return Version {
            entry: name.clone(),
            zip: zip(&[(name, design(&mut rng, n).into_bytes())]),
            pages: pages(seed, n),
        };
    }
    let (siblings, sections): (&[String], u64) = match kind {
        Kind::Slides => (&[], 1),
        _ => (&names, 4),
    };
    let mut files: Vec<(String, String)> = names
        .iter()
        .map(|name| (name.clone(), page(&mut rng, name, siblings, sections)))
        .collect();
    let list: String = names.iter().map(|p| format!("  - {p}\n")).collect();
    let yml = match kind {
        Kind::Site => format!("kind: site\ntitle: Site {n}\nroot: page-0.md\n"),
        Kind::Doc => format!("kind: doc\ntitle: Doc {n}\nchapters:\n{list}"),
        _ => format!("kind: slides\ntitle: Slides {n}\nslides:\n{list}"),
    };
    files.insert(0, ("devpage.yml".to_string(), yml));
    let files: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    Version {
        entry: "devpage.yml".to_string(),
        zip: zip(&files),
        pages: pages(seed, n),
    }
}

/// A folder of real documents published as one more site — devpage's own
/// `docs/` — as the CLI publishes it: the folder under its own name, entry
/// `<folder>/devpage.yml`, beside the files at the top of the folder above it
/// that its pages link to, such as a licence. Its Markdown files are pages.
pub fn folder(root: &Path) -> Result<Version> {
    let root = root
        .canonicalize()
        .with_context(|| format!("reading {}", root.display()))?;
    let parent = root.parent().context("a folder with a folder above it")?;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let path = entry?.path();
        if path.is_file() {
            files.push(path);
        }
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    let mut named = Vec::new();
    for path in files {
        let name = path
            .strip_prefix(parent)?
            .to_string_lossy()
            .replace('\\', "/");
        named.push((name, std::fs::read(&path)?));
    }
    named.sort_by(|a, b| a.0.cmp(&b.0));
    let top = root
        .file_name()
        .context("a folder with a name")?
        .to_string_lossy()
        .to_string();
    let entry = format!("{top}/devpage.yml");
    anyhow::ensure!(
        named.iter().any(|(name, _)| *name == entry),
        "{} has no devpage.yml at its top",
        root.display()
    );
    let pages = named
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| name.starts_with(&format!("{top}/")) && name.ends_with(".md"))
        .collect();
    Ok(Version {
        entry,
        zip: zip(&named),
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_corpus() {
        for n in 0..40 {
            assert_eq!(shape(7, n), shape(7, n));
            for v in 1..=3 {
                assert_eq!(version(7, n, v).zip, version(7, n, v).zip, "doc {n} v{v}");
            }
        }
        assert_ne!(version(7, 3, 1).zip, version(8, 3, 1).zip);
        assert_ne!(version(7, 3, 1).zip, version(7, 3, 2).zip);
    }

    #[test]
    fn every_kind_is_in_the_corpus_and_averages_five_versions() {
        let shapes: Vec<_> = (0..2000).map(|n| shape(1, n)).collect();
        for kind in Kind::ALL {
            assert!(shapes.iter().any(|(k, _)| *k == kind), "{kind:?}");
        }
        let versions: u64 = shapes.iter().map(|(_, v)| v).sum();
        let mean = versions as f64 / shapes.len() as f64;
        assert!((4.5..5.5).contains(&mean), "{mean}");
    }

    #[test]
    fn every_version_of_a_document_has_the_same_pages() {
        for n in 0..40 {
            assert_eq!(version(3, n, 1).pages, version(3, n, 4).pages);
        }
    }

    #[test]
    fn a_version_zip_holds_its_entry_and_pages() {
        for n in 0..40 {
            let doc = version(1, n, 1);
            let archive = zip::ZipArchive::new(std::io::Cursor::new(doc.zip)).unwrap();
            let names: Vec<&str> = archive.file_names().collect();
            assert!(names.contains(&doc.entry.as_str()), "{names:?}");
            for page in doc.pages.iter().filter(|p| p.ends_with(".md")) {
                assert!(names.contains(&page.as_str()), "{page}");
            }
        }
    }

    #[test]
    fn a_folder_is_published_whole_with_its_markdown_as_pages() {
        let above = std::env::temp_dir().join(format!("bench-folder-{}", std::process::id()));
        let root = above.join("docs");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(above.join("src")).unwrap();
        std::fs::write(root.join("devpage.yml"), "kind: site\n").unwrap();
        std::fs::write(root.join("a/b.md"), "# B\n").unwrap();
        std::fs::write(root.join("a/logo.svg"), "<svg/>").unwrap();
        std::fs::write(above.join("LICENSE"), "MIT").unwrap();
        std::fs::write(above.join("src/main.rs"), "fn main() {}").unwrap();
        let site = folder(&root).unwrap();
        assert_eq!(site.entry, "docs/devpage.yml");
        assert_eq!(site.pages, vec!["docs/a/b.md".to_string()]);
        let archive = zip::ZipArchive::new(std::io::Cursor::new(site.zip)).unwrap();
        let mut names: Vec<&str> = archive.file_names().collect();
        names.sort();
        assert_eq!(
            names,
            [
                "LICENSE",
                "docs/a/b.md",
                "docs/a/logo.svg",
                "docs/devpage.yml"
            ]
        );
        std::fs::remove_dir_all(&above).unwrap();
    }
}
