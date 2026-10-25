//! The comment store: one YAML file per thread under `.devpage/comments/`, by
//! the path of the page it is about. The server writes nothing outside `.devpage/`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Serialize)]
pub struct Listed {
    pub id: String,
    #[serde(flatten)]
    pub thread: Thread,
}

/// Thread files that don't parse, each with its error.
type Unparsable = Vec<(PathBuf, String)>;

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(served: &Path) -> Self {
        Self {
            root: served.join(".devpage"),
        }
    }

    /// The folder holding a page's threads. `page` is a path from the top of
    /// the served folder; anything climbing out of it is refused.
    fn folder(&self, page: &str) -> Result<PathBuf> {
        let page = Path::new(page.trim_start_matches('/'));
        if page
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            bail!("{} is not a page in the served folder", page.display());
        }
        Ok(self.root.join("comments").join(page))
    }

    fn thread_file(&self, page: &str, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            bail!("no thread {id}");
        }
        Ok(self.folder(page)?.join(format!("{id}.yml")))
    }

    /// Creates `.devpage/` with a `.gitignore` that keeps git away from it.
    fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let ignore = self.root.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(ignore, "*\n")?;
        }
        Ok(())
    }

    /// A page's threads, oldest first. A thread file that doesn't parse is
    /// left out — and left alone; [`problems`] names it.
    pub fn list(&self, page: &str) -> Result<Vec<Listed>> {
        Ok(self.read(page)?.0)
    }

    /// The page's threads, and the thread files that don't parse with why.
    fn read(&self, page: &str) -> Result<(Vec<Listed>, Unparsable)> {
        let Ok(entries) = std::fs::read_dir(self.folder(page)?) else {
            return Ok(Default::default());
        };
        let (mut listed, mut broken) = (Vec::new(), Vec::new());
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "yml") {
                continue;
            }
            let parsed = std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|text| Ok(serde_yaml::from_str::<Thread>(&text)?));
            match parsed {
                Ok(thread) => {
                    let id = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    listed.push(Listed { id, thread });
                }
                Err(error) => broken.push((path, format!("{error:#}"))),
            }
        }
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        broken.sort();
        Ok((listed, broken))
    }

    pub fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String> {
        self.prepare()?;
        let (stamp, nanos) = now();
        let id = format!("{}-{:04x}", stamp.replace(['-', ':'], ""), nanos & 0xffff);
        let file = self.thread_file(page, &id)?;
        std::fs::create_dir_all(file.parent().unwrap())?;
        let thread = Thread {
            anchor,
            thread: vec![Message {
                author: author.into(),
                at: stamp,
                text: text.into(),
            }],
        };
        std::fs::write(file, serde_yaml::to_string(&thread)?)?;
        Ok(id)
    }

    pub fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()> {
        let file = self.thread_file(page, id)?;
        let mut thread: Thread = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
        thread.thread.push(Message {
            author: author.into(),
            at: now().0,
            text: text.into(),
        });
        std::fs::write(file, serde_yaml::to_string(&thread)?)?;
        Ok(())
    }

    /// Resolving a thread deletes its file.
    pub fn resolve(&self, page: &str, id: &str) -> Result<()> {
        std::fs::remove_file(self.thread_file(page, id)?)?;
        Ok(())
    }
}

