//! Comments: threads readers leave on a page, kept by a comment store. The
//! [`Store`] port is what a store must do — list, create, reply, resolve —
//! and [`Comments`] is what the server writes through: every write it makes
//! announces itself on the change bus, so open pages hear it whatever keeps
//! the threads. The YAML files under `.devpage/comments/` are one store
//! ([`yaml`]); tests keep threads in memory ([`memory`]).

pub mod yaml;

#[cfg(test)]
pub mod memory;

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::changes::{Change, Changes, Scope};

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
    changes: Changes,
    scope: Scope,
}

impl Comments {
    pub fn new(store: Arc<dyn Store>, changes: Changes, scope: Scope) -> Self {
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
fn new_id() -> (String, String) {
    let (stamp, nanos) = now();
    let id = format!("{}-{:04x}", stamp.replace(['-', ':'], ""), nanos & 0xffff);
    (id, stamp)
}

/// Whether `id` can be a thread's: letters, digits and dashes, never empty.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Whether `page` stays inside the served folder: only plain path parts.
fn is_page(page: &str) -> bool {
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
        let changes = Changes::default();
        let mut heard = changes.subscribe(Scope::root());
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
            heard.drain(),
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

/// The page script's side of comments, in chrome-headless-shell. Run with
/// `cargo test -- --ignored`.
#[cfg(test)]
mod browser {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;

    use super::*;
    use crate::document::Document;
    use crate::server;

    /// Serves `entry` from `root`, opens `path` in a headless Chrome with a
    /// profile of its own and turns comment mode on, as a reader starting a
    /// comment does.
    async fn open(
        name: &str,
        root: &Path,
        entry: &Path,
        path: &str,
    ) -> (Browser, chromiumoxide::Page, tokio::task::JoinHandle<()>) {
        let opened = open_off(name, root, entry, path).await;
        eval::<bool>(
            &opened.1,
            "(document.querySelector('.comment-mode').click(), true)",
        )
        .await;
        opened
    }

    /// The same, with comment mode as a first visit finds it: off.
    async fn open_off(
        name: &str,
        root: &Path,
        entry: &Path,
        path: &str,
    ) -> (Browser, chromiumoxide::Page, tokio::task::JoinHandle<()>) {
        let document = Arc::new(Document::load(entry, root).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let changes = crate::changes::Changes::default();
        let index = Arc::new(crate::index::Index::build(&document.served));
        let entry = Arc::new(crate::document::Entry::new(document));
        let designs = crate::designs::Designs::new(entry.document().served.path().to_path_buf());
        let threads = Arc::new(yaml::Yaml::new(&entry.document().served));
        let _watcher = crate::watch::start(
            entry.clone(),
            index.clone(),
            designs.clone(),
            changes.clone(),
            threads.clone(),
        )
        .unwrap();
        let document = entry.document();
        let router = server::router(server::Options {
            entry,
            index,
            designs,
            comments: Some(server::Identity::Local("Ana".into())),
            store: threads,
            preview: crate::check::testing::preview(),
            ..server::Options::new(document, changes.clone())
        });
        tokio::spawn(async move {
            let _watcher = _watcher;
            axum::serve(listener, router).await.ok();
        });
        let profile = std::env::temp_dir().join(format!(
            "devpage-chrome-comments-{name}-{}",
            std::process::id()
        ));
        let config = BrowserConfig::builder()
            .chrome_executable(crate::pdf::headless_shell().await.unwrap())
            .user_data_dir(profile)
            .viewport(chromiumoxide::handler::viewport::Viewport {
                width: 1400,
                height: 900,
                ..Default::default()
            })
            .build()
            .unwrap();
        let (browser, mut handler) = Browser::launch(config).await.unwrap();
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(format!("http://{address}{path}").as_str())
            .await
            .unwrap();
        // Ready once the page and its scripts have loaded, however long a
        // loaded machine takes, rather than after a fixed wait.
        for _ in 0..200 {
            let ready = page
                .evaluate("document.readyState === 'complete' && !!document.getElementById('page')")
                .await
                .ok()
                .and_then(|v| v.into_value::<bool>().ok())
                .unwrap_or(false);
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        (browser, page, events)
    }

    async fn eval<T: serde::de::DeserializeOwned>(page: &chromiumoxide::Page, js: &str) -> T {
        page.evaluate(js).await.unwrap().into_value().unwrap()
    }

    /// Polls `js` until it returns true, for up to four seconds.
    async fn until(page: &chromiumoxide::Page, js: &str) -> bool {
        for _ in 0..40 {
            if eval::<bool>(page, js).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn folder(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("devpage-browser-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    #[ignore]
    async fn commenting_is_off_until_comment_mode_is_on_and_stays_on_after_a_reload() {
        let root = folder("mode");
        std::fs::write(root.join("a.md"), "# A\n\nA paragraph to comment on.\n").unwrap();
        let (mut browser, page, events) =
            open_off("mode", &root, &root.join("a.md"), "/a.md").await;
        let hover = "(document.querySelector('article p').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)";
        let pressed = "document.querySelector('.comment-mode').getAttribute('aria-pressed')";

        // Off: hovering offers nothing, and the switch is not pressed.
        eval::<bool>(&page, hover).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!eval::<bool>(&page, "!!document.querySelector('.comment-button')").await);
        assert_eq!(eval::<String>(&page, pressed).await, "false");

        // On: the icon appears beside the paragraph, and a comment can be made.
        eval::<bool>(
            &page,
            "(document.querySelector('.comment-mode').click(), true)",
        )
        .await;
        assert_eq!(eval::<String>(&page, pressed).await, "true");
        eval::<bool>(&page, "(document.querySelector('article h1').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)").await;
        eval::<bool>(&page, hover).await;
        assert!(until(&page, "!!document.querySelector('.comment-button')").await);
        eval::<bool>(
            &page,
            "(document.querySelector('.comment-button').click(), true)",
        )
        .await;
        assert!(
            until(
                &page,
                "!!document.querySelector('#devpage-comments .composer textarea')"
            )
            .await
        );
        eval::<bool>(
            &page,
            "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
               box.value = 'Is this the right paragraph?';
               box.closest('form').requestSubmit(); return true; })()",
        )
        .await;
        assert!(until(&page, "!!document.querySelector('mark.comment')").await);

        // Remembered: after a reload the switch is still on, and turning it off
        // removes the icon while the thread stays.
        page.reload().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(eval::<String>(&page, pressed).await, "true");
        eval::<bool>(
            &page,
            "(document.querySelector('.comment-mode').click(), true)",
        )
        .await;
        eval::<bool>(&page, hover).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!eval::<bool>(&page, "!!document.querySelector('.comment-button')").await);
        assert!(
            until(
                &page,
                "!!document.querySelector('#devpage-comments .thread')"
            )
            .await
        );

        browser.close().await.ok();
        events.abort();
    }

    #[tokio::test]
    #[ignore]
    async fn selected_text_becomes_a_thread_in_the_margin_and_in_the_store() {
        let root = folder("select");
        std::fs::write(
            root.join("a.md"),
            "# A\n\nThe client retries every write until the server acknowledges it.\n",
        )
        .unwrap();
        let (mut browser, page, events) = open("select", &root, &root.join("a.md"), "/a.md").await;

        // Select "retries every write" and release the mouse.
        eval::<bool>(
            &page,
            "(() => { const p = document.querySelector('article p'); const t = p.firstChild;
               const at = t.data.indexOf('retries every write');
               const r = document.createRange(); r.setStart(t, at); r.setEnd(t, at + 19);
               getSelection().removeAllRanges(); getSelection().addRange(r);
               p.dispatchEvent(new MouseEvent('mouseup', { bubbles: true })); return true; })()",
        )
        .await;
        assert!(until(&page, "!!document.querySelector('.comment-button')").await);
        eval::<bool>(
            &page,
            "document.querySelector('.comment-button').click(), true",
        )
        .await;
        assert!(
            until(
                &page,
                "!!document.querySelector('#devpage-comments .composer textarea')"
            )
            .await
        );
        eval::<bool>(
            &page,
            "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
               box.value = 'Every write, or only the outbox?';
               box.closest('form').requestSubmit(); return true; })()",
        )
        .await;
        assert!(
            until(
                &page,
                "document.querySelector('mark.comment')?.textContent === 'retries every write'"
            )
            .await
        );
        let layout: String = eval(
            &page,
            "innerWidth + ' ' + document.getElementById('devpage-comments').className
             + ' ' + !!document.querySelector('#devpage-comments .composer')",
        )
        .await;
        assert_eq!(
            layout, "1400 comment-ui beside false",
            "the thread sits beside the text and the composer closed"
        );

        let threads = yaml::Yaml::new(&root).list("/a.md").unwrap();
        assert_eq!(threads.len(), 1);
        let thread = &threads[0].thread;
        assert_eq!(thread.anchor.quote, "retries every write");
        assert_eq!(
            thread.anchor.prefix, "A The client ",
            "up to 32 characters of the text before, headings included"
        );
        assert_eq!(thread.anchor.suffix, " until the server acknowledges i");
        assert_eq!(thread.thread[0].author, "Ana");
        assert_eq!(thread.thread[0].text, "Every write, or only the outbox?");

        // Rewrite the sentence: the thread is detached, not lost.
        std::fs::write(root.join("a.md"), "# A\n\nThe client gives up.\n").unwrap();
        assert!(
            until(
                &page,
                "(document.getElementById('devpage-comments')?.textContent ?? '').includes('Detached comments')
                 && !document.querySelector('mark.comment')"
            )
            .await
        );
        browser.close().await.ok();
        events.abort();
    }

    #[tokio::test]
    #[ignore]
    async fn the_paragraph_comment_icon_stays_inside_narrow_windows() {
        use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
        let root = folder("narrow");
        std::fs::write(
            root.join("a.md"),
            "# A\n\nA paragraph long enough to reach the right edge of a narrow window, and then some more words.\n\n| a | b | c | d | e |\n| --- | --- | --- | --- | --- |\n| a_rather_long_cell_value | another_rather_long_cell | a_third_long_cell_value | yet_another_long_cell | the_last_long_cell_value |\n",
        )
        .unwrap();
        let (mut browser, page, events) = open("narrow", &root, &root.join("a.md"), "/a.md").await;
        for width in [390, 820] {
            page.execute(SetDeviceMetricsOverrideParams::new(width, 800, 1.0, false))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            // Hover the heading, then the paragraph, so the paragraph counts
            // as newly hovered at each width.
            eval::<bool>(
                &page,
                "(document.querySelector('article h1').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })),
                  document.querySelector('.comment-button')?.remove(),
                  document.querySelector('article p').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)",
            )
            .await;
            assert!(until(&page, "!!document.querySelector('.comment-button')").await);
            let fits: String = eval(
                &page,
                "(() => { const b = document.querySelector('.comment-button').getBoundingClientRect();
                   const w = document.documentElement.clientWidth;
                   return [b.left >= 0, b.right <= w, document.documentElement.scrollWidth <= w].join(' '); })()",
            )
            .await;
            assert_eq!(
                fits, "true true true",
                "at {width}px: icon left, icon right, no page scroll"
            );
        }
        browser.close().await.ok();
        events.abort();
    }

    #[tokio::test]
    #[ignore]
    async fn a_paragraph_comment_on_a_nested_list_item_anchors_again_after_a_reload() {
        let root = folder("paragraph");
        std::fs::write(
            root.join("a.md"),
            "# A\n\n- Writes\n  - retried   until acknowledged\n  - dropped after a day\n- Reads\n",
        )
        .unwrap();
        let (mut browser, page, events) =
            open("paragraph", &root, &root.join("a.md"), "/a.md").await;

        eval::<bool>(
            &page,
            "(document.querySelector('article li').dispatchEvent(new MouseEvent('mouseover', { bubbles: true })), true)",
        )
        .await;
        assert!(until(&page, "!!document.querySelector('.comment-button')").await);
        eval::<bool>(
            &page,
            "document.querySelector('.comment-button').click(), true",
        )
        .await;
        let quote: String = eval(
            &page,
            "document.querySelector('#devpage-comments .composer blockquote').textContent",
        )
        .await;
        assert_eq!(
            quote,
            "Writes retried until acknowledged dropped after a day"
        );
        eval::<bool>(
            &page,
            "(() => { const box = document.querySelector('#devpage-comments .composer textarea');
               box.value = 'Why a day?'; box.closest('form').requestSubmit(); return true; })()",
        )
        .await;
        assert!(
            until(
                &page,
                "document.querySelectorAll('mark.comment').length > 0"
            )
            .await
        );

        page.reload().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            until(
                &page,
                "[...document.querySelectorAll('mark.comment')].map(m => m.textContent).join('|')
                   .replace(/\\s+/g, ' ') .includes('retried until acknowledged')
                 && !(document.getElementById('devpage-comments')?.textContent ?? '').includes('Detached')"
            )
            .await
        );
        browser.close().await.ok();
        events.abort();
    }

    #[tokio::test]
    #[ignore]
    async fn on_a_deck_c_toggles_the_comments_panel() {
        let root = folder("deck");
        std::fs::write(
            root.join("devpage.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
        )
        .unwrap();
        std::fs::write(root.join("one.md"), "# One\n\nThe opening line.\n").unwrap();
        std::fs::write(root.join("two.md"), "# Two\n").unwrap();
        let anchor = Anchor {
            quote: "opening line".into(),
            prefix: "The ".into(),
            suffix: ".".into(),
        };
        yaml::Yaml::new(&root)
            .create("/one.md", anchor, "Bo", "Sharper?")
            .unwrap();
        let (mut browser, page, events) =
            open("deck", &root, &root.join("devpage.yml"), "/devpage.yml").await;

        let shown =
            "getComputedStyle(document.getElementById('devpage-comments')).display !== 'none'";
        assert!(until(&page, "!!document.querySelector('mark.comment')").await);
        assert!(!eval::<bool>(&page, shown).await, "hidden until asked for");
        eval::<bool>(
            &page,
            "(document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' })), true)",
        )
        .await;
        assert!(until(&page, shown).await);
        let text: String = eval(
            &page,
            "document.getElementById('devpage-comments').innerText",
        )
        .await;
        assert!(text.contains("Sharper?"), "{text}");
        eval::<bool>(
            &page,
            "(document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' })), true)",
        )
        .await;
        assert!(until(&page, &format!("!({shown})")).await);
        browser.close().await.ok();
        events.abort();
    }

    #[tokio::test]
    #[ignore]
    async fn a_live_update_with_the_comments_shown_swaps_the_page_in_place() {
        let root = folder("shown");
        std::fs::write(root.join("a.md"), "# A\n\nThe first draft.\n").unwrap();
        let (mut browser, page, events) = open("shown", &root, &root.join("a.md"), "/a.md").await;

        // Showing the comments marks the body; a fetched page never carries
        // that mark, and it must not make the update a full reload.
        eval::<bool>(
            &page,
            "(document.body.classList.add('show-comments'), window.notReloaded = true, true)",
        )
        .await;
        std::fs::write(root.join("a.md"), "# A\n\nThe second draft.\n").unwrap();
        assert!(
            until(
                &page,
                "document.querySelector('article p')?.textContent === 'The second draft.'"
            )
            .await
        );
        assert!(
            eval::<bool>(&page, "window.notReloaded === true").await,
            "the page was swapped in place, not reloaded"
        );
        assert!(
            eval::<bool>(&page, "document.body.classList.contains('show-comments')").await,
            "the comments stay shown"
        );
        browser.close().await.ok();
        events.abort();
    }
}
