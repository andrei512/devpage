//! The change bus: how open pages hear there is something new to show. A
//! change is typed — refresh the page, or fetch a page's comments again —
//! and scoped to the documents it concerns, so a page hears only its own.
//! Anything may publish: the file watcher today, a publish or a comment
//! store tomorrow. The bus only carries changes; what a producer forgets
//! when a file changes is the producer's business, not traffic here.

use std::sync::Arc;

use tokio::sync::broadcast;

/// How many changes wait for a slow subscriber before it lags. A lagging
/// subscriber hears a refresh, so nothing is ever missed, only merged.
const CAPACITY: usize = 64;

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
    fn hears(&self, scope: &Scope) -> bool {
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

/// The bus. Cheap to clone: every clone publishes to the same subscribers.
#[derive(Clone)]
pub struct Changes(broadcast::Sender<(Scope, Change)>);

impl Default for Changes {
    fn default() -> Self {
        Self(broadcast::channel(CAPACITY).0)
    }
}

impl Changes {
    /// Tells every page subscribed to `scope` — or to any scope, for
    /// [`Scope::Every`] — about `change`. No one listening is not an error.
    pub fn publish(&self, scope: &Scope, change: Change) {
        let _ = self.0.send((scope.clone(), change));
    }

    /// The changes `scope` hears from now on.
    pub fn subscribe(&self, scope: Scope) -> Subscription {
        Subscription {
            scope,
            receiver: self.0.subscribe(),
        }
    }
}

/// What one subscriber hears.
pub struct Subscription {
    scope: Scope,
    receiver: broadcast::Receiver<(Scope, Change)>,
}

impl Subscription {
    /// The next change this subscriber hears, waiting for it; a refresh when
    /// it fell behind and changes were merged; `None` once the bus is gone.
    pub async fn next(&mut self) -> Option<Change> {
        loop {
            match self.receiver.recv().await {
                Ok((scope, change)) if self.scope.hears(&scope) => return Some(change),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => return Some(Change::Refresh),
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Whether this subscriber heard anything since it last asked, without
    /// waiting — for a cache that any change clears.
    pub fn heard(&mut self) -> bool {
        let mut heard = false;
        loop {
            match self.receiver.try_recv() {
                Ok((scope, _)) => heard |= self.scope.hears(&scope),
                Err(broadcast::error::TryRecvError::Lagged(_)) => heard = true,
                Err(_) => return heard,
            }
        }
    }

    /// Every change this subscriber has already heard, in order — for tests.
    #[cfg(test)]
    pub fn drain(&mut self) -> Vec<Change> {
        let mut heard = Vec::new();
        while let Ok((scope, change)) = self.receiver.try_recv() {
            if self.scope.hears(&scope) {
                heard.push(change);
            }
        }
        heard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(prefix: &str) -> Scope {
        Scope::Mount(Arc::from(prefix))
    }

    #[test]
    fn a_subscriber_hears_its_scope_and_every_scope_only() {
        let changes = Changes::default();
        let mut root = changes.subscribe(Scope::root());
        let mut other = changes.subscribe(mount("/d/1"));
        changes.publish(&Scope::root(), Change::Refresh);
        changes.publish(&mount("/d/1"), Change::Comments("/a.md".into()));
        changes.publish(&Scope::Every, Change::Refresh);
        assert_eq!(root.drain(), [Change::Refresh, Change::Refresh]);
        assert_eq!(
            other.drain(),
            [Change::Comments("/a.md".into()), Change::Refresh]
        );
    }

    #[tokio::test]
    async fn next_waits_for_its_scope_and_ends_with_the_bus() {
        let changes = Changes::default();
        let mut root = changes.subscribe(Scope::root());
        changes.publish(&mount("/d/1"), Change::Refresh);
        changes.publish(&Scope::root(), Change::Comments("/b.md".into()));
        assert_eq!(root.next().await, Some(Change::Comments("/b.md".into())));
        drop(changes);
        assert_eq!(root.next().await, None);
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_hears_a_refresh() {
        let changes = Changes::default();
        let mut root = changes.subscribe(Scope::root());
        for n in 0..CAPACITY + 1 {
            changes.publish(&Scope::root(), Change::Comments(format!("/{n}.md")));
        }
        assert_eq!(root.next().await, Some(Change::Refresh));
    }

    #[test]
    fn heard_is_any_change_in_scope_since_the_last_look() {
        let changes = Changes::default();
        let mut root = changes.subscribe(Scope::root());
        assert!(!root.heard());
        changes.publish(&mount("/d/1"), Change::Refresh);
        assert!(!root.heard());
        changes.publish(&Scope::root(), Change::Comments("/c.md".into()));
        assert!(root.heard());
        assert!(!root.heard());
    }

    #[test]
    fn the_page_reads_the_same_messages_as_before() {
        assert_eq!(Change::Refresh.message(), "change");
        assert_eq!(
            Change::Comments("/docs/a.md".into()).message(),
            "comments /docs/a.md"
        );
    }
}
