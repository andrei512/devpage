//! The cache on the PVC: versions unpacked from their zips, each with the
//! router that serves it and the downloads rendered from it, bounded by a limit past which the eviction policy
//! chooses what goes. Everything in it is a copy of a version in the bucket,
//! and a version never changes, so nothing in it is ever stale.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use anyhow::{Context, Result};
use axum::Router;
use devpage_core::cache::{Entry, Eviction};

use super::api::unpack;
use super::metrics::{self, Stage};

/// A version: its document's id and its number.
pub type Key = (String, u64);

pub struct Cache {
    dir: PathBuf,
    limit: u64,
    policy: Box<dyn Eviction>,
    state: Mutex<State>,
    /// One lock per version being made, so misses arriving together unpack
    /// it once.
    making: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
}

#[derive(Default)]
struct State {
    /// Every version on disk, with when it was last read.
    entries: HashMap<Key, Entry>,
    /// The router of each version read since the server started, by the
    /// mount it serves it under: a version is the latest at `/d/<id>` and
    /// itself at `/d/<id>/v/<n>`.
    routers: HashMap<(Key, String), Router>,
    /// How many requests each version is answering now: never evicted.
    serving: HashMap<Key, usize>,
}

/// A version's download beside its folder: `<n>.download.<extension>`, so it
/// is counted with the version and goes with it.
fn download_of(name: &str) -> Option<u64> {
    if name.ends_with(".partial") {
        return None;
    }
    name.split_once(".download.")?.0.parse().ok()
}

/// A version's router, held while it answers one request.
pub struct Served {
    pub router: Router,
    /// The version's unpacked files.
    pub folder: PathBuf,
    cache: Arc<Cache>,
    key: Key,
    /// A version bigger than the cache's limit, unpacked for this request
    /// only and deleted after it.
    uncached: Option<PathBuf>,
}

impl Drop for Served {
    fn drop(&mut self) {
        match &self.uncached {
            Some(dir) => {
                let _ = std::fs::remove_dir_all(dir);
            }
            None => {
                let mut state = self.cache.state();
                if let Some(count) = state.serving.get_mut(&self.key) {
                    *count -= 1;
                    if *count == 0 {
                        state.serving.remove(&self.key);
                    }
                }
            }
        }
    }
}

impl Cache {
    /// The cache in `dir`, which it creates: what an earlier run left there
    /// is kept, each version's recency seeded from when it was unpacked, and
    /// anything half-unpacked is deleted.
    pub fn open(dir: &Path, limit: u64, policy: Box<dyn Eviction>) -> Result<Cache> {
        std::fs::create_dir_all(dir).context("the cache's folder")?;
        let dir = dir.canonicalize()?;
        let mut entries = HashMap::new();
        let mut downloads = Vec::new();
        for document in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            if !document.path().is_dir() {
                continue;
            }
            let id = document.file_name().to_string_lossy().into_owned();
            for version in std::fs::read_dir(document.path())?.filter_map(|e| e.ok()) {
                let name = version.file_name().to_string_lossy().into_owned();
                match name.parse::<u64>() {
                    Ok(n) if version.path().is_dir() => {
                        let read = version
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(SystemTime::UNIX_EPOCH);
                        entries.insert(
                            (id.clone(), n),
                            Entry {
                                document: id.clone(),
                                version: n,
                                bytes: size_of(&version.path()),
                                read,
                            },
                        );
                    }
                    _ if download_of(&name).is_some() && version.path().is_file() => {
                        downloads.push((id.clone(), name, version.path()));
                    }
                    _ => {
                        let _ = std::fs::remove_dir_all(version.path());
                        let _ = std::fs::remove_file(version.path());
                    }
                }
            }
        }
        // A download counts with its version, and without one is left over.
        for (id, name, file) in downloads {
            let key = (id, download_of(&name).unwrap_or_default());
            match entries.get_mut(&key) {
                Some(entry) => {
                    entry.bytes += std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
                }
                None => {
                    let _ = std::fs::remove_file(&file);
                }
            }
        }
        let cache = Cache {
            dir,
            limit,
            policy,
            state: Mutex::new(State {
                entries,
                ..State::default()
            }),
            making: Mutex::default(),
        };
        let versions = cache.state().entries.len();
        let size = cache.size();
        eprintln!(
            "cache: {versions} versions, {size} of {} bytes",
            cache.limit
        );
        Ok(cache)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn folder(&self, key: &Key) -> PathBuf {
        self.dir.join(&key.0).join(key.1.to_string())
    }

