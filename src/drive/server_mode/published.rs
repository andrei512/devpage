//! The published documents: publishing a version and deleting a document or
//! a version, each in one fixed order; finding a version by its [`Mount`];
//! and making what each document and each version needs — a document's
//! change bus, a version's comments and router, from one design store
//! shared by every version. What a document no longer needs goes with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, bail};
use axum::Router;
use devpage_core::comments::{Comments, Identity};
use devpage_core::designs;
use devpage_core::document::{Document, Kind};
use devpage_core::folder::{Folder, ServedFolder};
use devpage_design::Theme;
use sha2::{Digest, Sha256};

use super::Config;
use super::cache::{Cache, Served};
use crate::adapters::bucket::Bucket;
use crate::adapters::bundle;
use crate::adapters::changes::{Change, Changes};
use crate::adapters::sqlite::{
    Deleted, DocumentRow, DocumentThreads, Publish, Sqlite, ThreadRow, VersionRow,
};
use crate::drive::api_types::Refusal;
use crate::drive::http::{self, App, Commenting};

/// Which version of a document an address shows: the latest, in place at
/// `/d/<id>`, or version `n` at its own `/d/<id>/v/<n>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mount {
    Latest,
    Pinned(u64),
}

impl Mount {
    /// Where document `id` is served under this mount, without a trailing
    /// `/`: every address of a published document starts here.
    pub fn path(self, id: &str) -> String {
        match self {
            Mount::Latest => format!("/d/{id}"),
            Mount::Pinned(n) => format!("/d/{id}/v/{n}"),
        }
    }
}

/// What deleting takes: the whole document, or one version of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Deletion {
    Document,
    Version(u64),
}

/// A version held from the cache while it answers: never evicted meanwhile.
pub struct Version {
    pub id: String,
    pub number: u64,
    pub mount: Mount,
    served: Served,
}

impl Version {
    /// The router that answers for it under its mount.
    pub fn router(&self) -> Router {
        self.served.router.clone()
    }
}

/// A version's download: its bytes, media type and file name.
pub struct Download {
    pub bytes: Vec<u8>,
    pub media: &'static str,
    pub name: String,
}

/// Every published document.
#[derive(Clone)]
pub struct Published(Arc<Parts>);

/// What [`Published`] is made of, shared by every clone of it.
struct Parts {
    store: Arc<Sqlite>,
    bucket: Bucket,
    cache: Arc<Cache>,
    /// Where a publish unpacks its zip to check it, on the PVC.
    scratch: PathBuf,
    /// Who writes — a publish, a deletion or a comment: whoever
    /// `DEVPAGE_USER_HEADER` names, or `anonymous` when it isn't set.
    identity: Identity,
    /// Every version's designs: one renderer and one tile cache for all of
    /// them, each version's under its own mount, `/d/<id>/v/<n>`, whichever
    /// address serves it.
    designs: designs::Store,
    /// One lock per document being published to or deleted from: its
    /// publishes and deletions happen one at a time.
    locks: Locks,
    /// Each document's change bus, shared by every router serving its latest
    /// version at `/d/<id>`: a publish tells all of them, so a page open
    /// there swaps in the new version whichever version it loaded; and a
    /// comment written through the API reaches those pages as one written on
    /// them does. A deleted document's goes with it.
    buses: Mutex<HashMap<String, Changes>>,
}

