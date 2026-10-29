//! `devpage server`: documents published one at a time as zips, kept in the
//! bucket, recorded in the store, and served from the cache on the PVC —
//! each version through the same router a local devpage uses: the latest
//! mounted at `/d/<id>`, every version at `/d/<id>/v/<n>`.

mod cache;
#[cfg(test)]
mod comments_tests;
mod config;
pub mod metrics;
#[cfg(test)]
mod metrics_tests;
#[cfg(test)]
mod pages_tests;
mod published;
mod routes;
mod searches;
#[cfg(test)]
mod tests;

use anyhow::Result;
use axum::Router;

pub use config::Config;

/// The `devpage server` router for `config`.
#[cfg(test)]
pub fn router(config: &Config) -> Result<Router> {
    Ok(routers(config)?.0)
}

/// The `devpage server` router for `config` — its published documents, the
/// store, the bucket and the cache it names, answered over HTTP — and its
/// metrics port's router, which reports what that server holds.
pub fn routers(config: &Config) -> Result<(Router, Router)> {
    metrics::listen();
    let published = published::Published::open(config)?;
    let held = published.clone();
    let metrics = metrics::router(move || {
        let (versions, routers) = held.in_memory();
        metrics::Totals {
            versions,
            routers,
            open_pages: crate::drive::http::open_pages(),
            cache_bytes: held.cache_bytes(),
        }
    });
    Ok((routes::router(published, config.max_upload_bytes), metrics))
}
