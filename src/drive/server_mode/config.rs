//! What `devpage server` reads from the environment, checked before it
//! listens: a variable that is missing or malformed, or a `DEVPAGE_*` one
//! server mode doesn't read, stops it, named and never its value.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use devpage_core::cache::{Eviction, Lru};

/// The `DEVPAGE_*` variables other commands read: present in a shell that
/// also runs `devpage server`, they are not a mistake.
const OTHER_COMMANDS: &[&str] = &[
    "DEVPAGE_CONTEXT",
    "DEVPAGE_OPENAI_API_KEY",
    "DEVPAGE_AZURE_OPENAI_ENDPOINT",
    "DEVPAGE_AZURE_OPENAI_API_KEY",
    "DEVPAGE_VIDEO_MODEL",
    "DEVPAGE_UPDATE_REFERENCES",
];

/// The variables server mode reads.
const SERVER: &[&str] = &[
    "DEVPAGE_HOST",
    "DEVPAGE_PORT",
    "DEVPAGE_BUCKET_URL",
    "DEVPAGE_DATA_DIR",
    "DEVPAGE_CACHE_MAX_BYTES",
    "DEVPAGE_CACHE_POLICY",
    "DEVPAGE_MAX_UPLOAD_BYTES",
    "DEVPAGE_USER_HEADER",
];

/// The share of the volume under the data folder the cache may fill when
/// `DEVPAGE_CACHE_MAX_BYTES` doesn't say.
const VOLUME_SHARE_PERCENT: u64 = 80;

/// What `devpage server` reads from the environment.
#[derive(Debug, PartialEq)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub data_dir: PathBuf,
    pub bucket_url: String,
    /// The cache's limit: `DEVPAGE_CACHE_MAX_BYTES`, or 80% of the volume.
    pub cache_max_bytes: u64,
    pub cache_policy: Policy,
    pub max_upload_bytes: u64,
    /// The header naming each write's author; none, and writes are
    /// `anonymous`.
    pub user_header: Option<String>,
}

/// The eviction policies `DEVPAGE_CACHE_POLICY` names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Policy {
    Lru,
}

impl Policy {
    pub fn eviction(self) -> Box<dyn Eviction> {
        match self {
            Policy::Lru => Box::new(Lru),
        }
    }
}

impl Config {
    /// The configuration in the process's environment.
    pub fn from_env() -> Result<Config> {
        Config::from_vars(std::env::vars(), volume_size)
    }

