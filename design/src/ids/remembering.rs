//! Files read with each design file's last parse remembered: a new parse is
//! matched against the last ([`super::matched`]), so a save's elements keep
//! their ids — and the nodes they make theirs. What a store that keeps
//! designs between saves reads through; the parses outlive any one read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::Problem;
use crate::fonts::FontFile;
use crate::parse::{self, File};
use crate::scene::{Files, ImageFile};

/// Each design file's last parse, by path. A clone shares them: a store
/// keeps one and reads through it with whichever files each resolve uses.
#[derive(Clone, Default)]
pub struct Parses(Arc<Mutex<HashMap<String, Arc<File>>>>);

impl Parses {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<File>>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether `path`'s last parse is held.
    pub fn holds(&self, path: &str) -> bool {
        self.lock().contains_key(path)
    }

    /// Lets `path`'s last parse go: its next parse starts its ids afresh.
    pub fn forget(&self, path: &str) {
        self.lock().remove(path);
    }
}

/// `files`, with each design file parsed matched against its last parse in
/// [`Parses`] — and remembered there as its last.
pub struct Remembering<F> {
    pub files: F,
    parses: Parses,
}

impl<F> Remembering<F> {
    /// `files`, remembering their parses from none.
    pub fn new(files: F) -> Remembering<F> {
        Remembering::with(files, &Parses::default())
    }

    /// `files`, remembering their parses in `parses`, shared.
    pub fn with(files: F, parses: &Parses) -> Remembering<F> {
        Remembering {
            files,
            parses: parses.clone(),
        }
    }
}

impl<F: Files> Files for Remembering<F> {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.files.bytes(path)
    }

    fn list(&self, path: &str) -> Option<Vec<String>> {
        self.files.list(path)
    }

    fn read(&self, path: &str) -> Option<String> {
        self.files.read(path)
    }

    /// `source` parsed and matched against `path`'s last parse, which it
    /// then is.
    fn parse(&self, path: &str, source: &str) -> Result<Arc<File>, Problem> {
        let fresh = parse::parse(source)?;
        let mut last = self.parses.lock();
        let file = Arc::new(match last.get(path) {
            Some(last) => super::matched(fresh, last),
            None => fresh,
        });
        last.insert(path.to_string(), file.clone());
        Ok(file)
    }

    fn image(&self, path: &str) -> Option<Result<Arc<ImageFile>, String>> {
        self.files.image(path)
    }

    fn font(&self, path: &str) -> Option<Result<Arc<FontFile>, String>> {
        self.files.font(path)
    }
}
