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
    /// The context commands use when none is chosen; `None` until `devpage
    /// context use` sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_context: Option<String>,
    /// Every context, by name.
    #[serde(default)]
    pub contexts: BTreeMap<String, Context>,
    /// Package defaults used only by the loopback serving command.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, devpage_core::extensions::Config>,
    /// The live session's settings on this machine. devpage reads `program`
    /// alone; the session's program reads the rest, which is kept as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<serde_yaml::Mapping>,
}

impl Config {
    /// `live.program`: the folder of the program devpage runs as the live
    /// session in place of the installed one, a leading `~` being the home
    /// folder; `None` when it isn't set.
    pub fn live_program(&self) -> Result<Option<PathBuf>> {
        let program = self.live.as_ref().and_then(|live| live.get("program"));
        let Some(program) = program.filter(|program| !program.is_null()) else {
            return Ok(None);
        };
        let Some(program) = program.as_str() else {
            bail!("live: program in ~/.devpage/config.yml is not a folder's path");
        };
        let folder = match program.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => std::env::home_dir()
                .ok_or_else(|| anyhow!("no home folder"))?
                .join(rest.trim_start_matches('/')),
            _ => PathBuf::from(program),
        };
        if !folder.is_absolute() {
            bail!("live: program in ~/.devpage/config.yml is not an absolute path: {program}");
        }
        Ok(Some(folder))
    }
}

/// A server, and how to get a token for the proxy in front of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Context {
    /// The server's base URL; a trailing `/` is ignored.
    pub server: String,
    /// A shell command that prints a bearer token for the proxy; `None` when
    /// the server needs none.
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

    /// `~/.devpage/config.yml`.
    fn config_file(&self) -> PathBuf {
        self.0.join("config.yml")
    }

    /// `~/.devpage/contexts/<context>/published.yml`.
    fn published_file(&self, context: &str) -> PathBuf {
        self.0.join("contexts").join(context).join("published.yml")
    }

    /// The contexts; none when the file doesn't exist yet.
    pub fn config(&self) -> Result<Config> {
        read_yaml(&self.config_file())
    }

    /// Writes `config` to `~/.devpage/config.yml`, making the folder if it is
    /// missing.
    pub fn save_config(&self, config: &Config) -> Result<()> {
        write_yaml(&self.config_file(), config)
    }

    /// Which document each published file is on the context `name`: the
    /// file's absolute path, and the document's id.
    pub fn published(&self, name: &str) -> Result<BTreeMap<PathBuf, String>> {
        read_yaml(&self.published_file(name))
    }

    /// Writes which document each published file is on the context `name`.
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

/// The YAML in `file`; the default when it is missing or empty.
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

/// Writes `value` to `file` as YAML, making its folder if it is missing.
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
    fn the_live_settings_survive_a_save_and_program_is_a_folder_with_the_home_expanded() {
        let home = home("live");
        assert_eq!(home.config().unwrap().live_program().unwrap(), None);
        std::fs::create_dir_all(&home.0).unwrap();
        std::fs::write(
            home.0.join("config.yml"),
            "live:\n  program: ~/code/live\n  agents:\n    slow: codex\n",
        )
        .unwrap();
        let mut config = home.config().unwrap();
        let folder = std::env::home_dir().unwrap().join("code/live");
        assert_eq!(config.live_program().unwrap(), Some(folder));
        config.current_context = Some("work".into());
        home.save_config(&config).unwrap();
        let text = std::fs::read_to_string(home.0.join("config.yml")).unwrap();
        assert!(text.contains("program: ~/code/live"), "{text}");
        assert!(text.contains("slow: codex"), "{text}");
        let program = |text: &str| {
            let config: Config = serde_yaml::from_str(text).unwrap();
            config.live_program()
        };
        assert_eq!(program("live:\n").unwrap(), None);
        assert_eq!(program("live: {agents: {}}").unwrap(), None);
        assert_eq!(
            program("live: {program: /opt/live}").unwrap(),
            Some(PathBuf::from("/opt/live"))
        );
        let error = program("live: {program: live}").unwrap_err().to_string();
        assert!(error.contains("not an absolute path: live"), "{error}");
        assert!(program("live: {program: [a]}").is_err());
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
