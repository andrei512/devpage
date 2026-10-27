//! Comments: threads readers leave on a page, kept by a comment store. The
//! [`Store`] port is what a store must do — list, create, reply, resolve —
//! and [`Comments`] is what the server writes through: every write it makes
//! announces itself on the change bus, so open pages hear it whatever keeps
//! the threads. The YAML files under `.devpage/comments/` are one store
//! ([`yaml`]); tests keep threads in memory ([`memory`]). Where a store
//! keeps threads across versions of a document, [`anchor`] carries each one
//! onto the next version's text.

pub mod anchor;
pub mod yaml;

#[cfg(any(test, feature = "testing"))]
pub mod memory;

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::changes::{Change, Publish, Scope};

/// What a thread is attached to: rendered text, with a little context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Anchor {
    pub quote: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub author: String,
    pub at: String,
    pub text: String,
}

/// A thread as stored: its anchor and its messages, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thread {
    pub anchor: Anchor,
    pub thread: Vec<Message>,
}

/// A thread with its id, as the page receives it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listed {
    pub id: String,
    #[serde(flatten)]
    pub thread: Thread,
    /// Its text is gone from the version shown: the page lists it among the
    /// detached comments wherever its quote may still appear. Only a store
    /// that keeps threads across versions sets it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
}

/// The comment store port: where one served folder's threads are kept. A
/// `page` is the address of the Markdown file a thread is about (`/a.md`);
/// one that climbs out of the served folder, and an id that isn't a thread's,
/// are errors. Writes are silent — announcing them is [`Comments`]'s job.
pub trait Store: Send + Sync {
    /// A page's threads, oldest first.
    fn list(&self, page: &str) -> Result<Vec<Listed>>;
    /// Starts a thread on `page`; answers its id.
    fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String>;
    /// Appends a reply to thread `id`.
    fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()>;
    /// Resolves thread `id`: it is gone.
    fn resolve(&self, page: &str, id: &str) -> Result<()>;
}

/// The comments of the documents under one scope: a store, and the bus every
/// successful write is announced on — `comments <page>`, once, as the write
/// lands — so open pages fetch that page's threads again.
#[derive(Clone)]
pub struct Comments {
    store: Arc<dyn Store>,
    changes: Arc<dyn Publish>,
    scope: Scope,
}

impl Comments {
    pub fn new(store: Arc<dyn Store>, changes: Arc<dyn Publish>, scope: Scope) -> Self {
        Self {
            store,
            changes,
            scope,
        }
    }

    pub fn list(&self, page: &str) -> Result<Vec<Listed>> {
        self.store.list(page)
    }

    pub fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String> {
        let id = self.store.create(page, anchor, author, text)?;
        self.announce(page);
        Ok(id)
    }

    pub fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()> {
        self.store.reply(page, id, author, text)?;
        self.announce(page);
        Ok(())
    }

    pub fn resolve(&self, page: &str, id: &str) -> Result<()> {
        self.store.resolve(page, id)?;
        self.announce(page);
        Ok(())
    }

    fn announce(&self, page: &str) {
        let page = format!("/{}", page.trim_start_matches('/'));
        self.changes.publish(&self.scope, Change::Comments(page));
    }
}

/// A new thread's id: the UTC time it was started, to the second, and four
/// hex digits from the clock's nanoseconds — so a page's threads list oldest
/// first — with when it was started.
pub fn new_id() -> (String, String) {
    let (stamp, nanos) = now();
    let id = format!("{}-{:04x}", stamp.replace(['-', ':'], ""), nanos & 0xffff);
    (id, stamp)
}

/// Whether `id` can be a thread's: letters, digits and dashes, never empty.
pub fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Whether `page` stays inside the served folder: only plain path parts.
pub fn is_page(page: &str) -> bool {
    Path::new(page.trim_start_matches('/'))
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
}

/// The local author: `git config user.name` as git sees it from the served
/// folder — its global setting counts, so no `.git` folder is needed — or the
/// system's user name when git has none.
pub fn local_author(served: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(served)
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}

/// Now, as RFC 3339 UTC to the second, and the sub-second nanoseconds.
fn now() -> (String, u32) {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        ),
        since.subsec_nanos(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn anchor() -> Anchor {
        Anchor {
            quote: "retries".into(),
            prefix: "The client ".into(),
            suffix: ".".into(),
        }
    }

    #[test]
    fn every_write_announces_its_page_once_and_a_failed_one_nothing() {
        let changes = Arc::new(crate::changes::Recorder::default());
        let comments = Comments::new(
            Arc::new(memory::Memory::default()),
            changes.clone(),
            Scope::root(),
        );
        let id = comments
            .create("/a.md", anchor(), "Ana", "Every write?")
            .unwrap();
        comments
            .reply("/a.md", &id, "Bo", "Only the outbox.")
            .unwrap();
        comments.resolve("/a.md", &id).unwrap();
        assert!(comments.reply("/a.md", &id, "Bo", "gone").is_err());
        assert!(comments.create("/../x.md", anchor(), "Ana", "no").is_err());
        let comments_of_a = Change::Comments("/a.md".into());
        assert_eq!(
            changes.drain(&Scope::root()),
            [comments_of_a.clone(), comments_of_a.clone(), comments_of_a],
            "one per write that landed, none for the two refused"
        );
    }

    #[test]
    fn the_local_author_needs_no_git_folder() {
        let folder = std::env::temp_dir().join(format!("devpage-author-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(!folder.join(".git").exists());
        let expected = Command::new("git")
            .args(["config", "--global", "user.name"])
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| std::env::var("USER").ok());
        assert_eq!(Some(local_author(&folder)), expected);
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        let (stamp, _) = now();
        assert_eq!(stamp.len(), 20);
        assert!(
            stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
            "{stamp}"
        );
    }

    #[test]
    fn a_page_that_climbs_out_and_a_made_up_id_are_refused() {
        assert!(is_page("/docs/a.md"));
        assert!(!is_page("/../x.md"));
        assert!(is_id("20261025T181502Z-7f3a"));
        assert!(!is_id("../x") && !is_id(""));
    }
}
