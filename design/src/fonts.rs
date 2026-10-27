//! Fonts: a design's text is set in Inter, which the engine embeds, or in a
//! **font file** from the served folder — a `.ttf` or `.otf` of one font,
//! checked as it is read and known by its content, so a scene holds and
//! sends it once however many texts use it.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, FontStyle};

/// A font file's identity: the SHA-256 of its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub [u8; 32]);

impl FontId {
    pub fn of(bytes: &[u8]) -> FontId {
        use sha2::Digest;
        FontId(sha2::Sha256::digest(bytes).into())
    }

    /// The family name the file is registered under for layout: its own,
    /// so a text gets exactly this file — never another font that calls its
    /// family by the same name, Inter included.
    pub fn family(&self) -> String {
        let mut name = String::from("devpage-font-");
        for b in self.0 {
            name.push_str(&format!("{b:02x}"));
        }
        name
    }
}

impl std::fmt::Debug for FontId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// The largest font file a design may use. Every font file a design uses
/// goes whole to each browser that shows it, before the design draws: a
/// whole Latin family, Greek and Cyrillic included, is well under 1 MB, and
/// a larger file covers thousands of characters — Chinese or Japanese — and
/// should be subset to the ones the design shows.
pub const MAX_FONT_BYTES: usize = 4 * 1024 * 1024;

/// The family name of the embedded font, and its weights.
pub const INTER: &str = "Inter";
pub const INTER_WEIGHTS: [u16; 4] = [400, 500, 600, 700];

/// The weights a text may ask for: 100 to 900 in steps of 100.
pub const WEIGHTS: [u16; 9] = [100, 200, 300, 400, 500, 600, 700, 800, 900];

/// The weights one font file has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Weights {
    /// A static font: its one weight.
    Static(f32),
    /// A variable font: its weight axis, from and to.
    Axis(f32, f32),
}

impl Weights {
    pub fn has(&self, weight: u16) -> bool {
        let w = weight as f32;
        match *self {
            Weights::Static(own) => own == w,
            Weights::Axis(from, to) => (from..=to).contains(&w),
        }
    }
}

/// A font file, checked: its bytes, its family's name as its own tables give
/// it, its weights, and whether it is upright.
pub struct FontFile {
    pub id: FontId,
    pub bytes: Arc<Vec<u8>>,
    /// The bytes as parley and both backends take them: one blob for the
    /// file's life, so every cache keyed by it keeps working.
    pub blob: Blob<u8>,
    pub family: String,
    pub weights: Weights,
    pub upright: bool,
}

impl std::fmt::Debug for FontFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFile")
            .field("id", &self.id)
            .field("family", &self.family)
            .field("weights", &self.weights)
            .field("upright", &self.upright)
            .finish()
    }
}

/// Font files compare by their identity, which is their bytes.
impl PartialEq for FontFile {
    fn eq(&self, other: &FontFile) -> bool {
        self.id == other.id
    }
}

/// A scene's font files by id.
pub type FontFiles = std::collections::BTreeMap<FontId, Arc<FontFile>>;

impl FontFile {
    /// Checks `bytes` as a font file, or says why it isn't one — a sentence
    /// that follows the file's path.
    pub fn new(bytes: Vec<u8>) -> Result<FontFile, String> {
        FontFile::with_id(FontId::of(&bytes), bytes)
    }

    /// Checks `bytes`, already known to be the file `id`.
    pub fn with_id(id: FontId, bytes: Vec<u8>) -> Result<FontFile, String> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(format!(
                "is {:.1} MB; a font file is at most {} MB — subset it to the characters the design shows",
                bytes.len() as f64 / (1024.0 * 1024.0),
                MAX_FONT_BYTES / (1024 * 1024)
            ));
        }
        match bytes.get(..4) {
            Some(b"wOFF") => {
                return Err(
                    "is a WOFF file, which devpage doesn't read — decompress it to a .ttf or .otf"
                        .into(),
                );
            }
            Some(b"wOF2") => {
                return Err(
                    "is a WOFF2 file, which devpage doesn't read — decompress it to a .ttf or .otf"
                        .into(),
                );
            }
            Some(b"ttcf") => {
                return Err(
                    "is a collection of fonts; name one font's own .ttf or .otf file".into(),
                );
            }
            Some([0, 1, 0, 0] | b"OTTO" | b"true") => {}
            _ => return Err("is not a font: devpage reads .ttf and .otf files".into()),
        }
        let bytes = Arc::new(bytes);
        let blob = Blob::new(bytes.clone());
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let mut found = collection.register_fonts(blob.clone(), None);
        let (family, info) = match (found.len(), found.first_mut()) {
            (1, Some((family, infos))) if infos.len() == 1 => (*family, infos.remove(0)),
            (0, _) => return Err("is a .ttf or .otf file that doesn't read as a font".into()),
            _ => {
                return Err(
                    "holds more than one font; name one font's own .ttf or .otf file".into(),
                );
            }
        };
        let family = collection
            .family_name(family)
            .unwrap_or_default()
            .to_string();
        let weights = match info.axes().iter().find(|a| a.tag.to_be_bytes() == *b"wght") {
            Some(axis) => Weights::Axis(axis.min, axis.max),
            None => Weights::Static(info.weight().value()),
        };
        Ok(FontFile {
            id,
            bytes,
            blob,
            family,
            weights,
            upright: info.style() == FontStyle::Normal,
        })
    }
}

