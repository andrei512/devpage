//! The comment store as files: one YAML file per thread,
//! `.devpage/comments/<page>/<id>.yml`, under the served folder. It writes
//! nothing outside `.devpage/`, and finds a thread by its id through the
//! served folder, so a thread file that leads out of it is never read.
//!
//! It also remembers what each thread file held when open pages last heard
//! of it, so a thread file is announced by the watcher only when someone
//! other than this store changed it — a write through the store is
//! announced once, by [`super::Comments`], and a hand edit once, by the
//! watcher.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;

use super::{Anchor, Listed, Message, Store, Thread, new_id};
use crate::served::ServedFolder;

/// Thread files that don't parse, each with its error.
type Unparsable = Vec<(PathBuf, String)>;

pub struct Yaml {
    served: ServedFolder,
    /// Each thread file's bytes as open pages last heard of them — `None`
    /// once it is gone. Held while writing, so the watcher never reads a
    /// write before it is remembered.
    heard: Mutex<HashMap<PathBuf, Option<Vec<u8>>>>,
}

impl Yaml {
    pub fn new(served: &ServedFolder) -> Self {
        Self {
            served: served.clone(),
            heard: Mutex::default(),
        }
    }

    fn root(&self) -> PathBuf {
        self.served.path().join(".devpage")
    }

    fn comments(&self) -> PathBuf {
        self.root().join("comments")
    }

    /// The folder holding a page's threads.
    fn folder(&self, page: &str) -> PathBuf {
        self.comments().join(page.trim_start_matches('/'))
    }

    /// The page a thread file is about, from its path: `/<page>` for
    /// `.devpage/comments/<page>/<id>.yml` in the served folder.
    fn page_of(&self, file: &Path) -> Option<String> {
        let thread = self
            .served
            .relative(file)?
            .strip_prefix(".devpage/comments")
            .ok()?;
        if thread.extension().is_none_or(|ext| ext != "yml") {
            return None;
        }
        let page = thread
            .parent()
            .filter(|page| !page.as_os_str().is_empty())?;
        Some(format!("/{}", page.display()))
    }

    /// Thread `id`'s file and its page, found through the served folder.
    fn find(&self, id: &str) -> Option<(PathBuf, String)> {
        let name = format!("{id}.yml");
        thread_files(&self.comments())
            .into_iter()
            .filter(|file| file.file_name().is_some_and(|n| n == name.as_str()))
            .filter_map(|file| self.served.file(&file))
            .find_map(|file| self.page_of(&file).map(|page| (file, page)))
    }

    /// Creates `.devpage/` with a `.gitignore` that keeps git away from it.
    fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(self.root())?;
        let ignore = self.root().join(".gitignore");
        if !ignore.exists() {
            std::fs::write(ignore, "*\n")?;
        }
        Ok(())
    }

    /// The page's threads, and the thread files that don't parse with why.
    fn read(&self, page: &str) -> (Vec<Listed>, Unparsable) {
        let Ok(entries) = std::fs::read_dir(self.folder(page)) else {
            return Default::default();
        };
        let (mut listed, mut broken) = (Vec::new(), Vec::new());
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "yml") {
                continue;
            }
            match parse(&path) {
                Ok(thread) => {
                    let id = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    listed.push(Listed {
                        id,
                        thread,
                        detached: false,
                    });
                }
                Err(error) => broken.push((path, format!("{error:#}"))),
            }
        }
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        broken.sort();
        (listed, broken)
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
    fn unheard(&self, path: &Path) -> bool {
        let mut heard = self.heard.lock().unwrap_or_else(|p| p.into_inner());
        let now = std::fs::read(path).ok();
        if heard.get(path) == Some(&now) {
            return false;
        }
        heard.insert(path.to_path_buf(), now);
        true
    }
}

fn parse(file: &Path) -> Result<Thread> {
    Ok(serde_yaml::from_str(&std::fs::read_to_string(file)?)?)
}

/// Every `.yml` file under `folder`, not following symbolic links to folders.
fn thread_files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(path),
                _ if path.extension().is_some_and(|ext| ext == "yml") => found.push(path),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

impl Store for Yaml {
    /// A thread file that doesn't parse is left out — and left alone;
    /// [`problems`] names it.
    fn list(&self, page: &str) -> Result<Vec<Listed>> {
        Ok(self.read(page).0)
    }

