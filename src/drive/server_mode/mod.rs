//! `devpage server`: documents published one at a time as zips, kept in the
//! bucket, recorded in the store, and served from the cache on the PVC —
//! each version through the same router a local devpage uses: the latest
//! mounted at `/d/<id>`, every version at `/d/<id>/v/<n>`.

mod cache;
#[cfg(test)]
mod comments_tests;
mod config;
#[cfg(test)]
mod pages_tests;
mod published;
mod routes;
#[cfg(test)]
mod tests;

use anyhow::Result;
use axum::Router;

pub use config::Config;

/// The `devpage server` router for `config`: its published documents —
/// the store, the bucket and the cache it names — answered over HTTP.
pub fn router(config: &Config) -> Result<Router> {
    let published = published::Published::open(config)?;
    Ok(routes::router(published, config.max_upload_bytes))
}
