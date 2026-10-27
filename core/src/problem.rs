//! A problem: one broken thing a document holds, where it was found — the
//! value link checking, the navbar, widgets and comments report, and the page
//! lists above itself. A leaf: it depends on nothing else in devpage.

/// One broken thing, where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Problem {
    /// The file it is in, from the top of the served folder.
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
    /// The link as written, so the page can mark it where it stands.
    pub target: Option<String>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}
