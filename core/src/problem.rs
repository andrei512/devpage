//! A problem: one broken thing a document holds, where it was found — the
//! value link checking, the navbar, widgets and comments report, and the page
//! lists above itself. A leaf: it depends on nothing else in devpage.

/// One broken thing, where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Problem {
    /// The file it is in, from the top of the served folder.
    pub file: String,
    /// The line it is on, counted from 1 — `None` when it is about the whole
    /// file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// The column on `line`, where the problem names one: a design's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// What is broken, for a reader.
    pub message: String,
    /// The link as written, so the page can mark it where it stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(f, "{}:{line}:{column}: {}", self.file, self.message)
            }
            (Some(line), None) => write!(f, "{}:{line}: {}", self.file, self.message),
            _ => write!(f, "{}: {}", self.file, self.message),
        }
    }
}