impl Published {
    /// The documents published to the server `config` describes: its store,
    /// its bucket and its cache.
    pub fn open(config: &Config) -> Result<Published> {
        let bucket = Bucket::open(&config.bucket_url)?;
        let scratch = config.data_dir.join("scratch");
        // What a publish that stopped halfway left.
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)?;
        Ok(Published(Arc::new(Parts {
            store: Arc::new(Sqlite::open(&config.data_dir.join("store.db"))?),
            bucket,
            cache: Arc::new(Cache::open(
                &config.data_dir.join("cache"),
                config.cache_max_bytes,
                config.cache_policy.eviction(),
            )?),
            scratch: scratch.canonicalize()?,
            identity: match &config.user_header {
                Some(header) => Identity::Header(header.clone()),
                None => Identity::Local("anonymous".into()),
            },
            designs: designs::Store::new(),
            locks: Locks::default(),
            buses: Mutex::default(),
        })))
    }

    /// Who writes.
    pub fn identity(&self) -> &Identity {
        &self.0.identity
    }

    /// Publishes `zip`, whose document file is `entry`, by `author`: as
    /// version 1 of a new document, or as the next version of document `to`,
    /// which must exist. Answers the document's id and the version's number.
    /// In this order: the zip is checked; the document's lock taken; the
    /// number given; the document's threads' carry onto it worked out; the
    /// zip kept in the bucket; the version and its carry recorded together —
    /// or neither, and nothing is published; then its open pages are told.
    pub async fn publish(
        &self,
        to: Option<&str>,
        entry: &str,
        author: &str,
        zip: Vec<u8>,
    ) -> Result<(String, u64), Refusal> {
        let parts = &self.0;
        let id = match to {
            Some(id) => {
                parts.store.latest(id)?.ok_or(Refusal::NotFound)?;
                id.to_string()
            }
            None => uuid::Uuid::new_v4().hyphenated().to_string(),
        };
        let id = id.as_str();
        // Unpacked once to read and check it; the bucket keeps the zip
        // unchanged. Kept until the publish is done: the carry reads the new
        // version's pages from it.
        let scratch = Scratch::new(&parts.scratch)?;
        let (kind, title) = check(&parts.designs, &zip, entry, &scratch.0)
            .await
            .map_err(|e| Refusal::Unprocessable(format!("{e:#}")))?;
        let sha256 = format!("{:x}", Sha256::digest(&zip));
        let size = zip.len() as u64;

        let _one_at_a_time = parts.locks.hold(id).await;
        let version = parts.store.next_number(id)?;
        let folder = ServedFolder::new(&scratch.0)
            .map_err(anyhow::Error::from)?
            .mounted(&Mount::Latest.path(id));
        let comments = self.comments_of(id, version, folder.clone());
        let carrying = comments.clone();
        let entry_file = folder.path().join(entry);
        // Rendering a page draws its diagrams, which blocks.
        let carry = tokio::task::spawn_blocking(move || {
            let version = Folder::new(Arc::new(Document::load(&entry_file, &folder)?));
            carrying.carry(&version)
        })
        .await
        .map_err(anyhow::Error::from)??;
        parts.bucket.put_version(id, version, zip).await?;
        let recorded = parts.store.record(
            id,
            version,
            &Publish {
                entry,
                kind,
                title: &title,
                author,
                size,
                sha256: &sha256,
            },
            &carry,
        );
        if let Err(error) = recorded {
            // Nothing names it: the next publish takes its number again.
            let _ = parts.bucket.delete_version(id, version).await;
            return Err(error.into());
        }
        eprintln!("published {id} v{version} by {author}");
        comments.carried(carry);
        self.announce(id);
        Ok((id.to_string(), version))
    }

    /// Deletes what `deletion` names of document `id`, by `author`. In this
    /// order: the document's lock taken; the store's record deleted; each
    /// version's zip deleted from the bucket and its copy from the cache;
    /// its open pages told; and when the document went — deleting its only
    /// version takes it too — what it alone needed let go.
    pub async fn delete(&self, id: &str, deletion: Deletion, author: &str) -> Result<(), Refusal> {
        let parts = &self.0;
        let _one_at_a_time = parts.locks.hold(id).await;
        let (numbers, gone) = match deletion {
            Deletion::Document => (parts.store.delete(id)?.ok_or(Refusal::NotFound)?, true),
            Deletion::Version(n) => match parts.store.delete_version(id, n)? {
                Deleted::Nothing => return Err(Refusal::NotFound),
                Deleted::Version => (vec![n], false),
                Deleted::Document => (vec![n], true),
            },
        };
        for &n in &numbers {
            parts.bucket.delete_version(id, n).await?;
            parts.cache.remove(&(id.to_string(), n));
        }
        if gone {
            parts.cache.remove_document(id);
        }
        self.announce(id);
        if gone {
            lock(&parts.buses).remove(id);
        }
        match deletion {
            Deletion::Document => eprintln!("deleted {id} by {author}"),
            Deletion::Version(n) => eprintln!("deleted {id} v{n} by {author}"),
        }
        Ok(())
    }

    /// Every document, oldest first.
    pub fn list(&self) -> Result<Vec<DocumentRow>, Refusal> {
        Ok(self.0.store.list()?)
    }

    /// Document `id` and its versions, oldest first.
    pub fn get(&self, id: &str) -> Result<(DocumentRow, Vec<VersionRow>), Refusal> {
        self.0.store.get(id)?.ok_or(Refusal::NotFound)
    }

    /// Document `id`'s threads, oldest first.
    pub fn threads(&self, id: &str) -> Result<Vec<ThreadRow>, Refusal> {
        self.0.store.latest(id)?.ok_or(Refusal::NotFound)?;
        Ok(self.0.store.threads(id)?)
    }

    /// The version of document `id` that `mount` shows, held while it
    /// answers: its router kept in the cache, or made on a miss — from the
    /// zip in the bucket when it isn't unpacked yet.
    pub async fn version(&self, id: &str, mount: Mount) -> Result<Version, Refusal> {
        let parts = &self.0;
        let number = match mount {
            Mount::Latest => parts.store.latest(id)?.ok_or(Refusal::NotFound)?,
            Mount::Pinned(n) => n,
        };
        let entry = parts.store.entry(id, number)?.ok_or(Refusal::NotFound)?;
        let key = (id.to_string(), number);
        let at = mount.path(id);
        let fetch = parts.bucket.get_version(id, number);
        let served = parts
            .cache
            .router(&key, &at, fetch, |dir: &Path| {
                self.router(id, number, mount, dir, &entry)
            })
            .await?;
        Ok(Version {
            id: id.to_string(),
            number,
            mount,
            served,
        })
    }

    /// The comments of `version`, which is held: a thread started through
    /// them is on it.
    pub fn comments(&self, version: &Version) -> Result<Comments, Refusal> {
        let folder = ServedFolder::new(&version.served.folder)
            .map_err(anyhow::Error::from)?
            .mounted(&version.mount.path(&version.id));
        Ok(self.comments_of(&version.id, version.number, folder))
    }

    /// Version `n`'s download — a doc or slides printed as a PDF, a design
    /// painted as its PNG — rendered once and then kept in the cache beside
    /// the version; `None` when there is no such version or its kind has
    /// none.
    pub async fn download(&self, id: &str, n: u64) -> Result<Option<Download>, Refusal> {
        let (_, versions) = match self.0.store.get(id)? {
            Some(found) => found,
            None => return Ok(None),
        };
        let Some(row) = versions.into_iter().find(|v| v.number == n) else {
            return Ok(None);
        };
        let Some((extension, media)) = downloadable(&row.kind) else {
            return Ok(None);
        };
        let version = self.version(id, Mount::Pinned(n)).await?;
        let entry = self.0.store.entry(id, n)?.ok_or(Refusal::NotFound)?;
        let designs = self.0.designs.clone();
        let mount = Mount::Pinned(n).path(id);
        let bytes = self
            .0
            .cache
            .download(&version.served, extension, |folder| async move {
                render(&designs, &mount, &folder, &entry, extension).await
            })
            .await?;
        let name = devpage_core::document::download_name(&row.title, extension)
            .to_string_lossy()
            .into_owned();
        Ok(Some(Download { bytes, media, name }))
    }

    /// Document `id`'s change bus, made the first time.
    fn bus(&self, id: &str) -> Changes {
        lock(&self.0.buses)
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    /// Tells every page open at `/d/<id>` that its latest version changed.
    fn announce(&self, id: &str) {
        self.bus(id).publish(Change::Refresh);
    }

    /// The comments of version `version` of document `id`, whose files are
    /// `folder`: a thread started through them is on that version, a carry
    /// worked out through them is onto it, and every write is announced on
    /// the document's bus.
    fn comments_of(&self, id: &str, version: u64, folder: ServedFolder) -> Comments {
        Comments::new(
            Arc::new(DocumentThreads {
                store: self.0.store.clone(),
                document: id.to_string(),
                version,
            }),
            Arc::new(self.bus(id)),
            folder,
        )
    }

    /// Version `n`'s router under `mount`, from its files in `dir` and its
    /// document file `entry`. The latest's pages hear the document's
    /// publishes and its comments, and show and take them; a version's own
    /// pages, behind their banner, never change and are read as they were.
    fn router(&self, id: &str, n: u64, mount: Mount, dir: &Path, entry: &str) -> Result<Router> {
        // The served folder carries the mount, so every address the
        // version's router makes — redirects included — is already under it.
        let served = ServedFolder::new(dir)?.mounted(&mount.path(id));
        let document = Arc::new(Document::load(&served.path().join(entry), &served)?);
        let designs = self.0.designs.designs(&Mount::Pinned(n).path(id), &served);
        let app = if mount == Mount::Latest {
            let comments = Commenting::new(
                self.comments_of(id, n, served.clone()),
                self.0.identity.clone(),
            );
            App::published(document, designs, self.bus(id), Some(comments))
        } else {
            App::published(document, designs, Changes::default(), None)
        };
        Ok(http::router(app))
    }
}

