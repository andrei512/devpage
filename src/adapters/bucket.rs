//! The bucket: where every version's zip is kept, durable. S3, Azure Blob, or
//! — for running without a cloud — a folder, named by one URL.

use std::sync::Arc;

use anyhow::{Context, Result};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};

#[derive(Clone)]
pub struct Bucket {
    store: Arc<dyn ObjectStore>,
    prefix: Path,
}

impl Bucket {
    /// The bucket `url` names: `s3://bucket/prefix`, `az://container/prefix`
    /// or `file:///folder`. Credentials come from the SDKs' own environment.
    pub fn open(url: &str) -> Result<Bucket> {
        let parsed = url::Url::parse(url).context("DEVPAGE_BUCKET_URL")?;
        let options = std::env::vars()
            .filter(|(k, _)| k.starts_with("AWS_") || k.starts_with("AZURE_"))
            .map(|(k, v)| (k.to_ascii_lowercase(), v));
        let (store, prefix) =
            object_store::parse_url_opts(&parsed, options).context("DEVPAGE_BUCKET_URL")?;
        Ok(Bucket {
            store: Arc::from(store),
            prefix,
        })
    }

    fn version(&self, document: &str, version: u64) -> Path {
        self.prefix
            .clone()
            .join("documents")
            .join(document)
            .join("versions")
            .join(format!("{version}.zip"))
    }

    /// Keeps a version's zip as uploaded.
    pub async fn put_version(&self, document: &str, version: u64, zip: Vec<u8>) -> Result<()> {
        self.store
            .put(&self.version(document, version), PutPayload::from(zip))
            .await?;
        Ok(())
    }

    /// A version's zip, as uploaded.
    pub async fn get_version(&self, document: &str, version: u64) -> Result<Vec<u8>> {
        let got = self.store.get(&self.version(document, version)).await?;
        Ok(got.bytes().await?.to_vec())
    }
}
