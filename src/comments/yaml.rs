//! The comment store as files: one YAML file per thread under
//! `.devpage/comments/`, by the path of the page it is about. It writes
//! nothing outside `.devpage/`.
//!
//! It also remembers what each thread file held when open pages last heard
//! of it, so the watcher announces a thread file only when someone other
//! than this store changed it — a write through the store is announced once,
//! by [`super::Comments`], and a hand edit once, by the watcher.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Result, bail};

use super::{Anchor, Listed, Message, Store, Thread, is_id, is_page, new_id, now};

/// Thread files that don't parse, each with its error.
type Unparsable = Vec<(PathBuf, String)>;

pub struct Yaml {
    root: PathBuf,
    /// Each thread file's bytes as open pages last heard of them — `None`
    /// once it is gone. Held while writing, so the watcher never reads a
    /// write before it is remembered.
    heard: Mutex<HashMap<PathBuf, Option<Vec<u8>>>>,
}

impl Yaml {
    pub fn new(served: impl AsRef<Path>) -> Self {
        Self {
            root: served.as_ref().join(".devpage"),
            heard: Mutex::default(),
        }
    }

    /// The folder holding a page's threads. `page` is a path from the top of
    /// the served folder; anything climbing out of it is refused.
    fn folder(&self, page: &str) -> Result<PathBuf> {
        if !is_page(page) {
            bail!(
                "{} is not a page in the served folder",
                page.trim_start_matches('/')
            );
        }
        Ok(self
            .root
            .join("comments")
            .join(page.trim_start_matches('/')))
    }

    fn thread_file(&self, page: &str, id: &str) -> Result<PathBuf> {
        if !is_id(id) {
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

    /// Writes `thread` to `file`, remembering it as heard.
    fn write(&self, file: PathBuf, thread: &Thread) -> Result<()> {
        let bytes = serde_yaml::to_string(thread)?.into_bytes();
        let mut heard = self.heard.lock().unwrap_or_else(|p| p.into_inner());
        std::fs::write(&file, &bytes)?;
        heard.insert(file, Some(bytes));
        Ok(())
    }

    /// Whether open pages have yet to hear of the thread file at `path` as it
    /// is now — true for a file someone else changed, false for one this
    /// store just wrote or deleted. Asking marks it heard: whoever asks
    /// announces it.
    pub fn unheard(&self, path: &Path) -> bool {
        let mut heard = self.heard.lock().unwrap_or_else(|p| p.into_inner());
        let now = std::fs::read(path).ok();
        if heard.get(path) == Some(&now) {
            return false;
        }
        heard.insert(path.to_path_buf(), now);
        true
    }
}

impl Store for Yaml {
    /// A thread file that doesn't parse is left out — and left alone;
    /// [`problems`] names it.
    fn list(&self, page: &str) -> Result<Vec<Listed>> {
        Ok(self.read(page)?.0)
    }

    fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String> {
        self.prepare()?;
        let (id, stamp) = new_id();
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
        self.write(file, &thread)?;
        Ok(id)
    }

    fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()> {
        let file = self.thread_file(page, id)?;
        let mut thread: Thread = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
        thread.thread.push(Message {
            author: author.into(),
            at: now().0,
            text: text.into(),
        });
        self.write(file, &thread)
    }

    /// Resolving a thread deletes its file.
    fn resolve(&self, page: &str, id: &str) -> Result<()> {
        let file = self.thread_file(page, id)?;
        let mut heard = self.heard.lock().unwrap_or_else(|p| p.into_inner());
        std::fs::remove_file(&file)?;
        heard.insert(file, None);
        Ok(())
    }
}

/// The page's thread files that don't parse, as problems: listed on the page
/// in the preview and reported by `devpage check`.
pub fn problems(document: &crate::document::Document, page: &Path) -> Vec<crate::problem::Problem> {
    let store = Yaml::new(&document.served);
    let unparsable = store.read(&document.url(page)).map(|(_, broken)| broken);
    unparsable
        .unwrap_or_default()
        .into_iter()
        .map(|(file, error)| crate::problem::Problem {
            file: document
                .served
                .relative(&file)
                .unwrap_or(&file)
                .display()
                .to_string(),
            line: None,
            message: format!("comment thread doesn't parse — {error}"),
            target: None,
        })
        .collect()
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

    fn anchor() -> Anchor {
        Anchor {
            quote: "retries".into(),
            prefix: "The client ".into(),
            suffix: ".".into(),
        }
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

        let store = Yaml::new(&repo);
        let id = store
            .create("/docs/a.md", anchor(), "Ana", "Every write?")
            .unwrap();
        store
            .reply("/docs/a.md", &id, "Bo", "Only the outbox.")
            .unwrap();
        let listed = store.list("/docs/a.md").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].thread.anchor, anchor());
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
        assert!(store.create("/../x.md", anchor(), "Ana", "no").is_err());
    }