/// The page's thread files that don't parse, as problems: listed on the page
/// in the preview and reported by `devpage check`.
pub fn problems(document: &crate::document::Document, page: &Path) -> Vec<crate::check::Problem> {
    let store = Store::new(&document.served);
    let unparsable = store.read(&document.url(page)).map(|(_, broken)| broken);
    unparsable
        .unwrap_or_default()
        .into_iter()
        .map(|(file, error)| crate::check::Problem {
            file: file
                .strip_prefix(&document.served)
                .unwrap_or(&file)
                .display()
                .to_string(),
            line: None,
            message: format!("comment thread doesn't parse — {error}"),
            target: None,
        })
        .collect()
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

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn a_thread_lives_and_dies_without_touching_the_repository() {
        let repo = std::env::temp_dir().join(format!("devpage-comments-{}", std::process::id()));
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("docs/a.md"), "# A\n\nThe client retries.\n").unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "x",
            ],
        );

        let store = Store::new(&repo);
        let anchor = Anchor {
            quote: "retries".into(),
            prefix: "The client ".into(),
            suffix: ".".into(),
        };
        let id = store
            .create("/docs/a.md", anchor.clone(), "Ana", "Every write?")
            .unwrap();
        store
            .reply("/docs/a.md", &id, "Bo", "Only the outbox.")
            .unwrap();
        let listed = store.list("/docs/a.md").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].thread.anchor, anchor);
        assert_eq!(listed[0].thread.thread[1].author, "Bo");
        assert!(
            repo.join(format!(".devpage/comments/docs/a.md/{id}.yml"))
                .is_file()
        );
        assert_eq!(
            git(&repo, &["status", "--porcelain"]),
            "",
            "git sees nothing"
        );

        store.resolve("/docs/a.md", &id).unwrap();
        assert!(store.list("/docs/a.md").unwrap().is_empty());
        assert!(store.create("/../x.md", anchor, "Ana", "no").is_err());
    }

    #[test]
    fn a_thread_file_that_doesnt_parse_is_named_and_left_alone() {
        let root = std::env::temp_dir().join(format!("devpage-unparsable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Store::new(&root);
        let anchor = Anchor {
            quote: "x".into(),
            prefix: String::new(),
            suffix: String::new(),
        };
        store.create("/a.md", anchor, "Ana", "fine").unwrap();
        let bad = root.join(".devpage/comments/a.md/20261025T000000Z-0000.yml");
        std::fs::write(&bad, "anchor: [not, a, mapping]\n").unwrap();
        assert_eq!(
            store.list("/a.md").unwrap().len(),
            1,
            "the good thread still lists"
        );
        let broken = store.read("/a.md").unwrap().1;
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, bad);
        assert!(bad.is_file(), "left alone");

        std::fs::write(root.join("a.md"), "# A\n").unwrap();
        let document = crate::document::Document::load(&root.join("a.md"), &root).unwrap();
        let found = problems(&document, &document.served.join("a.md"));
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].file,
            ".devpage/comments/a.md/20261025T000000Z-0000.yml"
        );
        assert!(
            found[0].message.starts_with("comment thread doesn't parse"),
            "{}",
            found[0]
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
}

/// The page script's side of comments, in chrome-headless-shell. Run with
/// `cargo test -- --ignored`.
#[cfg(test)]
mod browser {
    use std::sync::Arc;
    use std::time::Duration;

    use chromiumoxide::browser::{Browser, BrowserConfig};
    use futures::StreamExt;
    use tokio::sync::broadcast;

    use super::*;
    use crate::document::Document;
    use crate::server;

    /// Serves `entry` from `root` and opens `path` in a headless Chrome with a
    /// profile of its own.
    async fn open(
        name: &str,
        root: &Path,
        entry: &Path,
        path: &str,
    ) -> (Browser, chromiumoxide::Page, tokio::task::JoinHandle<()>) {
        let document = Arc::new(Document::load(entry, root).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (changes, _) = broadcast::channel(16);
        let index = Arc::new(crate::index::Index::build(&document.served));
        let entry = Arc::new(crate::document::Entry::new(document));
        let designs = crate::designs::Designs::new(entry.document().served.clone());
        let _watcher = crate::watch::start(
            entry.clone(),
            index.clone(),
            designs.clone(),
            changes.clone(),
        )
        .unwrap();
        let router = server::router_with_index(
            entry,
            index,
            designs,
            changes,
            server::Identity::Local("Ana".into()),
        );
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
                "!!document.querySelector('#comments .composer textarea')"
            )
            .await
        );
        eval::<bool>(
            &page,
            "(() => { const box = document.querySelector('#comments .composer textarea');
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
            "innerWidth + ' ' + document.getElementById('comments').className
             + ' ' + !!document.querySelector('#comments .composer')",
        )
        .await;
        assert_eq!(
            layout, "1400 comment-ui beside false",
            "the thread sits beside the text and the composer closed"
        );

        let threads = Store::new(&root).list("/a.md").unwrap();
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
                "(document.getElementById('comments')?.textContent ?? '').includes('Detached comments')
                 && !document.querySelector('mark.comment')"
            )
            .await
        );
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
            "document.querySelector('#comments .composer blockquote').textContent",
        )
        .await;
        assert_eq!(
            quote,
            "Writes retried until acknowledged dropped after a day"
        );
        eval::<bool>(
            &page,
            "(() => { const box = document.querySelector('#comments .composer textarea');
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
                 && !(document.getElementById('comments')?.textContent ?? '').includes('Detached')"
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
        Store::new(&root)
            .create("/one.md", anchor, "Bo", "Sharper?")
            .unwrap();
        let (mut browser, page, events) =
            open("deck", &root, &root.join("devpage.yml"), "/devpage.yml").await;

        let shown = "getComputedStyle(document.getElementById('comments')).display !== 'none'";
        assert!(until(&page, "!!document.querySelector('mark.comment')").await);
        assert!(!eval::<bool>(&page, shown).await, "hidden until asked for");
        eval::<bool>(
            &page,
            "(document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' })), true)",
        )
        .await;
        assert!(until(&page, shown).await);
        let text: String = eval(&page, "document.getElementById('comments').innerText").await;
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
