//! Contexts: the servers the CLI talks to, as `kubectl` talks to clusters,
//! kept in `~/.devpage/config.yml`, and which document each published file
//! is on each of them, in `~/.devpage/contexts/<name>/published.yml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

/// `~/.devpage/config.yml`.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_context: Option<String>,
    #[serde(default)]
    pub contexts: BTreeMap<String, Context>,
}

/// A server, and how to get a token for the proxy in front of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Context {
    pub server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_command: Option<String>,
}

/// The folder the CLI keeps its files in: `~/.devpage`.
#[derive(Debug, Clone)]
pub struct Home(pub PathBuf);

impl Home {
    /// `~/.devpage`, in the user's home folder: `$HOME`, or the profile
    /// folder on Windows.
    pub fn from_env() -> Result<Home> {
        let home = std::env::home_dir().ok_or_else(|| anyhow!("no home folder"))?;
        Ok(Home(home.join(".devpage")))
    }

    fn config_file(&self) -> PathBuf {
        self.0.join("config.yml")
    }

    fn published_file(&self, context: &str) -> PathBuf {
        self.0.join("contexts").join(context).join("published.yml")
    }

    /// The contexts; none when the file doesn't exist yet.
    pub fn config(&self) -> Result<Config> {
        read_yaml(&self.config_file())
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        write_yaml(&self.config_file(), config)
    }

    /// Which document each published file is on the context `name`: the
    /// file's absolute path, and the document's id.
    pub fn published(&self, name: &str) -> Result<BTreeMap<PathBuf, String>> {
        read_yaml(&self.published_file(name))
    }

    pub fn save_published(&self, name: &str, published: &BTreeMap<PathBuf, String>) -> Result<()> {
        write_yaml(&self.published_file(name), published)
    }

    /// The context a command uses: `chosen` (`--context` or
    /// `DEVPAGE_CONTEXT`), or else the current one.
    pub fn context(&self, chosen: Option<&str>) -> Result<(String, Context)> {
        let config = self.config()?;
        let name = match chosen.or(config.current_context.as_deref()) {
            Some(name) => name.to_string(),
            None => bail!(
                "no context: add one with `devpage context add <name> <server>`, then `devpage context use <name>`"
            ),
        };
        let context = config
            .contexts
            .get(&name)
            .cloned()
            .ok_or_else(|| anyhow!("no context named {name}"))?;
        Ok((name, context))
    }
}

/// `command` run by the system's shell: `sh -c`, or `cmd /C` on Windows.
fn shell(command: &str) -> Command {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut shell = Command::new(shell);
    shell.arg(flag).arg(command);
    shell
}

impl Context {
    /// The bearer token for this context, from its token command, run
    /// before each command; `None` when it has none. devpage keeps no token.
    pub fn token(&self) -> Result<Option<String>> {
        let Some(command) = &self.token_command else {
            return Ok(None);
        };
        let output = shell(command)
            .output()
            .with_context(|| format!("running the token command `{command}`"))?;
        if !output.status.success() {
            bail!(
                "the token command `{command}` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let token = String::from_utf8(output.stdout)
            .context("the token command printed something that isn't text")?
            .trim()
            .to_string();
        if token.is_empty() {
            bail!("the token command `{command}` printed nothing");
        }
        Ok(Some(token))
    }
}

fn read_yaml<T: Default + for<'de> Deserialize<'de>>(file: &Path) -> Result<T> {
    match std::fs::read_to_string(file) {
        Ok(text) if text.trim().is_empty() => Ok(T::default()),
        Ok(text) => {
            serde_yaml::from_str(&text).with_context(|| format!("reading {}", file.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("reading {}", file.display())),
    }
}

fn write_yaml<T: Serialize>(file: &Path, value: &T) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_yaml::to_string(value)?;
    std::fs::write(file, text).with_context(|| format!("writing {}", file.display()))
}

/// An empty `~/.devpage` of its own, for a test.
#[cfg(test)]
pub fn test_home(name: &str) -> Home {
    let dir = std::env::temp_dir().join(format!("devpage-home-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Home(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> Home {
        test_home(name)
    }

    #[test]
    fn contexts_round_trip_through_config_yml_in_kebab_case() {
        let home = home("config");
        assert_eq!(home.config().unwrap(), Config::default());
        let mut config = Config::default();
        config.contexts.insert(
            "work".into(),
            Context {
                server: "https://docs.example.com".into(),
                token_command: Some("echo t".into()),
            },
        );
        config.current_context = Some("work".into());
        home.save_config(&config).unwrap();
        let text = std::fs::read_to_string(home.0.join("config.yml")).unwrap();
        assert!(text.contains("current-context: work"), "{text}");
        assert!(text.contains("token-command: echo t"), "{text}");
        assert_eq!(home.config().unwrap(), config);
        assert_eq!(home.context(None).unwrap().0, "work");
        assert!(home.context(Some("other")).is_err());
    }

    #[test]
    fn no_current_context_says_how_to_add_one() {
        let error = home("none").context(None).unwrap_err().to_string();
        assert!(error.contains("devpage context add"), "{error}");
    }

    #[test]
    fn the_token_is_what_the_command_prints_trimmed_and_a_failure_names_it() {
        let context = |command: &str| Context {
            server: "http://x".into(),
            token_command: Some(command.into()),
        };
        assert_eq!(
            context("printf ' abc\\n'").token().unwrap().as_deref(),
            Some("abc")
        );
        let error = context("echo nope >&2; exit 3")
            .token()
            .unwrap_err()
            .to_string();
        assert!(error.contains("nope"), "{error}");
        assert!(context("true").token().is_err());
        let none = Context {
            server: "http://x".into(),
            token_command: None,
        };
        assert_eq!(none.token().unwrap(), None);
    }

    #[test]
    fn published_files_are_kept_per_context() {
        let home = home("published");
        let mut work = BTreeMap::new();
        work.insert(PathBuf::from("/a/devpage.yml"), "id-1".to_string());
        home.save_published("work", &work).unwrap();
        assert_eq!(home.published("work").unwrap(), work);
        assert!(home.published("other").unwrap().is_empty());
        assert!(home.0.join("contexts/work/published.yml").is_file());
    }
}