/// What a version of `kind` downloads as: its extension and media type.
pub fn downloadable(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "doc" | "slides" => Some(("pdf", "application/pdf")),
        "design" => Some(("png", "image/png")),
        _ => None,
    }
}

/// The document at `entry` in `folder` rendered: printed by Chrome as
/// `devpage pdf` prints it, or a design painted as its PNG, in light, from
/// the version's designs under `mount`.
async fn render(
    designs: &designs::Store,
    mount: &str,
    folder: &Path,
    entry: &str,
    extension: &str,
) -> Result<Vec<u8>> {
    let served = ServedFolder::new(folder)?;
    if extension == "png" {
        let designs = designs.designs(mount, &served);
        let entry = entry.to_string();
        return tokio::task::spawn_blocking(move || designs.png(&entry, Theme::Light))
            .await?
            .map_err(anyhow::Error::msg);
    }
    let document = Document::load(&served.path().join(entry), &served)?;
    crate::drive::pdf::print(Arc::new(document)).await
}

/// What the zip is — its kind and title — or why it can't be published:
/// a file outside it, no document at `entry`, or a broken link inside it.
/// External addresses aren't asked: the CLI asks them before uploading, so a
/// publish never waits on the internet. Its designs are checked in the
/// server's one design store, under the scratch folder's own mount, which
/// leaves the store when the check is done.
async fn check(
    store: &designs::Store,
    zip: &[u8],
    entry: &str,
    scratch: &Path,
) -> Result<(&'static str, String)> {
    bundle::unpack(zip, scratch)?;
    let scratch = ServedFolder::new(scratch)?;
    let file = scratch.path().join(entry);
    if !file.is_file() {
        bail!("{entry}: no such file in the zip");
    }
    let folder = Folder::new(Arc::new(Document::load(&file, &scratch)?));
    let designs = store.designs(&format!("scratch:{}", scratch.path().display()), &scratch);
    let problems = devpage_core::check::document(&folder, &designs, None).await;
    let document = folder.entry();
    if !problems.is_empty() {
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        bail!("{}", lines.join("\n"));
    }
    let kind = match document.kind {
        Kind::Site { .. } => "site",
        Kind::Doc { .. } => "doc",
        Kind::Slides { .. } => "slides",
        Kind::Design => "design",
    };
    Ok((kind, document.title.clone()))
}

