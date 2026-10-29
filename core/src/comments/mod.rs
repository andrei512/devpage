//! Comments: threads readers leave on a page, kept by a comment store.
//! [`Comments`] is the only way a thread is written — from a page, from the
//! API, or by a publish carrying threads onto a new version — and it holds
//! every rule of a write: the page key, a new thread's page being a file in
//! the served or version folder, replies and resolves going by thread id,
//! where a carry moves each thread, and the one announcement each write
//! makes on the change bus. A carry is the one write it hands over: the
//! publish commits it together with the version, so both land or neither
//! does. [`Identity`] says who writes. The [`Store`] port is only where
//! threads are kept, by thread id: the YAML files under `.devpage/comments/`
//! ([`yaml`]), server mode's SQLite store, and memory for tests
//! ([`memory`]). [`anchor`] finds a carried thread's quote in the new
//! version's text.

pub mod anchor;
pub mod yaml;

#[cfg(any(test, feature = "testing"))]
pub mod memory;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::changes::{Change, Publish};
use crate::folder::{Folder, ServedFolder};
use crate::widgets::{Modules, Widgets};

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

/// Where a publish carries a document's threads: each one that moves, and
/// the pages they are on. Made by [`Comments::carry`].
#[derive(Debug, Default, PartialEq)]
pub struct Carry {
    moved: Vec<Moved>,
    pages: std::collections::BTreeSet<String>,
}

impl Carry {
    /// The threads that move, each once.
    pub fn moved(&self) -> &[Moved] {
        &self.moved
    }
}

/// A thread carried onto a new version: to `anchor`, or detached — `None` —
/// when its text is gone.
#[derive(Debug, PartialEq)]
pub struct Moved {
    pub id: String,
    pub anchor: Option<Anchor>,
}

/// The comment store port: where one served folder's — or one published
/// document's — threads are kept, by thread id. A `page` here is always a
/// page key, `/a.md`, made by [`Comments`]; an `id` is always one that
/// [`is_id`] accepts. Writes are silent — announcing them is [`Comments`]'s
/// job.
pub trait Store: Send + Sync {
    /// A page's threads, oldest first.
    fn list(&self, page: &str) -> Result<Vec<Listed>>;
    /// Starts a thread on `page` with its first message; answers its id.
    fn create(&self, page: &str, anchor: Anchor, first: Message) -> Result<String>;
    /// Appends `message` to thread `id`: the page it is on, or `None` when
    /// there is no such thread.
    fn reply(&self, id: &str, message: Message) -> Result<Option<String>>;
    /// Resolves thread `id` — it is gone: the page it was on, or `None` when
    /// there was no such thread.
    fn resolve(&self, id: &str) -> Result<Option<String>>;
    /// Every thread still attached, each with its page: what a publish
    /// carries onto the new version.
    fn attached(&self) -> Result<Vec<(String, Listed)>>;
    /// The pages whose threads changed among `paths` by a hand other than
    /// this store's — a thread file edited in an editor. Asking marks them
    /// heard: whoever asks announces them. A store with no files hears
    /// nothing.
    fn heard(&self, paths: &[PathBuf]) -> Vec<String>;
}

/// Why a comment request is refused.
#[derive(Debug)]
pub enum Refused {
    /// The request names no author: the header [`Identity::Header`] names
    /// is missing or blank.
    Unidentified(String),
    /// The page is not a file in the served or version folder.
    NotAPage(String),
    /// There is no such thread.
    NoThread(String),
    /// The store failed.
    Failed(anyhow::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Unidentified(header) => write!(f, "a write needs the {header} header"),
            Refused::NotAPage(page) => write!(f, "{page} is not a page in the served folder"),
            Refused::NoThread(id) => write!(f, "no thread {id}"),
            Refused::Failed(error) => write!(f, "{error:#}"),
        }
    }
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Self {
        Refused::Failed(error)
    }
}

/// Who writes: one local author — the git name locally, `anonymous` on a
/// server with no header — or, deployed, whoever the auth proxy names in a
/// header. devpage authenticates no one itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Identity {
    Local(String),
    Header(String),
}

