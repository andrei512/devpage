//! The bucket: where every version's zip is kept, durable. S3, Azure Blob, or
//! — for running without a cloud — a folder, named by one URL.

use std::sync::Arc;

use anyhow::{Context, Result};
use object_store::aws::AmazonS3Builder;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::local::LocalFileSystem;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};

#[derive(Clone)]
pub struct Bucket {
    store: Arc<dyn ObjectStore>,
    prefix: Path,
}

impl Bucket {
    /// The bucket `url` names: `s3://bucket/prefix`, `az://container/prefix`
    /// or `file:///folder`. Credentials come from object_store's own reading
    /// of the `AWS_*` and `AZURE_*` variables, so workload identity needs
    /// none set.
    pub fn open(url: &str) -> Result<Bucket> {
        let parsed = url::Url::parse(url)
            .ok()
            .context("DEVPAGE_BUCKET_URL is not a URL")?;
        let prefix = Path::from(parsed.path().trim_matches('/'));
        let store: Arc<dyn ObjectStore> = match parsed.scheme() {
            "s3" => Arc::new(
                AmazonS3Builder::from_env()
                    .with_url(url)
                    .build()
                    .context("DEVPAGE_BUCKET_URL: S3")?,
            ),
            "az" | "azure" => Arc::new(
                MicrosoftAzureBuilder::from_env()
                    .with_url(url)
                    .build()
                    .context("DEVPAGE_BUCKET_URL: Azure Blob")?,
            ),
            "file" => {
                let folder = parsed
                    .to_file_path()
                    .ok()
                    .context("DEVPAGE_BUCKET_URL is not a folder")?;
                std::fs::create_dir_all(&folder).context("DEVPAGE_BUCKET_URL's folder")?;
                return Ok(Bucket {
                    store: Arc::new(
                        LocalFileSystem::new_with_prefix(&folder)
                            .context("DEVPAGE_BUCKET_URL's folder")?,
                    ),
                    prefix: Path::default(),
                });
            }
            _ => anyhow::bail!("DEVPAGE_BUCKET_URL is not an s3://, az:// or file:// URL"),
        };
        Ok(Bucket { store, prefix })
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

    /// Deletes a version's zip; one already gone is no error.
    pub async fn delete_version(&self, document: &str, version: u64) -> Result<()> {
        match self.store.delete(&self.version(document, version)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A URL that names no bucket stops the server by the variable's name,
    /// never echoing its value, which may be a secret pasted into the wrong
    /// one.
    #[test]
    fn a_url_that_names_no_bucket_is_refused_by_name() {
        let secret = "s3cr3t-value";
        for (url, expected) in [
            (secret.to_string(), "DEVPAGE_BUCKET_URL is not a URL"),
            (
                format!("ftp://{secret}/p"),
                "DEVPAGE_BUCKET_URL is not an s3://, az:// or file:// URL",
            ),
        ] {
            let error = format!("{:#}", Bucket::open(&url).err().unwrap());
            assert_eq!(error, expected);
            assert!(!error.contains(secret));
        }
    }
}
