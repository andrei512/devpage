//! The change bus: how open pages hear there is something new to show —
//! the core's [`Change`]s, carried to every subscriber. A bus serves one set
//! of documents — a local devpage's served folder, or one published document
//! in server mode — so a subscriber hears everything published on it.
//! Anything may publish: the file watcher, a comment store, the
//! external-link checker. The bus only carries changes; what a producer
//! forgets when a file changes is the producer's business, not traffic here.

use tokio::sync::broadcast;

pub use devpage_core::changes::{Change, Publish};

/// How many changes wait for a slow subscriber before it lags. A lagging
/// subscriber hears a refresh, so nothing is ever missed, only merged.
const CAPACITY: usize = 64;

/// The bus. Cheap to clone: every clone publishes to the same subscribers.
#[derive(Clone)]
pub struct Changes(broadcast::Sender<Change>);

impl Default for Changes {
    fn default() -> Self {
        Self(broadcast::channel(CAPACITY).0)
    }
}

impl Publish for Changes {
    fn publish(&self, change: Change) {
        Changes::publish(self, change);
    }
}

impl Changes {
    /// Tells every subscriber about `change`. No one listening is not an
    /// error.
    pub fn publish(&self, change: Change) {
        let _ = self.0.send(change);
    }

    /// The changes published from now on.
    pub fn subscribe(&self) -> Subscription {
        Subscription(self.0.subscribe())
    }
}

/// What one subscriber hears.
pub struct Subscription(broadcast::Receiver<Change>);

impl Subscription {
    /// The next change, waiting for it; a refresh when this subscriber fell
    /// behind and changes were merged; `None` once the bus is gone.
    pub async fn next(&mut self) -> Option<Change> {
        match self.0.recv().await {
            Ok(change) => Some(change),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(Change::Refresh),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// Whether this subscriber heard anything since it last asked, without
    /// waiting — for a cache that any change clears.
    pub fn heard(&mut self) -> bool {
        let mut heard = false;
        loop {
            match self.0.try_recv() {
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => heard = true,
                Err(_) => return heard,
            }
        }
    }

    /// Every change this subscriber has already heard, in order — for tests.
    #[cfg(test)]
    pub fn drain(&mut self) -> Vec<Change> {
        let mut heard = Vec::new();
        while let Ok(change) = self.0.try_recv() {
            heard.push(change);
        }
        heard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subscriber_hears_every_change_in_order() {
        let changes = Changes::default();
        let mut one = changes.subscribe();
        let mut other = changes.subscribe();
        changes.publish(Change::Comments("/a.md".into()));
        changes.publish(Change::Refresh);
        let heard = [Change::Comments("/a.md".into()), Change::Refresh];
        assert_eq!(one.drain(), heard);
        assert_eq!(other.drain(), heard);
    }

    #[tokio::test]
    async fn next_waits_for_a_change_and_ends_with_the_bus() {
        let changes = Changes::default();
        let mut heard = changes.subscribe();
        changes.publish(Change::Comments("/b.md".into()));
        assert_eq!(heard.next().await, Some(Change::Comments("/b.md".into())));
        drop(changes);
        assert_eq!(heard.next().await, None);
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_hears_a_refresh() {
        let changes = Changes::default();
        let mut heard = changes.subscribe();
        for n in 0..CAPACITY + 1 {
            changes.publish(Change::Comments(format!("/{n}.md")));
        }
        assert_eq!(heard.next().await, Some(Change::Refresh));
    }

    #[test]
    fn heard_is_any_change_since_the_last_look() {
        let changes = Changes::default();
        let mut heard = changes.subscribe();
        assert!(!heard.heard());
        changes.publish(Change::Comments("/c.md".into()));
        assert!(heard.heard());
        assert!(!heard.heard());
    }
}