impl Identity {
    /// The author of a write whose request carries `header`'s value by name:
    /// the local author, or the header's value trimmed — refused when it is
    /// missing or blank.
    pub fn author<'a>(
        &self,
        header: impl FnOnce(&str) -> Option<&'a str>,
    ) -> Result<String, Refused> {
        match self {
            Identity::Local(name) => Ok(name.clone()),
            Identity::Header(name) => header(name)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Refused::Unidentified(name.clone())),
        }
    }
}

/// The comments of one served or version folder: its store, the bus every
/// successful write is announced on — `comments <page>`, once, as the write
/// lands — so open pages fetch that page's threads again, and the folder a
/// new thread's page must be a file in.
#[derive(Clone)]
pub struct Comments {
    store: Arc<dyn Store>,
    changes: Arc<dyn Publish>,
    folder: ServedFolder,
}

impl Comments {
    pub fn new(store: Arc<dyn Store>, changes: Arc<dyn Publish>, folder: ServedFolder) -> Self {
        Self {
            store,
            changes,
            folder,
        }
    }

    /// A page's threads, oldest first.
    pub fn list(&self, page: &str) -> Result<Vec<Listed>, Refused> {
        Ok(self.store.list(&page_key(page)?)?)
    }

    /// Starts a thread on `page`, which must be a file in the folder.
    pub fn create(
        &self,
        page: &str,
        anchor: Anchor,
        author: &str,
        text: &str,
    ) -> Result<String, Refused> {
        let page = page_key(page)?;
        if self.folder.file_at(&page).is_none() {
            return Err(Refused::NotAPage(page));
        }
        let id = self.store.create(&page, anchor, message(author, text))?;
        self.announce(page);
        Ok(id)
    }

    /// Appends a reply to thread `id`, wherever it is — detached included.
    pub fn reply(&self, id: &str, author: &str, text: &str) -> Result<(), Refused> {
        let page = self.found(id, |id| self.store.reply(id, message(author, text)))?;
        self.announce(page);
        Ok(())
    }

    /// Resolves thread `id`: it is gone.
    pub fn resolve(&self, id: &str) -> Result<(), Refused> {
        let page = self.found(id, |id| self.store.resolve(id))?;
        self.announce(page);
        Ok(())
    }

    /// Where every attached thread goes on `version`, this folder opened on
    /// the version's entry document: each finds its quote in its
    /// page's text as the page shows it — front matter left out, widget
    /// tags expanded — by its context where it can, and is detached when
    /// the quote, or the page, is gone. Nothing is written: the publish
    /// commits the [`Carry`] together with its version, so both land or
    /// neither does, and then [`Comments::carried`] announces it. It renders
    /// pages, which draws their diagrams: it blocks.
    pub fn carry(&self, version: &Folder) -> Result<Carry> {
        let modules = Modules::default();
        let mut pages: std::collections::BTreeMap<String, Vec<Listed>> = Default::default();
        for (page, thread) in self.store.attached()? {
            pages.entry(page).or_default().push(thread);
        }
        let mut carry = Carry::default();
        for (page, threads) in pages {
            let text = self
                .content(version, &modules, &page)
                .map(|html| anchor::text(&html));
            for listed in threads {
                let found = text
                    .as_deref()
                    .and_then(|text| anchor::reanchor(&listed.thread.anchor, text));
                if found.as_ref() != Some(&listed.thread.anchor) {
                    carry.moved.push(Moved {
                        id: listed.id,
                        anchor: found,
                    });
                    carry.pages.insert(page.clone());
                }
            }
        }
        Ok(carry)
    }

    /// A carry committed: each page whose threads moved is announced once.
    pub fn carried(&self, carry: Carry) {
        for page in carry.pages {
            self.announce(page);
        }
    }

    /// The changes open pages should hear for `paths`, changed by another
    /// hand: a page's comments for each page whose threads changed. Nothing
    /// is published — the watcher publishes what it heard together.
    pub fn heard(&self, paths: &[PathBuf]) -> Vec<Change> {
        self.store
            .heard(paths)
            .into_iter()
            .map(Change::Comments)
            .collect()
    }