/// A publish's scratch folder, removed however the publish ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(under: &Path) -> Result<Scratch> {
        let folder = under.join(uuid::Uuid::new_v4().simple().to_string());
        std::fs::create_dir_all(&folder)?;
        Ok(Scratch(folder))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One lock per document, kept only while something holds or waits for it.
#[derive(Default)]
struct Locks(Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>);

/// A document's lock, held: let go — and forgotten when no one else wants
/// it — when dropped.
struct Held<'a> {
    locks: &'a Locks,
    id: String,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Locks {
    async fn hold(&self, id: &str) -> Held<'_> {
        let mutex = lock(&self.0).entry(id.to_string()).or_default().clone();
        Held {
            locks: self,
            id: id.to_string(),
            guard: Some(mutex.lock_owned().await),
        }
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        drop(self.guard.take());
        let mut locks = lock(&self.locks.0);
        // Only the map holds it: no one holds it or waits for it, and anyone
        // who wants it next takes it from the map, under this same lock.
        if locks
            .get(&self.id)
            .is_some_and(|mutex| Arc::strong_count(mutex) == 1)
        {
            locks.remove(&self.id);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::server_mode::tests::{config, site};

    /// A deleted document takes its change bus with it, and no lock is kept
    /// once its publishes and deletions are done.
    #[tokio::test]
    async fn a_deleted_document_leaves_nothing_behind() {
        let published = Published::open(&config("prune")).unwrap();
        let one = || site("Plan", "one");
        let (id, _) = published
            .publish(None, "devpage.yml", "ana", one())
            .await
            .unwrap();
        published
            .publish(Some(&id), "devpage.yml", "ana", one())
            .await
            .unwrap();
        drop(published.version(&id, Mount::Latest).await.unwrap());
        assert!(lock(&published.0.buses).contains_key(&id));
        assert!(lock(&published.0.locks.0).is_empty());

        published
            .delete(&id, Deletion::Version(1), "ana")
            .await
            .unwrap();
        assert!(
            lock(&published.0.buses).contains_key(&id),
            "it has version 2"
        );
        published
            .delete(&id, Deletion::Version(2), "ana")
            .await
            .unwrap();
        assert!(
            lock(&published.0.buses).is_empty(),
            "its only version took it"
        );
        assert!(lock(&published.0.locks.0).is_empty());
        assert!(matches!(
            published.delete(&id, Deletion::Document, "ana").await,
            Err(Refusal::NotFound)
        ));
    }

    #[test]
    fn a_mount_is_where_its_version_is_served() {
        let id = "8b1f";
        assert_eq!(Mount::Latest.path(id), "/d/8b1f");
        assert_eq!(Mount::Pinned(3).path(id), "/d/8b1f/v/3");
    }
}
