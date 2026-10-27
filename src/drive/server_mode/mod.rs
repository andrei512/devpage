//! `devpage server`: documents published one at a time as zips, kept in the
//! bucket, recorded in the store, and served from the cache on the PVC —
//! each version through the same router a local devpage uses: the latest
//! mounted at `/d/<id>`, every version at `/d/<id>/v/<n>`.

mod api;
mod cache;
mod comments;
#[cfg(test)]
mod comments_tests;
mod config;
mod pages;
#[cfg(test)]
mod pages_tests;
#[cfg(test)]
mod tests;
mod versions;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{any, delete, get, post};

pub use config::Config;

use crate::adapters::bucket::Bucket;
use crate::adapters::sqlite::Sqlite;
use crate::changes::{Change, Changes, Scope};
use crate::server;
use cache::Cache;

#[derive(Clone)]
struct ServerMode {
    store: Arc<Sqlite>,
    bucket: Bucket,
    cache: Arc<Cache>,
    /// Where a publish unpacks its zip to check it, on the PVC.
    scratch: PathBuf,
    user_header: Option<String>,
    /// One lock per document: its publishes and deletions happen one at a
    /// time.
    publishing: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// Each document's change bus, shared by every router serving its latest
    /// version at `/d/<id>`: a publish tells all of them, so a page open
    /// there swaps in the new version whichever version it loaded; and a
    /// comment written through the API reaches those pages as one written on
    /// them does.
    buses: Arc<Mutex<HashMap<String, Changes>>>,
}

impl ServerMode {
    fn publishing(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.publishing
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    /// The change bus of document `id`'s latest version.
    fn bus(&self, id: &str) -> Changes {
        self.buses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    /// Tells every page open at `/d/<id>` that its latest version changed.
    fn announce(&self, id: &str) {
        self.bus(id).publish(&Scope::root(), Change::Refresh);
    }
}

/// The `devpage server` router for `config`.
pub fn router(config: &Config) -> Result<Router> {
    let scratch = config.data_dir.join("scratch");
    // What a publish that stopped halfway left.
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;
    let state = ServerMode {
        store: Arc::new(Sqlite::open(&config.data_dir.join("store.db"))?),
        bucket: Bucket::open(&config.bucket_url)?,
        cache: Arc::new(Cache::open(
            &config.data_dir.join("cache"),
            config.cache_max_bytes,
            config.cache_policy.eviction(),
        )?),
        scratch: scratch.canonicalize()?,
        user_header: config.user_header.clone(),
        publishing: Arc::default(),
        buses: Arc::default(),
    };
    let api = Router::new()
        .route("/api/documents", post(api::create).get(api::list))
        .route("/api/documents/{id}", get(api::get_one).delete(api::delete))
        .route("/api/documents/{id}/versions", post(api::add_version))
        .route(
            "/api/documents/{id}/versions/{n}",
            delete(api::delete_version),
        )
        .route(
            "/api/documents/{id}/comments",
            get(comments::list).post(comments::create),
        )
        .route(
            "/api/documents/{id}/comments/{thread}",
            delete(comments::resolve),
        )
        .route(
            "/api/documents/{id}/comments/{thread}/replies",
            post(comments::reply),
        )
        .layer(DefaultBodyLimit::max(
            usize::try_from(config.max_upload_bytes).unwrap_or(usize::MAX),
        ));
    Ok(Router::new()
        .merge(api)
        .route("/", get(pages::front))
        .route("/d/{id}", get(versions::to_folder))
        .route("/d/{id}/", any(versions::latest_root))
        .route("/d/{id}/{*rest}", any(versions::in_document))
        .route("/__devpage/{asset}", get(server::asset))
        .with_state(state))
}