    #[test]
    fn a_thread_file_is_the_yaml_it_always_was() {
        let root = std::env::temp_dir().join(format!("devpage-format-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Yaml::new(&root);
        let id = store
            .create("/a.md", anchor(), "Ana", "Every write?")
            .unwrap();
        store.reply("/a.md", &id, "Bo", "Only the outbox.").unwrap();
        let file = root.join(format!(".devpage/comments/a.md/{id}.yml"));
        let text = std::fs::read_to_string(file).unwrap();
        let at: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim_start_matches("- ").strip_prefix("  at: "))
            .collect();
        assert_eq!(
            text,
            format!(
                "anchor:\n  quote: retries\n  prefix: 'The client '\n  suffix: .\nthread:\n\
                 - author: Ana\n  at: {}\n  text: Every write?\n\
                 - author: Bo\n  at: {}\n  text: Only the outbox.\n",
                at[0], at[1]
            )
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".devpage/.gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn a_thread_file_that_doesnt_parse_is_named_and_left_alone() {
        let root = std::env::temp_dir().join(format!("devpage-unparsable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Yaml::new(&root);
        store.create("/a.md", anchor(), "Ana", "fine").unwrap();
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
        let found = problems(&document, &document.served.path().join("a.md"));
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
    fn only_a_change_the_store_didnt_make_is_unheard() {
        let root = std::env::temp_dir().join(format!("devpage-unheard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Yaml::new(&root);
        let id = store
            .create("/a.md", anchor(), "Ana", "Every write?")
            .unwrap();
        let file = root.join(format!(".devpage/comments/a.md/{id}.yml"));
        assert!(!store.unheard(&file), "the store's own write");
        store.reply("/a.md", &id, "Bo", "Only the outbox.").unwrap();
        assert!(!store.unheard(&file), "the store's own reply");

        // A hand edit is unheard once; asked again, it has been heard.
        let edited =
            std::fs::read_to_string(&file).unwrap() + "- author: Cy\n  at: x\n  text: hi\n";
        std::fs::write(&file, &edited).unwrap();
        assert!(store.unheard(&file), "a hand edit");
        assert!(!store.unheard(&file), "the same edit, heard");

        // Editing it back to what the store wrote is a change pages haven't seen.
        store.reply("/a.md", &id, "Bo", "Again.").unwrap();
        let written = std::fs::read(&file).unwrap();
        std::fs::write(&file, &edited).unwrap();
        assert!(store.unheard(&file));
        std::fs::write(&file, &written).unwrap();
        assert!(
            store.unheard(&file),
            "back to the store's bytes, after an edit"
        );

        store.resolve("/a.md", &id).unwrap();
        assert!(!store.unheard(&file), "the store's own delete");
        let other = root.join(".devpage/comments/a.md/20261025T000000Z-0000.yml");
        std::fs::write(&other, "x").unwrap();
        std::fs::remove_file(&other).unwrap();
        assert!(store.unheard(&other), "a file the store never touched");
    }
}
