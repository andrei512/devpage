//! A publish's zip: packed by the client from exactly the files `devpage
//! check` follows from the document — nothing else lying beside it — and
//! unpacked by the server, to check it and into its cache.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::check;
use crate::designs::Designs;
use crate::folder::Folder;

/// A document zipped: the bytes, the document file's path inside the zip,
/// and every path the zip holds.
pub struct Bundle {
    pub zip: Vec<u8>,
    pub entry: String,
    pub files: Vec<String>,
}

/// Zips the folder's entry document: every file the check follows — each
/// design's as `designs`, the folder's design store, read them — at its
/// path from the deepest folder that holds them all — the document's own
/// folder, unless it links to a file beside it.
pub fn pack(folder: &Folder, designs: &Designs) -> Result<Bundle> {
    let document = folder.entry();
    let files = check::files(folder, designs);
    let root = common_folder(&files).unwrap_or_else(|| document.base.clone());
    let relative = |file: &Path| -> String {
        file.strip_prefix(&root)
            .unwrap_or(file)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut names = Vec::new();
    for file in &files {
        let name = relative(file);
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        writer.start_file(name.as_str(), options)?;
        writer.write_all(&bytes)?;
        names.push(name);
    }
    let zip = writer.finish()?.into_inner();
    Ok(Bundle {
        zip,
        entry: relative(&document.path()),
        files: names,
    })
}

/// Unpacks `zip` into `into`, refusing an entry that would land outside it
/// and a symbolic link.
pub fn unpack(zip: &[u8], into: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_symlink() {
            bail!("{}: a symbolic link", file.name());
        }
        let Some(relative) = file.enclosed_name() else {
            bail!("{}: outside the document", file.name());
        };
        let target = into.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut file, &mut out)?;
    }
    Ok(())
}

/// The deepest folder holding every one of `files`.
fn common_folder(files: &[PathBuf]) -> Option<PathBuf> {
    let mut common: Option<PathBuf> = None;
    for file in files {
        let folder = file.parent()?;
        common = Some(match common {
            None => folder.to_path_buf(),
            Some(so_far) => so_far
                .components()
                .zip(folder.components())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    common
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::folder::ServedFolder;
    use std::sync::Arc;

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-bundle-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// `document` zipped, its designs read through a store of their own.
    fn packed(document: Document) -> Bundle {
        let designs = Designs::new(&document.served);
        pack(&Folder::new(Arc::new(document)), &designs).unwrap()
    }

    fn names(zip: &[u8]) -> Vec<String> {
        let archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    /// The zip holds the document, its pages, what they link to, its widgets
    /// and their imports, its logo and its designs' files — and not the
    /// draft and the secret lying in the same folder.
    #[test]
    fn the_zip_holds_exactly_what_the_check_follows() {
        let dir = folder("exact");
        let write = |path: &str, text: &[u8]| {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        };
        write(
            "docs/devpage.yml",
            b"kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\nlogo: logo.svg\n",
        );
        write(
            "docs/README.md",
            b"# Home\n\n[Two](two.md)\n\n![pic](img/pic.png)\n\n![screen](screen.design)\n\n<Hello />\n",
        );
        write("docs/two.md", b"# Two\n\n[Back](README.md)\n");
        write("docs/img/pic.png", b"png");
        write("docs/logo.svg", b"<svg/>");
        write(
            "docs/screen.design",
            b"import Button from \"./button.design\"\n\n<Frame><Button /></Frame>\n",
        );
        write("docs/button.design", b"<Frame width={10} height={10} />\n");
        write(
            "docs/widgets/Hello.tsx",
            b"import { greet } from \"./lib/greet\";\nexport default function Hello() { return <p>{greet()}</p>; }\n",
        );
        write(
            "docs/widgets/lib/greet.ts",
            b"export const greet = () => \"hi\";\n",
        );
        write("docs/draft.md", b"# Draft\n");
        write("docs/.env", b"SECRET=1\n");
        let document =
            Document::load(&dir.join("docs"), &ServedFolder::new(&dir).unwrap()).unwrap();
        let bundle = packed(document);
        assert_eq!(bundle.entry, "devpage.yml");
        assert_eq!(
            names(&bundle.zip),
            [
                "README.md",
                "button.design",
                "devpage.yml",
                "img/pic.png",
                "logo.svg",
                "screen.design",
                "two.md",
                "widgets/Hello.tsx",
                "widgets/lib/greet.ts",
            ]
        );
    }

    /// A doc whose chapter links to a file beside the doc's folder: the zip
    /// starts at the folder holding both, and names the doc file inside it.
    #[test]
    fn a_link_beside_the_document_widens_the_zip_to_hold_it() {
        let dir = folder("beside");
        std::fs::create_dir_all(dir.join("launch")).unwrap();
        std::fs::create_dir_all(dir.join("shared")).unwrap();
        std::fs::write(
            dir.join("launch/talk.yml"),
            "kind: doc\ntitle: Talk\nchapters: [one.md]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("launch/one.md"),
            "# One\n\n![d](../shared/d.png)\n",
        )
        .unwrap();
        std::fs::write(dir.join("shared/d.png"), "png").unwrap();
        std::fs::write(dir.join("shared/other.png"), "png").unwrap();
        let document = Document::load(
            &dir.join("launch/talk.yml"),
            &ServedFolder::new(&dir).unwrap(),
        )
        .unwrap();
        let bundle = packed(document);
        assert_eq!(bundle.entry, "launch/talk.yml");
        assert_eq!(
            names(&bundle.zip),
            ["launch/one.md", "launch/talk.yml", "shared/d.png"]
        );
    }

    /// What the client packs, the server unpacks file for file.
    #[test]
    fn a_packed_document_unpacks_as_it_was() {
        let dir = folder("round-trip");
        std::fs::create_dir_all(dir.join("doc/img")).unwrap();
        std::fs::write(dir.join("doc/notes.md"), "# Notes\n\n![p](img/p.png)\n").unwrap();
        std::fs::write(dir.join("doc/img/p.png"), "png").unwrap();
        let document =
            Document::load(&dir.join("doc/notes.md"), &ServedFolder::new(&dir).unwrap()).unwrap();
        let bundle = packed(document);
        let out = folder("round-trip-out");
        unpack(&bundle.zip, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join(&bundle.entry)).unwrap(),
            "# Notes\n\n![p](img/p.png)\n"
        );
        assert_eq!(std::fs::read(out.join("img/p.png")).unwrap(), b"png");
    }
}