    /// The page thread `id` was on after `write`, or why there is none.
    fn found(
        &self,
        id: &str,
        write: impl FnOnce(&str) -> Result<Option<String>>,
    ) -> Result<String, Refused> {
        if !is_id(id) {
            return Err(Refused::NoThread(id.into()));
        }
        write(id)?.ok_or_else(|| Refused::NoThread(id.into()))
    }

    /// The HTML of `page`'s content as its page shows it: rendered with the
    /// widgets of the document that shows it — its owner, or else the
    /// version's entry document. `None` when the page is gone.
    fn content(&self, version: &Folder, modules: &Modules, page: &str) -> Option<String> {
        let file = self.folder.file_at(page)?;
        let source = std::fs::read_to_string(&file).ok()?;
        let site = version.owner(&file).and_then(|owner| owner.document.ok());
        let entry = version.entry();
        let widgets = Widgets::of(site.as_ref().unwrap_or(&entry), modules);
        Some(
            crate::page::content(&widgets, &file, source)
                .html()
                .to_string(),
        )
    }

    fn announce(&self, page: String) {
        self.changes.publish(Change::Comments(page));
    }
}

/// A page's key: its address with exactly one leading `/` — or a refusal
/// when it is empty or climbs out of the folder.
fn page_key(page: &str) -> Result<String, Refused> {
    let name = page.trim_start_matches('/');
    let plain = Path::new(name)
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)));
    if name.is_empty() || !plain {
        return Err(Refused::NotAPage(page.into()));
    }
    Ok(format!("/{name}"))
}

/// A message by `author`, written now.
fn message(author: &str, text: &str) -> Message {
    Message {
        author: author.into(),
        at: now().0,
        text: text.into(),
    }
}

/// A new thread's id: the UTC time it was started, to the second, and four
/// hex digits from the clock's nanoseconds — so a page's threads list oldest
/// first.
pub fn new_id() -> String {
    let (stamp, nanos) = now();
    format!("{}-{:04x}", stamp.replace(['-', ':'], ""), nanos & 0xffff)
}

