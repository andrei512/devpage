//! A comment store in memory, for tests: threads by id, each with its page
//! and whether it is detached — without a disk.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;

use super::{Anchor, Listed, Message, Store, Thread, new_id};

/// A kept thread: its page, the thread, and whether it is detached.
struct Kept {
    page: String,
    thread: Thread,
    detached: bool,
}

/// Threads by id — so they list oldest first.
#[derive(Default)]
pub struct Memory {
    threads: Mutex<BTreeMap<String, Kept>>,
}

impl Memory {
    fn threads(&self) -> MutexGuard<'_, BTreeMap<String, Kept>> {
        self.threads.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Detaches thread `id`, as a publish whose version lost its text does.
    pub fn detach(&self, id: &str) {
        if let Some(kept) = self.threads().get_mut(id) {
            kept.detached = true;
        }
    }
}

fn listed(id: &str, kept: &Kept) -> Listed {
    Listed {
        id: id.into(),
        thread: kept.thread.clone(),
        detached: kept.detached,
    }
}

impl Store for Memory {
    fn list(&self, page: &str) -> Result<Vec<Listed>> {
        Ok(self
            .threads()
            .iter()
            .filter(|(_, kept)| kept.page == page)
            .map(|(id, kept)| listed(id, kept))
            .collect())
    }

    fn create(&self, page: &str, anchor: Anchor, first: Message) -> Result<String> {
        let mut threads = self.threads();
        // Two threads in one tick would share an id; the files would collide
        // too, so the clock has moved on by the next write in practice.
        let mut id = new_id();
        while threads.contains_key(&id) {
            id.push('0');
        }
        threads.insert(
            id.clone(),
            Kept {
                page: page.into(),
                thread: Thread {
                    anchor,
                    thread: vec![first],
                },
                detached: false,
            },
        );
        Ok(id)
    }

    fn reply(&self, id: &str, message: Message) -> Result<Option<String>> {
        Ok(self.threads().get_mut(id).map(|kept| {
            kept.thread.thread.push(message);
            kept.page.clone()
        }))
    }

    fn resolve(&self, id: &str) -> Result<Option<String>> {
        Ok(self.threads().remove(id).map(|kept| kept.page))
    }

    fn attached(&self) -> Result<Vec<(String, Listed)>> {
        Ok(self
            .threads()
            .iter()
            .filter(|(_, kept)| !kept.detached)
            .map(|(id, kept)| (kept.page.clone(), listed(id, kept)))
            .collect())
    }

    fn heard(&self, _: &[PathBuf]) -> Vec<String> {
        Vec::new()
    }
}