    /// The configuration in `vars`, with `volume` the size of the volume a
    /// folder is on — or the first variable that is missing, malformed or
    /// not server mode's.
    pub fn from_vars(
        vars: impl IntoIterator<Item = (String, String)>,
        volume: impl Fn(&Path) -> std::io::Result<u64>,
    ) -> Result<Config> {
        let vars: HashMap<String, String> = vars.into_iter().collect();
        let mut unknown: Vec<&String> = vars
            .keys()
            .filter(|name| name.starts_with("DEVPAGE_"))
            .filter(|name| !SERVER.contains(&name.as_str()))
            .filter(|name| !OTHER_COMMANDS.contains(&name.as_str()))
            .collect();
        unknown.sort();
        if let Some(name) = unknown.first() {
            bail!("{name} is not a variable devpage server reads");
        }
        let get = |name: &str| vars.get(name).map(|v| v.trim()).filter(|v| !v.is_empty());
        let required = |name: &str| get(name).with_context(|| format!("{name} is not set"));

        let host = match get("DEVPAGE_HOST") {
            Some(text) => text
                .parse()
                .ok()
                .context("DEVPAGE_HOST is not an IP address")?,
            None => IpAddr::from([127, 0, 0, 1]),
        };
        let port = match get("DEVPAGE_PORT") {
            Some(text) => text.parse().ok().context("DEVPAGE_PORT is not a port")?,
            None => 3030,
        };
        let bucket_url = required("DEVPAGE_BUCKET_URL")?.to_string();
        let scheme = bucket_url.split_once("://").map(|(scheme, _)| scheme);
        if !matches!(scheme, Some("s3" | "az" | "azure" | "file")) {
            bail!("DEVPAGE_BUCKET_URL is not an s3://, az:// or file:// URL");
        }
        let data_dir = PathBuf::from(required("DEVPAGE_DATA_DIR")?);
        if !data_dir.is_dir() {
            bail!("DEVPAGE_DATA_DIR is not a folder");
        }
        let cache_max_bytes = match get("DEVPAGE_CACHE_MAX_BYTES") {
            Some(text) => bytes(text)
                .ok()
                .filter(|&n| n > 0)
                .context("DEVPAGE_CACHE_MAX_BYTES is not a size")?,
            None => {
                let size = volume(&data_dir).context(
                    "DEVPAGE_CACHE_MAX_BYTES is not set, and the size of the volume \
                     under DEVPAGE_DATA_DIR can't be read",
                )?;
                size / 100 * VOLUME_SHARE_PERCENT
            }
        };
        let cache_policy = match get("DEVPAGE_CACHE_POLICY") {
            None | Some("lru") => Policy::Lru,
            Some(_) => bail!("DEVPAGE_CACHE_POLICY is not a policy: lru is the only one"),
        };
        let max_upload_bytes = match get("DEVPAGE_MAX_UPLOAD_BYTES") {
            Some(text) => bytes(text)
                .ok()
                .filter(|&n| n > 0)
                .context("DEVPAGE_MAX_UPLOAD_BYTES is not a size")?,
            None => 100 << 20,
        };
        let user_header = match get("DEVPAGE_USER_HEADER") {
            Some(name) => {
                axum::http::HeaderName::try_from(name)
                    .ok()
                    .context("DEVPAGE_USER_HEADER is not a header name")?;
                Some(name.to_string())
            }
            None => None,
        };
        Ok(Config {
            host,
            port,
            data_dir,
            bucket_url,
            cache_max_bytes,
            cache_policy,
            max_upload_bytes,
            user_header,
        })
    }
}

/// `8GB`, `512MB`, `100KB` or a number of bytes.
pub fn bytes(text: &str) -> Result<u64> {
    let text = text.trim();
    let (number, unit) = match text.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let scale: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "KB" => 1 << 10,
        "MB" => 1 << 20,
        "GB" => 1 << 30,
        "TB" => 1 << 40,
        _ => bail!("unknown unit"),
    };
    number
        .parse::<u64>()?
        .checked_mul(scale)
        .context("too large")
}

/// The size of the volume `folder` is on, in bytes.
#[cfg(unix)]
pub fn volume_size(folder: &Path) -> std::io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(folder.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid C string and `stat` a statvfs to fill.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    #[allow(clippy::unnecessary_cast)]
    Ok(stat.f_blocks as u64 * stat.f_frsize as u64)
}

