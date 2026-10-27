//! The cache on the PVC: versions unpacked from their zips, each with the
//! router that serves it, bounded by a limit past which the eviction policy
//! chooses what goes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::Router;
use devpage_core::cache::{Entry, Eviction};

/// Versions unpacked on the PVC, each with the router that serves it.
pub struct Cache {
    pub dir: PathBuf,
    pub limit: u64,
    pub policy: Box<dyn Eviction>,
    pub entries: Mutex<HashMap<(String, u64), Entry>>,
    pub routers: Mutex<HashMap<(String, u64), Router>>,
}

/// Past its limit, the cache deletes what its policy chooses — never `serving`.
pub fn evict(cache: &Cache, serving: &(String, u64)) {
    let entries: Vec<Entry> = cache.entries.lock().unwrap().values().cloned().collect();
    for key in cache.policy.choose(&entries, cache.limit) {
        if &key == serving {
            continue;
        }
        cache.routers.lock().unwrap().remove(&key);
        cache.entries.lock().unwrap().remove(&key);
        let _ = std::fs::remove_dir_all(cache.dir.join(&key.0).join(key.1.to_string()));
    }
}

pub fn size_of(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => size_of(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}