    fn create(&self, page: &str, anchor: Anchor, first: Message) -> Result<String> {
        self.prepare()?;
        let id = new_id();
        let file = self.folder(page).join(format!("{id}.yml"));
        std::fs::create_dir_all(file.parent().unwrap())?;
        let thread = Thread {
            anchor,
            thread: vec![first],
        };
        self.write(file, &thread)?;
        Ok(id)
    }

    fn reply(&self, id: &str, message: Message) -> Result<Option<String>> {
        let Some((file, page)) = self.find(id) else {
            return Ok(None);
        };
        let mut thread = parse(&file)?;
        thread.thread.push(message);
        self.write(file, &thread)?;
        Ok(Some(page))
    }

    /// Resolving a thread deletes its file.
    fn resolve(&self, id: &str) -> Result<Option<String>> {
        let Some((file, page)) = self.find(id) else {
            return Ok(None);
        };
        let mut heard = self.heard.lock().unwrap_or_else(|p| p.into_inner());
        std::fs::remove_file(&file)?;
        heard.insert(file, None);
        Ok(Some(page))
    }

    /// Every thread that parses: a served folder's threads are never
    /// detached.
    fn attached(&self) -> Result<Vec<(String, Listed)>> {
        Ok(thread_files(&self.comments())
            .into_iter()
            .filter_map(|file| self.served.file(&file))
            .filter_map(|file| {
                let page = self.page_of(&file)?;
                let thread = parse(&file).ok()?;
                let id = file.file_stem()?.to_string_lossy().into_owned();
                Some((
                    page,
                    Listed {
                        id,
                        thread,
                        detached: false,
                    },
                ))
            })
            .collect())
    }

    fn heard(&self, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .filter_map(|path| self.page_of(path).filter(|_| self.unheard(path)))
            .collect()
    }
}

