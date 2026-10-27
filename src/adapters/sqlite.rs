//! The store: documents and their versions in one SQLite file.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

pub struct Sqlite(Mutex<Connection>);

impl Sqlite {
    pub fn open(file: &Path) -> Result<Sqlite> {
        let connection = Connection::open(file)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS documents (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL,
               latest INTEGER NOT NULL, created TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS versions (
               document TEXT NOT NULL REFERENCES documents(id), number INTEGER NOT NULL,
               author TEXT NOT NULL, published TEXT NOT NULL, size INTEGER NOT NULL,
               sha256 TEXT NOT NULL, PRIMARY KEY (document, number));",
        )?;
        Ok(Sqlite(Mutex::new(connection)))
    }

    /// Records a new document at version 1.
    pub fn create(&self, id: &str, kind: &str, title: &str, size: u64, sha256: &str) -> Result<()> {
        let mut connection = self.0.lock().unwrap();
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT INTO documents (id, kind, title, latest, created) VALUES (?1, ?2, ?3, 1, datetime('now'))",
            params![id, kind, title],
        )?;
        tx.execute(
            "INSERT INTO versions (document, number, author, published, size, sha256)
             VALUES (?1, 1, 'anonymous', datetime('now'), ?2, ?3)",
            params![id, size as i64, sha256],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Records version `n` of a document as its latest.
    pub fn add_version(
        &self,
        id: &str,
        n: u64,
        title: &str,
        size: u64,
        sha256: &str,
    ) -> Result<()> {
        let mut connection = self.0.lock().unwrap();
        let tx = connection.transaction()?;
        tx.execute(
            "INSERT INTO versions (document, number, author, published, size, sha256)
             VALUES (?1, ?2, 'anonymous', datetime('now'), ?3, ?4)",
            params![id, n as i64, size as i64, sha256],
        )?;
        tx.execute(
            "UPDATE documents SET latest = ?2, title = ?3 WHERE id = ?1",
            params![id, n as i64, title],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The document's latest version, if it exists.
    pub fn latest(&self, id: &str) -> Result<Option<u64>> {
        let connection = self.0.lock().unwrap();
        Ok(connection
            .query_row("SELECT latest FROM documents WHERE id = ?1", [id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .map(|n| n as u64))
    }

    /// Whether version `n` of the document exists.
    pub fn has(&self, id: &str, n: u64) -> Result<bool> {
        let connection = self.0.lock().unwrap();
        Ok(connection
            .query_row(
                "SELECT 1 FROM versions WHERE document = ?1 AND number = ?2",
                params![id, n as i64],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
