//! A comment store in memory, for tests: the rules of the files — a page
//! that climbs out is refused, an id must be a thread's — without a disk.

use std::collections::BTreeMap;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

use super::{Anchor, Listed, Message, Store, Thread, is_id, is_page, new_id, now};

/// Threads by page, each page's by id — so they list oldest first.
#[derive(Default)]
pub struct Memory {
    pages: Mutex<BTreeMap<String, BTreeMap<String, Thread>>>,
}

/// A page's address with exactly one leading `/`, or an error if it climbs
/// out of the served folder.
fn page_key(page: &str) -> Result<String> {
    if !is_page(page) {
        bail!(
            "{} is not a page in the served folder",
            page.trim_start_matches('/')
        );
    }
    Ok(format!("/{}", page.trim_start_matches('/')))
}

impl Store for Memory {
    fn list(&self, page: &str) -> Result<Vec<Listed>> {
        let pages = self.pages.lock().unwrap();
        Ok(pages
            .get(&page_key(page)?)
            .into_iter()
            .flatten()
            .map(|(id, thread)| Listed {
                id: id.clone(),
                thread: thread.clone(),
                detached: false,
            })
            .collect())
    }

    fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String> {
        let page = page_key(page)?;
        let mut pages = self.pages.lock().unwrap();
        let threads = pages.entry(page).or_default();
        // Two threads in one tick would share an id; the files would collide
        // too, so the clock has moved on by the next write in practice.
        let (mut id, stamp) = new_id();
        while threads.contains_key(&id) {
            id.push('0');
        }
        threads.insert(
            id.clone(),
            Thread {
                anchor,
                thread: vec![Message {
                    author: author.into(),
                    at: stamp,
                    text: text.into(),
                }],
            },
        );
        Ok(id)
    }

    fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()> {
        let page = page_key(page)?;
        let mut pages = self.pages.lock().unwrap();
        let thread = pages
            .get_mut(&page)
            .and_then(|threads| threads.get_mut(id))
            .filter(|_| is_id(id))
            .with_context(|| format!("no thread {id}"))?;
        thread.thread.push(Message {
            author: author.into(),
            at: now().0,
            text: text.into(),
        });
        Ok(())
    }

    fn resolve(&self, page: &str, id: &str) -> Result<()> {
        let page = page_key(page)?;
        let mut pages = self.pages.lock().unwrap();
        pages
            .get_mut(&page)
            .and_then(|threads| threads.remove(id))
            .with_context(|| format!("no thread {id}"))?;
        Ok(())
    }
}
