//! Whether a newer devpage is out: the latest GitHub Release, asked at most
//! once a day, and the line that says how to update to it.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// Set to anything, devpage never asks whether a newer release is out.
pub const OFF: &str = "DEVPAGE_NO_UPDATE_CHECK";
/// Redirects to the latest release's tag, `…/releases/tag/v<version>`.
const LATEST: &str = "https://github.com/andrei512/devpage/releases/latest";
/// How long the latest version, kept in the cache folder, is believed.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// How long the release page has to answer.
const WAIT: Duration = Duration::from_secs(5);

/// The line to print when a newer release is out; `None` when it isn't, when
/// the check is off — by [`OFF`] or on CI — or when the network didn't answer.
pub async fn notice() -> Option<String> {
    if set(OFF) || set("CI") {
        return None;
    }
    let file = super::chrome::cache_folder().ok()?.join("latest-release");
    let latest = match fresh(&file, SystemTime::now()) {
        Some(latest) => latest,
        None => {
            let latest = ask().await?;
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(&file, &latest);
            latest
        }
    };
    let exe = std::env::current_exe().ok()?;
    line(
        env!("CARGO_PKG_VERSION"),
        &latest,
        &command(&exe, cargo_bin().as_deref()),
    )
}

/// Whether the variable `name` is set to something; empty counts as not set.
fn set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// The version kept in `file`, when it was written less than a [`DAY`]
/// before `now`.
fn fresh(file: &Path, now: SystemTime) -> Option<String> {
    let written = std::fs::metadata(file).ok()?.modified().ok()?;
    if now.duration_since(written).ok()? >= DAY {
        return None;
    }
    Some(std::fs::read_to_string(file).ok()?.trim().to_string())
}

/// The latest release's version, read from where [`LATEST`] redirects.
async fn ask() -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("devpage/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(WAIT)
        .build()
        .ok()?;
    let response = client.head(LATEST).send().await.ok()?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)?
        .to_str()
        .ok()?;
    version_in(location)
}

/// The version a release's address names: `…/tag/v0.3.3` → `0.3.3`.
fn version_in(location: &str) -> Option<String> {
    let tag = location.rsplit('/').next()?;
    let version = tag.strip_prefix('v')?;
    parsed(version)?;
    Some(version.to_string())
}

/// `major.minor.patch`, or `None` for anything else — a pre-release among them.
fn parsed(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(triple)
}

/// The line saying `latest` is out and how to update, when it is newer than
/// `current`.
fn line(current: &str, latest: &str, command: &str) -> Option<String> {
    (parsed(latest)? > parsed(current)?)
        .then(|| format!("devpage {latest} is out (you have {current}); update with: {command}"))
}

/// cargo's `bin` folder: `$CARGO_HOME/bin`, `~/.cargo/bin` by default.
fn cargo_bin() -> Option<std::path::PathBuf> {
    let home = match std::env::var_os("CARGO_HOME") {
        Some(home) => home.into(),
        None => std::env::home_dir()?.join(".cargo"),
    };
    Some(home.join("bin"))
}

/// The command that updates the devpage at `exe`: cargo's when cargo
/// installed it, in `cargo_bin`, otherwise the install script's.
fn command(exe: &Path, cargo_bin: Option<&Path>) -> String {
    if cargo_bin.is_some_and(|bin| exe.starts_with(bin)) {
        "cargo install devpage --locked".to_string()
    } else if cfg!(windows) {
        "powershell -ExecutionPolicy Bypass -c \"irm https://devpage.ai/install.ps1 | iex\""
            .to_string()
    } else {
        "curl -fsSL https://devpage.ai/install.sh | sh".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_the_tag_the_latest_release_redirects_to() {
        let location = "https://github.com/andrei512/devpage/releases/tag/v0.3.3";
        assert_eq!(version_in(location).as_deref(), Some("0.3.3"));
        assert_eq!(
            version_in("https://github.com/andrei512/devpage/releases"),
            None
        );
        assert_eq!(version_in(".../tag/v0.4.0-rc.1"), None);
    }

    #[test]
    fn only_a_newer_release_is_told() {
        let told = line("0.3.3", "0.10.0", "cargo install devpage --locked").unwrap();
        assert_eq!(
            told,
            "devpage 0.10.0 is out (you have 0.3.3); update with: cargo install devpage --locked"
        );
        assert_eq!(line("0.3.3", "0.3.3", "x"), None);
        assert_eq!(line("0.3.3", "0.3.2", "x"), None);
        assert_eq!(line("0.3.3", "nonsense", "x"), None);
    }

    #[test]
    fn a_cargo_install_is_updated_by_cargo_and_any_other_by_the_install_script() {
        let bin = Path::new("/home/me/.cargo/bin");
        assert_eq!(
            command(&bin.join("devpage"), Some(bin)),
            "cargo install devpage --locked"
        );
        let script = command(Path::new("/home/me/.local/bin/devpage"), Some(bin));
        assert!(script.contains("devpage.ai/install."), "{script}");
        assert!(command(&bin.join("devpage"), None).contains("devpage.ai/install."));
    }

    #[test]
    fn the_kept_version_is_believed_for_a_day() {
        let file = std::env::temp_dir().join(format!("devpage-latest-{}", std::process::id()));
        std::fs::write(&file, "0.3.3\n").unwrap();
        let now = SystemTime::now();
        assert_eq!(fresh(&file, now).as_deref(), Some("0.3.3"));
        assert_eq!(fresh(&file, now + DAY), None);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(fresh(&file, now), None);
    }
}
