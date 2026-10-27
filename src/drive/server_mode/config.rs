//! What `devpage server` reads from the environment, checked before it
//! listens.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

/// What `devpage server` reads from the environment.
#[derive(Debug, PartialEq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bucket_url: String,
    pub cache_max_bytes: u64,
}

impl Config {
    /// The configuration in `vars`, or the first variable that is missing or
    /// malformed — named, never its value.
    pub fn from_vars(vars: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let required = |name: &str| vars(name).with_context(|| format!("{name} is not set"));
        let cache_max_bytes = match vars("DEVPAGE_CACHE_MAX_BYTES") {
            Some(text) => bytes(&text).context("DEVPAGE_CACHE_MAX_BYTES is not a size")?,
            // TODO: 80% of the volume under DEVPAGE_DATA_DIR.
            None => 1 << 30,
        };
        Ok(Config {
            data_dir: required("DEVPAGE_DATA_DIR")?.into(),
            bucket_url: required("DEVPAGE_BUCKET_URL")?,
            cache_max_bytes,
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
    Ok(number.parse::<u64>()? * scale)
}
