//! The comment store: one YAML file per thread under `.devpage/comments/`, by
//! the path of the page it is about. devpage writes nothing outside `.devpage/`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
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

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(repo: &Path) -> Self {
        Self {
            root: repo.join(".devpage"),
        }
    }

    /// The folder holding a page's threads. `page` is a repository-relative
    /// path; anything climbing out of it is refused.
    fn folder(&self, page: &str) -> Result<PathBuf> {
        let page = Path::new(page.trim_start_matches('/'));
        if page
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            bail!("{} is not a page in this repository", page.display());
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

    pub fn list(&self, page: &str) -> Result<Vec<Listed>> {
        let Ok(entries) = std::fs::read_dir(self.folder(page)?) else {
            return Ok(Vec::new());
        };
        let mut listed = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "yml") {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            let thread =
                serde_yaml::from_str(&text).with_context(|| format!("in {}", path.display()))?;
            let id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            listed.push(Listed { id, thread });
        }
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(listed)
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

/// The local author: `git config user.name` in the repository, or the
/// system's user name when git has none.
pub fn local_author(repo: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("USER").ok())
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
    fn timestamps_are_rfc3339_utc() {
        let (stamp, _) = now();
        assert_eq!(stamp.len(), 20);
        assert!(
            stamp.ends_with('Z') && stamp.as_bytes()[10] == b'T',
            "{stamp}"
        );
    }
}
