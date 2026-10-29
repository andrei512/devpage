//! Files held across saves, for tests: each design file's last parse kept,
//! as the store keeps it, so a save's elements keep their ids.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::parse::{self, File};
use crate::{Files, Problem};

#[derive(Default)]
pub(crate) struct Saves {
    pub(crate) files: HashMap<String, String>,
    syntax: RefCell<HashMap<String, Arc<File>>>,
}

impl Saves {
    /// `path`'s text, saved.
    pub(crate) fn save(&mut self, path: &str, text: impl Into<String>) {
        self.files.insert(path.to_string(), text.into());
    }

    /// The design at `path` resolved from the files as saved.
    pub(crate) fn resolve(&self, path: &str) -> crate::Scene {
        crate::resolve(self, path).unwrap_or_else(|p| panic!("{p:?}"))
    }
}

impl Files for Saves {
    fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).map(|s| s.clone().into_bytes())
    }
    fn list(&self, path: &str) -> Option<Vec<String>> {
        crate::scene::listed(self.files.keys(), path)
    }
    fn parse(&self, path: &str, source: &str) -> Result<Arc<File>, Problem> {
        let fresh = parse::parse(source)?;
        let mut syntax = self.syntax.borrow_mut();
        let file = Arc::new(match syntax.get(path) {
            Some(last) => super::matched(fresh, last),
            None => fresh,
        });
        syntax.insert(path.to_string(), file.clone());
        Ok(file)
    }
}
