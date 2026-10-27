//! `devpage server`: documents published one at a time as zips, kept in the
//! bucket, recorded in the store, and served from the cache on the PVC —
//! each version through the same router a local devpage uses, mounted at
//! `/d/<id>/v/<n>`.

mod api;
mod cache;
mod config;
#[cfg(test)]
mod tests;
mod versions;

use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use axum::routing::{get, post};
use devpage_core::cache::Lru;

pub use config::Config;

use crate::adapters::bucket::Bucket;
use crate::adapters::sqlite::Sqlite;
use crate::server;
use cache::Cache;

#[derive(Clone)]
struct ServerMode {
    store: Arc<Sqlite>,
    bucket: Bucket,
    cache: Arc<Cache>,
}

/// The `devpage server` router for `config`.
pub fn router(config: &Config) -> Result<Router> {
    std::fs::create_dir_all(config.data_dir.join("cache"))?;
    let state = ServerMode {
        store: Arc::new(Sqlite::open(&config.data_dir.join("store.db"))?),
        bucket: Bucket::open(&config.bucket_url)?,
        cache: Arc::new(Cache {
            dir: config.data_dir.join("cache").canonicalize()?,
            limit: config.cache_max_bytes,
            policy: Box::new(Lru),
            entries: Mutex::default(),
            routers: Mutex::default(),
        }),
    };
    Ok(Router::new()
        .route("/api/documents", post(api::create))
        .route("/api/documents/{id}/versions", post(api::add_version))
        .route("/d/{id}", get(versions::latest))
        .route("/d/{id}/v/{n}", get(versions::version_root))
        .route("/d/{id}/v/{n}/{*rest}", get(versions::version))
        .route("/__devpage/{asset}", get(server::asset))
        .fallback(versions::by_referer)
        .with_state(state))
}
