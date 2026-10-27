//! What open pages hear: a change is typed — refresh the page, or fetch a
//! page's comments again — and scoped to the documents it concerns, so a
//! page hears only its own. Anything may publish through the [`Publish`]
//! port: the file watcher, a comment store, the external-link checker. The
//! bus that carries changes to open pages is the `devpage` binary's.

use std::sync::Arc;

/// Which documents a change concerns: the documents served under one mount
/// prefix — every document of the served folder at `/` — or every document
/// there is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// What every open page may show, whatever its document: an external
    /// link's answer.
    Every,
    /// The documents served under this mount prefix; empty at `/`.
    Mount(Arc<str>),
}

impl Scope {
    /// The documents served at `/` — everything a local devpage serves.
    pub fn root() -> Self {
        Self::Mount(Arc::from(""))
    }

    /// Whether a page subscribed to `self` hears a change to `scope`.
    pub fn hears(&self, scope: &Scope) -> bool {
        matches!(scope, Scope::Every) || scope == self
    }
}

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
    /// Tells every page subscribed to `scope` — or to any scope, for
    /// [`Scope::Every`] — about `change`. No one listening is not an error.
    fn publish(&self, scope: &Scope, change: Change);
}

/// A [`Publish`] that keeps what it was told, for tests.
#[cfg(any(test, feature = "testing"))]
#[derive(Default)]
pub struct Recorder(std::sync::Mutex<Vec<(Scope, Change)>>);

#[cfg(any(test, feature = "testing"))]
impl Recorder {
    /// The changes published to `scope` — or to every scope — so far, in
    /// order, forgotten once taken.
    pub fn drain(&self, scope: &Scope) -> Vec<Change> {
        std::mem::take(&mut *self.0.lock().unwrap())
            .into_iter()
            .filter(|(published, _)| scope.hears(published))
            .map(|(_, change)| change)
            .collect()
    }
}

#[cfg(any(test, feature = "testing"))]
impl Publish for Recorder {
    fn publish(&self, scope: &Scope, change: Change) {
        self.0.lock().unwrap().push((scope.clone(), change));
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