/// Whether `id` can be a thread's: letters, digits and dashes, never empty.
pub fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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

    fn anchor() -> Anchor {
        Anchor {
            quote: "retries".into(),
            prefix: "The client ".into(),
            suffix: ".".into(),
        }
    }

    /// A served folder holding `files`, its comments in memory announcing
    /// on a recorder.
    fn folder(
        name: &str,
        files: &[(&str, &str)],
    ) -> (
        Comments,
        Arc<crate::changes::Recorder>,
        Arc<memory::Memory>,
        ServedFolder,
    ) {
        let root =
            std::env::temp_dir().join(format!("devpage-comments-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        let served = ServedFolder::new(&root).unwrap();
        let changes = Arc::new(crate::changes::Recorder::default());
        let store = Arc::new(memory::Memory::default());
        let comments = Comments::new(store.clone(), changes.clone(), served.clone());
        (comments, changes, store, served)
    }

    #[test]
    fn every_write_announces_its_page_once_and_a_failed_one_nothing() {
        let (comments, changes, _, _) = folder("announce", &[("a.md", "# A\n")]);
        let id = comments
            .create("a.md", anchor(), "Ana", "Every write?")
            .unwrap();
        comments.reply(&id, "Bo", "Only the outbox.").unwrap();
        comments.resolve(&id).unwrap();
        assert!(matches!(
            comments.reply(&id, "Bo", "gone"),
            Err(Refused::NoThread(_))
        ));
        assert!(matches!(
            comments.create("/../x.md", anchor(), "Ana", "no"),
            Err(Refused::NotAPage(_))
        ));
        let comments_of_a = Change::Comments("/a.md".into());
        assert_eq!(
            changes.drain(),
            [comments_of_a.clone(), comments_of_a.clone(), comments_of_a],
            "one per write that landed, keyed `/a.md` however it was named, none for the two refused"
        );
    }

    #[test]
    fn a_thread_starts_only_on_a_file_in_the_folder() {
        let (comments, changes, _, _) = folder(
            "create",
            &[
                ("a.md", "# A\n"),
                ("notes/lone.md", "# Lone\n"),
                ("docs/x", ""),
            ],
        );
        // A lone page no document links to is still a file in the folder.
        assert!(
            comments
                .create("/notes/lone.md", anchor(), "Ana", "ok")
                .is_ok()
        );
        for page in ["/missing.md", "/notes", "/../escape.md", "", "/"] {
            assert!(
                matches!(
                    comments.create(page, anchor(), "Ana", "no"),
                    Err(Refused::NotAPage(_))
                ),
                "{page}"
            );
        }
        assert_eq!(changes.drain(), [Change::Comments("/notes/lone.md".into())]);
    }

    #[test]
    fn a_reply_and_a_resolve_go_by_id_whatever_page_it_is_on() {
        let (comments, _, store, _) = folder("by-id", &[("a.md", "# A\n")]);
        let id = comments.create("/a.md", anchor(), "Ana", "x").unwrap();
        // Detached: its text is gone from the version shown.
        store.detach(&id);
        comments.reply(&id, "Bo", "still here").unwrap();
        let listed = comments.list("/a.md").unwrap();
        assert!(listed[0].detached);
        assert_eq!(listed[0].thread.thread[1].text, "still here");
        comments.resolve(&id).unwrap();
        assert!(comments.list("/a.md").unwrap().is_empty());
        assert!(matches!(
            comments.resolve("../x"),
            Err(Refused::NoThread(_))
        ));
    }

    #[test]
    fn carry_reads_the_page_as_it_is_shown() {
        let page = "---\ntitle: Launch\n---\n\n# Launch\n\n<Callout>\nThe client **retries** every write.\n</Callout>\n";
        let (comments, changes, store, served) = folder("carry", &[("a.md", page)]);
        let kept = comments
            .create(
                "/a.md",
                Anchor {
                    quote: "retries every write".into(),
                    prefix: "The client ".into(),
                    suffix: ".".into(),
                },
                "Ana",
                "kept",
            )
            .unwrap();
        let gone = comments
            .create(
                "/a.md",
                Anchor {
                    quote: "title: Launch".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                },
                "Ana",
                "front matter is never shown",
            )
            .unwrap();
        changes.drain();
        let entry = crate::document::Document::load(&served.path().join("a.md"), &served).unwrap();
        let carry = comments.carry(&Folder::new(Arc::new(entry))).unwrap();
        assert_eq!(
            carry.moved(),
            [Moved {
                id: gone,
                anchor: None
            }],
            "the widget's text is found as the page shows it; front matter never is"
        );
        assert!(
            store.list("/a.md").unwrap().iter().all(|t| !t.detached),
            "nothing written: the publish commits the carry"
        );
        let _ = kept;
        assert!(
            changes.drain().is_empty(),
            "nothing written, nothing announced"
        );
        comments.carried(carry);
        assert_eq!(changes.drain(), [Change::Comments("/a.md".into())]);
    }

    #[test]
    fn a_header_author_is_trimmed_and_a_blank_one_refused() {
        let deployed = Identity::Header("X-Forwarded-Email".into());
        for blank in [None, Some(""), Some("   ")] {
            assert!(
                matches!(deployed.author(|_| blank), Err(Refused::Unidentified(_))),
                "{blank:?}"
            );
        }
        assert_eq!(
            deployed
                .author(|name| (name == "X-Forwarded-Email").then_some(" ana@example.com "))
                .unwrap(),
            "ana@example.com"
        );
        let local = Identity::Local("Andrei".into());
        assert_eq!(local.author(|_| None).unwrap(), "Andrei");
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
        assert_eq!(page_key("docs/a.md").unwrap(), "/docs/a.md");
        assert!(page_key("/../x.md").is_err());
        assert!(is_id("20261025T181502Z-7f3a"));
        assert!(!is_id("../x") && !is_id(""));
    }
}
