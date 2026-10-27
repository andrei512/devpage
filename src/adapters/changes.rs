//! The change bus: how open pages hear there is something new to show —
//! the core's [`Change`]s, each scoped to the documents it concerns, carried
//! to every subscriber. Anything may publish: the file watcher, a comment
//! store, the external-link checker. The bus only carries changes; what a
//! producer forgets when a file changes is the producer's business, not
//! traffic here.

use tokio::sync::broadcast;

pub use devpage_core::changes::{Change, Publish, Scope};

/// How many changes wait for a slow subscriber before it lags. A lagging
/// subscriber hears a refresh, so nothing is ever missed, only merged.
const CAPACITY: usize = 64;

/// The bus. Cheap to clone: every clone publishes to the same subscribers.
#[derive(Clone)]
pub struct Changes(broadcast::Sender<(Scope, Change)>);

impl Default for Changes {
    fn default() -> Self {
        Self(broadcast::channel(CAPACITY).0)
    }
}

impl Publish for Changes {
    fn publish(&self, scope: &Scope, change: Change) {
        Changes::publish(self, scope, change);
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
    use std::sync::Arc;

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
}