/// The page's thread files that don't parse, as problems: listed on the page
/// in the preview and reported by `devpage check`.
pub fn problems(document: &crate::document::Document, page: &Path) -> Vec<crate::problem::Problem> {
    let store = Yaml::new(&document.served);
    // A page names its threads by its path in the served folder, whatever
    // the folder's mount.
    let (_, unparsable) = store.read(&format!("/{}", document.name(page)));
    unparsable
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

    fn message(author: &str, text: &str) -> Message {
        Message {
            author: author.into(),
            at: "2026-10-25T18:15:02Z".into(),
            text: text.into(),
        }
    }

    /// A fresh served folder at a temporary path named for `name`.
    fn served(name: &str) -> ServedFolder {
        let root = std::env::temp_dir().join(format!("devpage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        ServedFolder::new(&root).unwrap()
    }

    #[test]
    fn a_thread_lives_and_dies_without_touching_the_repository() {
        let served = served("yaml-repo");
        let repo = served.path();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("docs/a.md"), "# A\n\nThe client retries.\n").unwrap();
        git(repo, &["init", "-q"]);
        git(repo, &["add", "."]);
        git(
            repo,
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

        let store = Yaml::new(&served);
        let id = store
            .create("/docs/a.md", anchor(), message("Ana", "Every write?"))
            .unwrap();
        assert_eq!(
            store
                .reply(&id, message("Bo", "Only the outbox."))
                .unwrap()
                .as_deref(),
            Some("/docs/a.md"),
            "found by its id alone"
        );
        let listed = store.list("/docs/a.md").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].thread.anchor, anchor());
        assert_eq!(listed[0].thread.thread[1].author, "Bo");
        assert!(
            repo.join(format!(".devpage/comments/docs/a.md/{id}.yml"))
                .is_file()
        );
        assert_eq!(
            git(repo, &["status", "--porcelain"]),
            "",
            "git sees nothing"
        );

        assert_eq!(store.resolve(&id).unwrap().as_deref(), Some("/docs/a.md"));
        assert!(store.list("/docs/a.md").unwrap().is_empty());
        assert_eq!(store.resolve(&id).unwrap(), None);
        assert_eq!(store.reply(&id, message("Bo", "gone")).unwrap(), None);
    }

    #[test]
    fn a_thread_file_reached_through_a_link_out_of_the_folder_is_never_found() {
        let served = served("yaml-link");
        let outside =
            std::env::temp_dir().join(format!("devpage-yaml-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let id = "20261025T000000Z-0000";
        std::fs::write(
            outside.join(format!("{id}.yml")),
            "anchor:\n  quote: x\nthread: []\n",
        )
        .unwrap();
        let folder = served.path().join(".devpage/comments/a.md");
        std::fs::create_dir_all(&folder).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            outside.join(format!("{id}.yml")),
            folder.join(format!("{id}.yml")),
        )
        .unwrap();
        let store = Yaml::new(&served);
        assert_eq!(store.reply(id, message("Bo", "out")).unwrap(), None);
        assert_eq!(store.resolve(id).unwrap(), None);
        assert!(outside.join(format!("{id}.yml")).is_file(), "left alone");
    }

    #[test]
    fn a_thread_file_is_the_yaml_it_always_was() {
        let served = served("yaml-format");
        let root = served.path();
        let store = Yaml::new(&served);
        let id = store
            .create("/a.md", anchor(), message("Ana", "Every write?"))
            .unwrap();
        store.reply(&id, message("Bo", "Only the outbox.")).unwrap();
        let file = root.join(format!(".devpage/comments/a.md/{id}.yml"));
        let text = std::fs::read_to_string(file).unwrap();
        assert_eq!(
            text,
            "anchor:\n  quote: retries\n  prefix: 'The client '\n  suffix: .\nthread:\n\
             - author: Ana\n  at: 2026-10-25T18:15:02Z\n  text: Every write?\n\
             - author: Bo\n  at: 2026-10-25T18:15:02Z\n  text: Only the outbox.\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".devpage/.gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn a_thread_file_that_doesnt_parse_is_named_and_left_alone() {
        let served = served("yaml-unparsable");
        let root = served.path();
        let store = Yaml::new(&served);
        store
            .create("/a.md", anchor(), message("Ana", "fine"))
            .unwrap();
        let bad = root.join(".devpage/comments/a.md/20261025T000000Z-0000.yml");
        std::fs::write(&bad, "anchor: [not, a, mapping]\n").unwrap();
        assert_eq!(
            store.list("/a.md").unwrap().len(),
            1,
            "the good thread still lists"
        );
        let broken = store.read("/a.md").1;
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].0, bad);
        assert!(bad.is_file(), "left alone");

        std::fs::write(root.join("a.md"), "# A\n").unwrap();
        let document = crate::document::Document::load(&root.join("a.md"), &served).unwrap();
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
    fn only_a_change_the_store_didnt_make_is_heard() {
        let served = served("yaml-heard");
        let root = served.path();
        let store = Yaml::new(&served);
        let id = store
            .create("/docs/a.md", anchor(), message("Ana", "Every write?"))
            .unwrap();
        let file = root.join(format!(".devpage/comments/docs/a.md/{id}.yml"));
        let heard = |path: &Path| store.heard(&[path.to_path_buf()]);
        let a = vec!["/docs/a.md".to_string()];
        assert!(heard(&file).is_empty(), "the store's own write");
        store.reply(&id, message("Bo", "Only the outbox.")).unwrap();
        assert!(heard(&file).is_empty(), "the store's own reply");

        // A hand edit is heard once; asked again, it has been heard.
        let edited =
            std::fs::read_to_string(&file).unwrap() + "- author: Cy\n  at: x\n  text: hi\n";
        std::fs::write(&file, &edited).unwrap();
        assert_eq!(heard(&file), a, "a hand edit");
        assert!(heard(&file).is_empty(), "the same edit, heard");

        // Editing it back to what the store wrote is a change pages haven't seen.
        store.reply(&id, message("Bo", "Again.")).unwrap();
        let written = std::fs::read(&file).unwrap();
        std::fs::write(&file, &edited).unwrap();
        assert_eq!(heard(&file), a);
        std::fs::write(&file, &written).unwrap();
        assert_eq!(heard(&file), a, "back to the store's bytes, after an edit");

        store.resolve(&id).unwrap();
        assert!(heard(&file).is_empty(), "the store's own delete");
        let other = root.join(".devpage/comments/docs/a.md/20261025T000000Z-0000.yml");
        std::fs::write(&other, "x").unwrap();
        std::fs::remove_file(&other).unwrap();
        assert_eq!(heard(&other), a, "a file the store never touched");

        // Only a thread file is one: not the page, nor another file there.
        for path in [
            root.join("docs/a.md"),
            root.join(".devpage/comments/docs/a.md/notes.txt"),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
            assert!(heard(&path).is_empty(), "{}", path.display());
        }
    }
}