/// One family's font files, from a file or a folder: the faces a text's
/// weight picks from.
#[derive(Debug)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Arc<FontFile>>,
}

impl Family {
    /// The face a text of `weight` is set in, when the family has it.
    pub fn face(&self, weight: u16) -> Option<&Arc<FontFile>> {
        self.faces.iter().find(|f| f.weights.has(weight))
    }

    /// Every weight a text may ask of the family.
    pub fn weights(&self) -> Vec<u16> {
        WEIGHTS
            .into_iter()
            .filter(|&w| self.face(w).is_some())
            .collect()
    }

    /// A folder's font files, each with its name, as one family — or why
    /// they aren't one. Italic and oblique faces are passed over.
    pub fn of_folder(files: Vec<(String, Arc<FontFile>)>) -> Result<Family, String> {
        if files.is_empty() {
            return Err("holds no .ttf or .otf file".into());
        }
        let upright: Vec<_> = files.into_iter().filter(|(_, f)| f.upright).collect();
        let Some((_, first)) = upright.first() else {
            return Err("holds italic faces only; a text is set upright".into());
        };
        let name = first.family.clone();
        if upright.iter().any(|(_, f)| f.family != name) {
            let mut families: Vec<String> = Vec::new();
            for (file, f) in &upright {
                let entry = format!("{} ({file})", f.family);
                if !families
                    .iter()
                    .any(|e| e.starts_with(&format!("{} (", f.family)))
                {
                    families.push(entry);
                }
            }
            return Err(format!(
                "holds more than one family: {}",
                families.join(", ")
            ));
        }
        for w in WEIGHTS {
            let of: Vec<&str> = upright
                .iter()
                .filter(|(_, f)| f.weights.has(w))
                .map(|(file, _)| file.as_str())
                .collect();
            if of.len() > 1 {
                return Err(format!(
                    "holds two files of weight {w}: {} and {}",
                    of[0], of[1]
                ));
            }
        }
        Ok(Family {
            name,
            faces: upright.into_iter().map(|(_, f)| f).collect(),
        })
    }
}

/// Whether a file in a font folder is one of its fonts, by its name.
pub fn is_font_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".ttf") || lower.ends_with(".otf")
}

/// `weights` as a sentence's list: `300, 400 and 700`.
pub fn list(weights: &[u16]) -> String {
    let words: Vec<String> = weights.iter().map(|w| w.to_string()).collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => "none".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inter() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fonts/Inter-Regular.ttf"
        ))
        .unwrap()
    }

    /// A face as a folder might hold it: Inter's regular, said to be
    /// `weight` and upright or not.
    fn face(weight: f32, upright: bool) -> Arc<FontFile> {
        let mut file = FontFile::new(inter()).unwrap();
        file.weights = Weights::Static(weight);
        file.upright = upright;
        Arc::new(file)
    }

    #[test]
    fn a_font_file_knows_its_family_and_weights() {
        let file = FontFile::new(inter()).unwrap();
        assert_eq!(file.family, "Inter");
        assert_eq!(file.weights, Weights::Static(400.0));
        assert!(file.upright);
        let figtree = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf"
        ))
        .unwrap();
        let file = FontFile::new(figtree).unwrap();
        assert_eq!(file.family, "Figtree");
        assert_eq!(file.weights, Weights::Axis(300.0, 900.0));
        let family = Family {
            name: file.family.clone(),
            faces: vec![Arc::new(file)],
        };
        assert_eq!(family.weights(), [300, 400, 500, 600, 700, 800, 900]);
    }

    /// A folder's italic faces are passed over; one with nothing else is a
    /// problem.
    #[test]
    fn a_folder_is_its_upright_faces() {
        let family = Family::of_folder(vec![
            ("Regular.ttf".into(), face(400.0, true)),
            ("Italic.ttf".into(), face(400.0, false)),
            ("Bold.ttf".into(), face(700.0, true)),
        ])
        .unwrap();
        assert_eq!(family.weights(), [400, 700]);
        assert_eq!(
            Family::of_folder(vec![("Italic.ttf".into(), face(400.0, false))]).unwrap_err(),
            "holds italic faces only; a text is set upright"
        );
        assert_eq!(list(&[400]), "400");
        assert_eq!(list(&[400, 700]), "400 and 700");
    }
}