    /// The bytes every version on disk takes.
    pub fn size(&self) -> u64 {
        self.state().entries.values().map(|e| e.bytes).sum()
    }

    /// How many versions have a router in memory, and how many routers they
    /// have: one per mount each was read under.
    pub fn in_memory(&self) -> (usize, usize) {
        let state = self.state();
        let versions: std::collections::HashSet<&Key> =
            state.routers.keys().map(|(key, _)| key).collect();
        (versions.len(), state.routers.len())
    }

    /// Whether version `key` is on disk.
    #[cfg(test)]
    pub fn holds(&self, key: &Key) -> bool {
        self.state().entries.contains_key(key)
    }

    /// The router of version `key` under `mount`: kept, made from what is on
    /// disk, or — on a miss — made from the zip `fetch` gives, unpacked into
    /// the cache. `make` builds a router from a version's unpacked folder.
    pub async fn router<F>(
        self: &Arc<Self>,
        key: &Key,
        mount: &str,
        fetch: F,
        make: impl Fn(&Path) -> Result<Router>,
    ) -> Result<Served>
    where
        F: Future<Output = Result<Vec<u8>>>,
    {
        if let Some(served) = self.kept(key, mount) {
            return Ok(served);
        }
        let lock = self
            .making
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(key.clone())
            .or_default()
            .clone();
        let _making = lock.lock().await;
        let served = self.make(key, mount, fetch, make).await;
        self.making
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        served
    }

    /// A version already served since start: a hit.
    fn kept(self: &Arc<Self>, key: &Key, mount: &str) -> Option<Served> {
        let mut state = self.state();
        let router = state
            .routers
            .get(&(key.clone(), mount.to_string()))?
            .clone();
        if let Some(entry) = state.entries.get_mut(key) {
            entry.read = SystemTime::now();
        }
        *state.serving.entry(key.clone()).or_default() += 1;
        metrics::count(metrics::Cache::Hit);
        Some(self.served(router, key))
    }

    fn served(self: &Arc<Self>, router: Router, key: &Key) -> Served {
        Served {
            router,
            folder: self.folder(key),
            cache: self.clone(),
            key: key.clone(),
            uncached: None,
        }
    }

    async fn make<F>(
        self: &Arc<Self>,
        key: &Key,
        mount: &str,
        fetch: F,
        make: impl Fn(&Path) -> Result<Router>,
    ) -> Result<Served>
    where
        F: Future<Output = Result<Vec<u8>>>,
    {
        // Another request made it while this one waited.
        if let Some(served) = self.kept(key, mount) {
            return Ok(served);
        }
        let folder = self.folder(key);
        let on_disk = self.state().entries.contains_key(key) && folder.is_dir();
        if on_disk {
            eprintln!("cache: hit {} v{} on disk", key.0, key.1);
            metrics::count(metrics::Cache::Hit);
        } else {
            eprintln!("cache: miss {} v{}", key.0, key.1);
            metrics::count(metrics::Cache::Miss);
            let zip = fetch.await?;
            let partial = self.dir.join(&key.0).join(format!(
                "{}.partial-{}",
                key.1,
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&partial)?;
            if let Err(error) = metrics::time(Stage::Unpack, || unpack(&zip, &partial)) {
                let _ = std::fs::remove_dir_all(&partial);
                return Err(error);
            }
            let bytes = size_of(&partial);
            if bytes > self.limit {
                eprintln!(
                    "cache: {} v{} is {bytes} bytes, over the limit of {}: served uncached",
                    key.0, key.1, self.limit
                );
                let router = make(&partial.canonicalize()?);
                return match router {
                    Ok(router) => Ok(Served {
                        router,
                        folder: partial.clone(),
                        cache: self.clone(),
                        key: key.clone(),
                        uncached: Some(partial),
                    }),
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(&partial);
                        Err(error)
                    }
                };
            }
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::rename(&partial, &folder)?;
            self.state().entries.insert(
                key.clone(),
                Entry {
                    document: key.0.clone(),
                    version: key.1,
                    bytes,
                    read: SystemTime::now(),
                },
            );
        }
        let router = make(&folder.canonicalize()?)?;
        {
            let mut state = self.state();
            state
                .routers
                .insert((key.clone(), mount.to_string()), router.clone());
            if let Some(entry) = state.entries.get_mut(key) {
                entry.read = SystemTime::now();
            }
            *state.serving.entry(key.clone()).or_default() += 1;
        }
        self.evict();
        Ok(self.served(router, key))
    }

