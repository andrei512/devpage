//! What open pages hear: a change is typed — refresh the page, or fetch a
//! page's comments again. Anything may publish through the [`Publish`]
//! port: the file watcher, a comment store, the external-link checker. The
//! bus that carries changes to open pages is the `devpage` binary's, and a
//! bus serves one set of documents — a local devpage's served folder, or one
//! published document in server mode — so every page on it hears every
//! change on it.

/// Something new for open pages to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Fetch the page again.
    Refresh,
    /// The threads on the Markdown file at this address changed.
    Comments(String),
}

impl Change {
    /// The message the page script reads off its event stream.
    pub fn message(&self) -> String {
        match self {
            Change::Refresh => "change".into(),
            Change::Comments(page) => format!("comments {page}"),
        }
    }
}

/// Where a change goes: the bus open pages listen to.
pub trait Publish: Send + Sync {
    /// Tells every page listening on this bus about `change`. No one
    /// listening is not an error.
    fn publish(&self, change: Change);
}

/// A [`Publish`] that keeps what it was told, for tests.
#[cfg(any(test, feature = "testing"))]
#[derive(Default)]
pub struct Recorder(std::sync::Mutex<Vec<Change>>);

#[cfg(any(test, feature = "testing"))]
impl Recorder {
    /// The changes published so far, in order, forgotten once taken.
    pub fn drain(&self) -> Vec<Change> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

#[cfg(any(test, feature = "testing"))]
impl Publish for Recorder {
    fn publish(&self, change: Change) {
        self.0.lock().unwrap().push(change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_reads_the_same_messages_as_before() {
        assert_eq!(Change::Refresh.message(), "change");
        assert_eq!(
            Change::Comments("/docs/a.md".into()).message(),
            "comments /docs/a.md"
        );
    }
}