/// Elsewhere the volume's size is unknown: `DEVPAGE_CACHE_MAX_BYTES` must say.
#[cfg(not(unix))]
pub fn volume_size(_: &Path) -> std::io::Result<u64> {
    Err(std::io::Error::other("unsupported"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn data_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn minimal(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let dir = data_dir();
        let mut all = vars(&[
            ("DEVPAGE_DATA_DIR", dir.as_str()),
            ("DEVPAGE_BUCKET_URL", "s3://docs/devpage"),
        ]);
        all.extend(vars(extra));
        all
    }

    fn gigabytes(n: u64) -> impl Fn(&Path) -> std::io::Result<u64> {
        move |_| Ok(n << 30)
    }

    #[test]
    fn the_defaults_fill_what_the_environment_leaves_out() {
        let config = Config::from_vars(minimal(&[]), gigabytes(10)).unwrap();
        assert_eq!(config.host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 3030);
        assert_eq!(config.cache_max_bytes, (10 << 30) / 100 * 80);
        assert_eq!(config.cache_policy, Policy::Lru);
        assert_eq!(config.max_upload_bytes, 100 << 20);
        assert_eq!(config.user_header, None);
    }

    #[test]
    fn every_variable_is_read() {
        let config = Config::from_vars(
            minimal(&[
                ("DEVPAGE_HOST", "0.0.0.0"),
                ("DEVPAGE_PORT", "8080"),
                ("DEVPAGE_CACHE_MAX_BYTES", "8GB"),
                ("DEVPAGE_CACHE_POLICY", "lru"),
                ("DEVPAGE_MAX_UPLOAD_BYTES", "5MB"),
                ("DEVPAGE_USER_HEADER", "X-Forwarded-Email"),
            ]),
            |_| panic!("the volume isn't asked when the limit is set"),
        )
        .unwrap();
        assert_eq!(config.host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 8080);
        assert_eq!(config.cache_max_bytes, 8 << 30);
        assert_eq!(config.max_upload_bytes, 5 << 20);
        assert_eq!(config.user_header.as_deref(), Some("X-Forwarded-Email"));
    }

    /// Each failure names its variable and never echoes the value, which may
    /// be a secret pasted into the wrong one.
    #[test]
    fn a_missing_malformed_or_foreign_variable_stops_the_server_by_name() {
        let secret = "s3cr3t-value";
        let cases: Vec<(Vec<(String, String)>, &str)> = vec![
            (
                vars(&[("DEVPAGE_BUCKET_URL", "s3://b/p")]),
                "DEVPAGE_DATA_DIR is not set",
            ),
            (
                vars(&[("DEVPAGE_DATA_DIR", &data_dir())]),
                "DEVPAGE_BUCKET_URL is not set",
            ),
            (
                minimal(&[("DEVPAGE_PORT", secret)]),
                "DEVPAGE_PORT is not a port",
            ),
            (
                minimal(&[("DEVPAGE_HOST", secret)]),
                "DEVPAGE_HOST is not an IP address",
            ),
            (
                minimal(&[("DEVPAGE_CACHE_MAX_BYTES", secret)]),
                "DEVPAGE_CACHE_MAX_BYTES is not a size",
            ),
            (
                minimal(&[("DEVPAGE_CACHE_MAX_BYTES", "0")]),
                "DEVPAGE_CACHE_MAX_BYTES is not a size",
            ),
            (
                minimal(&[("DEVPAGE_MAX_UPLOAD_BYTES", secret)]),
                "DEVPAGE_MAX_UPLOAD_BYTES is not a size",
            ),
            (
                minimal(&[("DEVPAGE_CACHE_POLICY", secret)]),
                "DEVPAGE_CACHE_POLICY is not a policy: lru is the only one",
            ),
            (
                minimal(&[("DEVPAGE_USER_HEADER", "not a header")]),
                "DEVPAGE_USER_HEADER is not a header name",
            ),
            (
                minimal(&[("DEVPAGE_BUKET_URL", secret)]),
                "DEVPAGE_BUKET_URL is not a variable devpage server reads",
            ),
            (
                vars(&[
                    ("DEVPAGE_DATA_DIR", &data_dir()),
                    ("DEVPAGE_BUCKET_URL", secret),
                ]),
                "DEVPAGE_BUCKET_URL is not an s3://, az:// or file:// URL",
            ),
            (
                vars(&[
                    ("DEVPAGE_DATA_DIR", "/no/such/folder"),
                    ("DEVPAGE_BUCKET_URL", "s3://b/p"),
                ]),
                "DEVPAGE_DATA_DIR is not a folder",
            ),
        ];
        for (vars, expected) in cases {
            let error = format!("{:#}", Config::from_vars(vars, gigabytes(1)).unwrap_err());
            assert_eq!(error, expected);
            assert!(!error.contains(secret));
        }
        // Without a limit, a volume whose size can't be read is a stop too.
        let error = Config::from_vars(minimal(&[]), |_| Err(std::io::Error::other("x")))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("DEVPAGE_CACHE_MAX_BYTES is not set"),
            "{error}"
        );
    }

    #[test]
    fn other_commands_variables_are_not_a_mistake() {
        let config = Config::from_vars(
            minimal(&[("DEVPAGE_CONTEXT", "work"), ("DEVPAGE_OPENAI_API_KEY", "k")]),
            gigabytes(1),
        );
        assert!(config.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn the_volume_under_a_folder_has_a_size() {
        assert!(volume_size(&std::env::temp_dir()).unwrap() > 0);
    }
}