    /// Past its limit, the cache deletes what its policy chooses, skipping
    /// any version answering a request now.
    fn evict(&self) {
        let mut state = self.state();
        let entries: Vec<Entry> = state.entries.values().cloned().collect();
        for key in self.policy.choose(&entries, self.limit) {
            if state.serving.contains_key(&key) {
                continue;
            }
            state.routers.retain(|(routed, _), _| *routed != key);
            if let Some(entry) = state.entries.remove(&key) {
                self.delete(&key);
                metrics::count(metrics::Cache::Eviction);
                eprintln!(
                    "cache: evicted {} v{} ({} bytes)",
                    key.0, key.1, entry.bytes
                );
            }
        }
        let size: u64 = state.entries.values().map(|e| e.bytes).sum();
        eprintln!("cache: {size} of {} bytes", self.limit);
    }

    /// Forgets version `key` and deletes it from disk.
    pub fn remove(&self, key: &Key) {
        let mut state = self.state();
        state.routers.retain(|(routed, _), _| routed != key);
        state.entries.remove(key);
        self.delete(key);
    }

    /// Deletes version `key`'s folder and its downloads from disk.
    fn delete(&self, key: &Key) {
        let _ = std::fs::remove_dir_all(self.folder(key));
        let prefix = format!("{}.download.", key.1);
        for file in std::fs::read_dir(self.dir.join(&key.0))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
        {
            if file.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }

    /// Version `served`'s download as `extension` — rendered by `render`
    /// from its folder the first time, then read from beside it, its bytes
    /// counted with the version. A version served uncached renders it each
    /// time and keeps nothing.
    pub async fn download<F>(
        &self,
        served: &Served,
        extension: &str,
        render: impl FnOnce(PathBuf) -> F,
    ) -> Result<Vec<u8>>
    where
        F: Future<Output = Result<Vec<u8>>>,
    {
        if served.uncached.is_some() {
            return render(served.folder.clone()).await;
        }
        let key = &served.key;
        let file = self
            .dir
            .join(&key.0)
            .join(format!("{}.download.{extension}", key.1));
        let making = (format!("{}.download", key.0), key.1);
        let lock = self
            .making
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(making.clone())
            .or_default()
            .clone();
        let _making = lock.lock().await;
        let bytes = self.rendered(served, &file, render).await;
        self.making
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&making);
        bytes
    }

    /// The download at `file`, or rendered into it.
    async fn rendered<F>(
        &self,
        served: &Served,
        file: &Path,
        render: impl FnOnce(PathBuf) -> F,
    ) -> Result<Vec<u8>>
    where
        F: Future<Output = Result<Vec<u8>>>,
    {
        let key = &served.key;
        if let Ok(bytes) = std::fs::read(file) {
            eprintln!("cache: hit {} v{} download", key.0, key.1);
            metrics::count(metrics::Cache::Hit);
            return Ok(bytes);
        }
        eprintln!("cache: miss {} v{} download", key.0, key.1);
        metrics::count(metrics::Cache::Miss);
        let bytes = render(served.folder.clone()).await?;
        let mut partial = file.as_os_str().to_owned();
        partial.push(".partial");
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, file)?;
        if let Some(entry) = self.state().entries.get_mut(key) {
            entry.bytes += bytes.len() as u64;
        }
        self.evict();
        Ok(bytes)
    }

    /// Forgets every version of `document` and deletes them from disk.
    pub fn remove_document(&self, document: &str) {
        let mut state = self.state();
        state.routers.retain(|(key, _), _| key.0 != document);
        state.entries.retain(|key, _| key.0 != document);
        let _ = std::fs::remove_dir_all(self.dir.join(document));
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
