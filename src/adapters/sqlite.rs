//! The store: documents, their versions and their comment threads in one
//! SQLite file.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, bail};
use devpage_core::comments::{self, Anchor, Listed, Message, Thread};
use rusqlite::{Connection, OptionalExtension, params};

pub struct Sqlite(Mutex<Connection>);

/// A document as the store records it.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRow {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub latest: u64,
    pub created: String,
}

/// One publish of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionRow {
    pub number: u64,
    pub kind: String,
    pub title: String,
    pub author: String,
    pub published: String,
    pub size: u64,
    pub sha256: String,
}

/// What a publish records.
pub struct Publish<'a> {
    /// The document file, from the top of the zip: `devpage.yml`,
    /// `talk.yml`, `notes.md`, `screen.design`…
    pub entry: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub author: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

/// What deleting one version did.
#[derive(Debug, PartialEq)]
pub enum Deleted {
    /// There was no such version.
    Nothing,
    /// The version went; the document has others.
    Version,
    /// It was the only version, so the document went with it.
    Document,
}

impl Sqlite {
    pub fn open(file: &Path) -> Result<Sqlite> {
        let connection = Connection::open(file)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS documents (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
               latest INTEGER NOT NULL,
               -- The highest number ever given: a deleted version's number is
               -- never given again, so its link never shows something else.
               last_number INTEGER NOT NULL,
               created TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS versions (
               document TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
               number INTEGER NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL,
               -- The document file, from the top of the zip.
               entry TEXT NOT NULL,
               author TEXT NOT NULL, published TEXT NOT NULL, size INTEGER NOT NULL,
               sha256 TEXT NOT NULL, PRIMARY KEY (document, number));
             -- A thread belongs to the document, not to a version: it is
             -- carried onto each new one, or detached when its text is gone.
             CREATE TABLE IF NOT EXISTS threads (
               document TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
               id TEXT NOT NULL,
               -- The Markdown file it is on, from the top of the document.
               page TEXT NOT NULL,
               quote TEXT NOT NULL, prefix TEXT NOT NULL, suffix TEXT NOT NULL,
               -- The version it was started on.
               version INTEGER NOT NULL,
               -- The version its text was gone from, or NULL.
               detached_in INTEGER,
               PRIMARY KEY (document, id));
             CREATE TABLE IF NOT EXISTS messages (
               document TEXT NOT NULL, thread TEXT NOT NULL,
               seq INTEGER NOT NULL,
               author TEXT NOT NULL, at TEXT NOT NULL, text TEXT NOT NULL,
               PRIMARY KEY (document, thread, seq),
               FOREIGN KEY (document, thread) REFERENCES threads(document, id)
                 ON DELETE CASCADE);",
        )?;
        Ok(Sqlite(Mutex::new(connection)))
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The number the document's next version takes: 1 for a new one.
    pub fn next_number(&self, id: &str) -> Result<u64> {
        let last: Option<i64> = self
            .connection()
            .query_row(
                "SELECT last_number FROM documents WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(last.map_or(1, |n| n as u64 + 1))
    }

    /// Records version `number` as the document's latest — creating the
    /// document when it is 1. Refused unless `number` is the one
    /// [`Sqlite::next_number`] gives, checked in the same transaction, so two
    /// publishes never share a number.
    pub fn record(&self, id: &str, number: u64, publish: &Publish) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let last: Option<i64> = tx
            .query_row(
                "SELECT last_number FROM documents WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if last.map_or(1, |n| n as u64 + 1) != number {
            bail!("version {number} of {id} was taken by another publish");
        }
        if last.is_none() {
            tx.execute(
                "INSERT INTO documents (id, kind, title, latest, last_number, created)
                 VALUES (?1, ?2, ?3, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
                params![id, publish.kind, publish.title],
            )?;
        } else {
            tx.execute(
                "UPDATE documents SET latest = ?2, last_number = ?2, kind = ?3, title = ?4
                 WHERE id = ?1",
                params![id, number as i64, publish.kind, publish.title],
            )?;
        }
        tx.execute(
            "INSERT INTO versions
               (document, number, entry, kind, title, author, published, size, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?7, ?8)",
            params![
                id,
                number as i64,
                publish.entry,
                publish.kind,
                publish.title,
                publish.author,
                publish.size as i64,
                publish.sha256
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The document's latest version, if it exists.
    pub fn latest(&self, id: &str) -> Result<Option<u64>> {
        Ok(self
            .connection()
            .query_row("SELECT latest FROM documents WHERE id = ?1", [id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .map(|n| n as u64))
    }

    /// Version `n`'s document file, when the version exists.
    pub fn entry(&self, id: &str, n: u64) -> Result<Option<String>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT entry FROM versions WHERE document = ?1 AND number = ?2",
                params![id, n as i64],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Every document, oldest first.
    pub fn list(&self) -> Result<Vec<DocumentRow>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, kind, title, latest, created FROM documents ORDER BY created, id",
        )?;
        let rows = statement.query_map([], document_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The document and its versions, oldest first.
    pub fn get(&self, id: &str) -> Result<Option<(DocumentRow, Vec<VersionRow>)>> {
        let connection = self.connection();
        let Some(document) = connection
            .query_row(
                "SELECT id, kind, title, latest, created FROM documents WHERE id = ?1",
                [id],
                document_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT number, kind, title, author, published, size, sha256 FROM versions
             WHERE document = ?1 ORDER BY number",
        )?;
        let versions = statement
            .query_map([id], |row| {
                Ok(VersionRow {
                    number: row.get::<_, i64>(0)? as u64,
                    kind: row.get(1)?,
                    title: row.get(2)?,
                    author: row.get(3)?,
                    published: row.get(4)?,
                    size: row.get::<_, i64>(5)? as u64,
                    sha256: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some((document, versions)))
    }

    /// Deletes the document and its versions: the numbers it had, or `None`
    /// when there was no such document.
    pub fn delete(&self, id: &str) -> Result<Option<Vec<u64>>> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let numbers = numbers(&tx, id)?;
        if tx.execute("DELETE FROM documents WHERE id = ?1", [id])? == 0 {
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(numbers))
    }

    /// Deletes version `n`. Deleting the latest makes the one before it the
    /// latest, and deleting the only one deletes the document.
    pub fn delete_version(&self, id: &str, n: u64) -> Result<Deleted> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let removed = tx.execute(
            "DELETE FROM versions WHERE document = ?1 AND number = ?2",
            params![id, n as i64],
        )?;
        if removed == 0 {
            return Ok(Deleted::Nothing);
        }
        let latest: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT number, kind, title FROM versions WHERE document = ?1
                 ORDER BY number DESC LIMIT 1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let deleted = match latest {
            Some((number, kind, title)) => {
                tx.execute(
                    "UPDATE documents SET latest = ?2, kind = ?3, title = ?4 WHERE id = ?1",
                    params![id, number, kind, title],
                )?;
                Deleted::Version
            }
            None => {
                tx.execute("DELETE FROM documents WHERE id = ?1", [id])?;
                Deleted::Document
            }
        };
        tx.commit()?;
        Ok(deleted)
    }
}

/// A thread as the store records it: the comment store's fields, its page,
/// the version it was started on, whether its text is gone from the latest
/// version, and the version it left.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRow {
    pub id: String,
    pub page: String,
    pub version: u64,
    pub detached: bool,
    /// The version whose publish found its text gone; `None` while attached.
    pub detached_in: Option<u64>,
    pub anchor: Anchor,
    pub thread: Vec<Message>,
}

impl Sqlite {
    /// The document's threads, oldest first — on `page` only, when given.
    pub fn threads(&self, document: &str, page: Option<&str>) -> Result<Vec<ThreadRow>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, page, version, detached_in, quote, prefix, suffix
             FROM threads WHERE document = ?1 AND (?2 IS NULL OR page = ?2) ORDER BY rowid",
        )?;
        let threads: Vec<ThreadRow> = statement
            .query_map(params![document, page], |row| {
                Ok(ThreadRow {
                    id: row.get(0)?,
                    page: row.get(1)?,
                    version: row.get::<_, i64>(2)? as u64,
                    detached: row.get::<_, Option<i64>>(3)?.is_some(),
                    detached_in: row.get::<_, Option<i64>>(3)?.map(|n| n as u64),
                    anchor: Anchor {
                        quote: row.get(4)?,
                        prefix: row.get(5)?,
                        suffix: row.get(6)?,
                    },
                    thread: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut messages = connection.prepare(
            "SELECT author, at, text FROM messages WHERE document = ?1 AND thread = ?2
             ORDER BY seq",
        )?;
        threads
            .into_iter()
            .map(|mut thread| {
                thread.thread = messages
                    .query_map(params![document, thread.id], |row| {
                        Ok(Message {
                            author: row.get(0)?,
                            at: row.get(1)?,
                            text: row.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(thread)
            })
            .collect()
    }

    /// Starts a thread on `page` of version `version`: its id. The document
    /// must exist.
    pub fn create_thread(
        &self,
        document: &str,
        version: u64,
        page: &str,
        anchor: &Anchor,
        message: &Message,
    ) -> Result<String> {
        let (id, _) = comments::new_id();
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT INTO threads (document, id, page, quote, prefix, suffix, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                document,
                id,
                page,
                anchor.quote,
                anchor.prefix,
                anchor.suffix,
                version as i64
            ],
        )?;
        add_message(&tx, document, &id, message)?;
        tx.commit()?;
        Ok(id)
    }

    /// Appends `message` to thread `id` — on `page`, when given. Whether there
    /// was such a thread.
    pub fn reply(
        &self,
        document: &str,
        page: Option<&str>,
        id: &str,
        message: &Message,
    ) -> Result<bool> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        if !thread_exists(&tx, document, page, id)? {
            return Ok(false);
        }
        add_message(&tx, document, id, message)?;
        tx.commit()?;
        Ok(true)
    }

    /// Resolves thread `id` — on `page`, when given: it is deleted. Whether
    /// there was such a thread.
    pub fn resolve(&self, document: &str, page: Option<&str>, id: &str) -> Result<bool> {
        Ok(self.connection().execute(
            "DELETE FROM threads WHERE document = ?1 AND id = ?2 AND (?3 IS NULL OR page = ?3)",
            params![document, id, page],
        )? > 0)
    }

    /// Carries thread `id` onto version `version`: at `anchor`, or detached
    /// when its text is gone.
    pub fn reanchor(
        &self,
        document: &str,
        id: &str,
        version: u64,
        anchor: Option<&Anchor>,
    ) -> Result<()> {
        let connection = self.connection();
        match anchor {
            Some(anchor) => connection.execute(
                "UPDATE threads SET quote = ?3, prefix = ?4, suffix = ?5
                 WHERE document = ?1 AND id = ?2",
                params![document, id, anchor.quote, anchor.prefix, anchor.suffix],
            )?,
            None => connection.execute(
                "UPDATE threads SET detached_in = ?3 WHERE document = ?1 AND id = ?2",
                params![document, id, version as i64],
            )?,
        };
        Ok(())
    }
}

fn thread_exists(
    connection: &Connection,
    document: &str,
    page: Option<&str>,
    id: &str,
) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM threads WHERE document = ?1 AND id = ?2 AND (?3 IS NULL OR page = ?3)",
            params![document, id, page],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn add_message(connection: &Connection, document: &str, id: &str, message: &Message) -> Result<()> {
    connection.execute(
        "INSERT INTO messages (document, thread, seq, author, at, text)
         VALUES (?1, ?2,
           (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE document = ?1 AND thread = ?2),
           ?3, ?4, ?5)",
        params![document, id, message.author, message.at, message.text],
    )?;
    Ok(())
}

/// One document's threads as a comment store: what the page of a version
/// writes through. A thread started here is on `version`.
pub struct DocumentThreads {
    pub store: Arc<Sqlite>,
    pub document: String,
    pub version: u64,
}

/// A page as the store keys it — `/a.md` — or why it can't be one.
fn page_key(page: &str) -> Result<String> {
    if !comments::is_page(page) || page.trim_start_matches('/').is_empty() {
        bail!("{page}: not a page of the document");
    }
    Ok(format!("/{}", page.trim_start_matches('/')))
}

impl comments::Store for DocumentThreads {
    fn list(&self, page: &str) -> Result<Vec<Listed>> {
        let page = page_key(page)?;
        Ok(self
            .store
            .threads(&self.document, Some(&page))?
            .into_iter()
            .map(|row| Listed {
                id: row.id,
                thread: Thread {
                    anchor: row.anchor,
                    thread: row.thread,
                },
                detached: row.detached,
            })
            .collect())
    }

    fn create(&self, page: &str, anchor: Anchor, author: &str, text: &str) -> Result<String> {
        let page = page_key(page)?;
        let (_, at) = comments::new_id();
        self.store.create_thread(
            &self.document,
            self.version,
            &page,
            &anchor,
            &Message {
                author: author.into(),
                at,
                text: text.into(),
            },
        )
    }

    fn reply(&self, page: &str, id: &str, author: &str, text: &str) -> Result<()> {
        let page = page_key(page)?;
        let (_, at) = comments::new_id();
        let message = Message {
            author: author.into(),
            at,
            text: text.into(),
        };
        if !self
            .store
            .reply(&self.document, Some(&page), id, &message)?
        {
            bail!("{id}: no such thread on {page}");
        }
        Ok(())
    }

    fn resolve(&self, page: &str, id: &str) -> Result<()> {
        let page = page_key(page)?;
        if !self.store.resolve(&self.document, Some(&page), id)? {
            bail!("{id}: no such thread on {page}");
        }
        Ok(())
    }
}

fn document_row(row: &rusqlite::Row) -> rusqlite::Result<DocumentRow> {
    Ok(DocumentRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        title: row.get(2)?,
        latest: row.get::<_, i64>(3)? as u64,
        created: row.get(4)?,
    })
}

fn numbers(connection: &Connection, id: &str) -> Result<Vec<u64>> {
    let mut statement =
        connection.prepare("SELECT number FROM versions WHERE document = ?1 ORDER BY number")?;
    let numbers = statement
        .query_map([id], |row| row.get::<_, i64>(0))?
        .map(|n| n.map(|n| n as u64))
        .collect::<rusqlite::Result<_>>()?;
    Ok(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Sqlite {
        let dir = std::env::temp_dir().join(format!("devpage-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Sqlite::open(&dir.join("store.db")).unwrap()
    }

    fn publish(title: &str) -> Publish<'_> {
        Publish {
            entry: "notes.md",
            kind: "doc",
            title,
            author: "ana@example.com",
            size: 10,
            sha256: "abc",
        }
    }

    #[test]
    fn a_number_is_given_once_even_after_its_version_is_deleted() {
        let store = store("numbers");
        assert_eq!(store.next_number("d").unwrap(), 1);
        store.record("d", 1, &publish("One")).unwrap();
        store.record("d", 2, &publish("Two")).unwrap();
        // A publish that read the same next number loses.
        assert!(store.record("d", 2, &publish("Late")).is_err());
        assert_eq!(store.delete_version("d", 2).unwrap(), Deleted::Version);
        assert_eq!(store.latest("d").unwrap(), Some(1));
        assert_eq!(store.next_number("d").unwrap(), 3);
        let (document, versions) = store.get("d").unwrap().unwrap();
        assert_eq!(document.title, "One");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].author, "ana@example.com");
    }

    #[test]
    fn deleting_the_only_version_deletes_the_document() {
        let store = store("only");
        store.record("d", 1, &publish("One")).unwrap();
        assert_eq!(store.delete_version("d", 9).unwrap(), Deleted::Nothing);
        assert_eq!(store.delete_version("d", 1).unwrap(), Deleted::Document);
        assert!(store.get("d").unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn deleting_a_document_deletes_its_versions() {
        let store = store("delete");
        store.record("d", 1, &publish("One")).unwrap();
        store.record("d", 2, &publish("Two")).unwrap();
        store.record("e", 1, &publish("Else")).unwrap();
        assert_eq!(store.delete("d").unwrap(), Some(vec![1, 2]));
        assert_eq!(store.delete("d").unwrap(), None);
        assert_eq!(store.entry("d", 1).unwrap(), None);
        assert_eq!(store.entry("e", 1).unwrap().as_deref(), Some("notes.md"));
        let listed: Vec<String> = store.list().unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(listed, ["e"]);
    }
}
