//! The server's API, as the CLI calls it: one request per method, each
//! carrying the context's token when it has one.

use anyhow::{Context as _, Result, bail};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

use super::contexts::Context;

/// A server, reached with a token or without one.
pub struct Api {
    server: String,
    token: Option<String>,
    http: reqwest::Client,
}

/// What a publish answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Published {
    pub id: String,
    pub version: u64,
    pub url: String,
}

/// A document as `GET /api/documents` lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Listed {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub latest: u64,
}

/// A document and its versions, as `GET /api/documents/<id>` answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Described {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub latest: u64,
    pub versions: Vec<Version>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Version {
    pub number: u64,
    pub author: String,
    pub published: String,
    #[serde(default)]
    pub size: u64,
}

/// A thread, as `GET /api/documents/<id>/comments` answers: the comment
/// store's fields, its page, the version it was started on, whether its
/// text is gone from the latest version, and the version it left.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thread {
    pub id: String,
    pub page: String,
    pub version: u64,
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub detached_in: Option<u64>,
    pub anchor: devpage_core::comments::Anchor,
    pub thread: Vec<devpage_core::comments::Message>,
}

impl Api {
    /// The context's server, with the token its token command prints.
    pub fn connect(context: &Context) -> Result<Api> {
        Ok(Api {
            server: context.server.trim_end_matches('/').to_string(),
            token: context.token()?,
            http: reqwest::Client::new(),
        })
    }

    /// `path` on this server, as a link.
    pub fn link(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let request = self.http.request(method, self.link(path));
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        what: &str,
    ) -> Result<reqwest::Response> {
        let response = request
            .send()
            .await
            .with_context(|| format!("{what}: reaching {}", self.server))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        let body = body.trim();
        match status {
            StatusCode::UNPROCESSABLE_ENTITY => bail!("{what}: the server refused it:\n{body}"),
            StatusCode::NOT_FOUND => bail!("{what}: not found on {}", self.server),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                bail!("{what}: {status} — does the context need a token command?")
            }
            _ if body.is_empty() => bail!("{what}: {status}"),
            _ => bail!("{what}: {status}: {body}"),
        }
    }

    async fn json<T: for<'de> Deserialize<'de>>(&self, path: &str, what: &str) -> Result<T> {
        let response = self.send(self.request(Method::GET, path), what).await?;
        let bytes = response.bytes().await?;
        serde_json::from_slice(&bytes).with_context(|| format!("{what}: reading the answer"))
    }

    /// Creates a document from `zip` — `id` `None` — or adds a version to
    /// document `id`. `entry` names the document file inside the zip.
    pub async fn publish(&self, id: Option<&str>, zip: Vec<u8>, entry: &str) -> Result<Published> {
        let path = match id {
            None => "/api/documents".to_string(),
            Some(id) => format!("/api/documents/{id}/versions"),
        };
        let entry: String = url::form_urlencoded::byte_serialize(entry.as_bytes()).collect();
        let request = self
            .request(Method::POST, &format!("{path}?entry={entry}"))
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .body(zip);
        let response = self.send(request, "publish").await?;
        let bytes = response.bytes().await?;
        let mut published: serde_json::Value =
            serde_json::from_slice(&bytes).context("publish: reading the answer")?;
        // Adding a version answers without the id it was given.
        if let (Some(id), Some(object)) = (id, published.as_object_mut()) {
            object
                .entry("id")
                .or_insert_with(|| serde_json::Value::String(id.to_string()));
        }
        serde_json::from_value(published).context("publish: reading the answer")
    }

    pub async fn list(&self) -> Result<Vec<Listed>> {
        self.json("/api/documents", "list").await
    }

    pub async fn describe(&self, id: &str) -> Result<Described> {
        self.json(&format!("/api/documents/{id}"), id).await
    }

    pub async fn comments(&self, id: &str) -> Result<Vec<Thread>> {
        self.json(&format!("/api/documents/{id}/comments"), id)
            .await
    }

    pub async fn reply(&self, id: &str, thread: &str, text: &str) -> Result<()> {
        let request = self
            .request(
                Method::POST,
                &format!("/api/documents/{id}/comments/{thread}/replies"),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::json!({ "text": text }).to_string());
        self.send(request, thread).await.map(drop)
    }

    pub async fn resolve(&self, id: &str, thread: &str) -> Result<()> {
        let request = self.request(
            Method::DELETE,
            &format!("/api/documents/{id}/comments/{thread}"),
        );
        self.send(request, thread).await.map(drop)
    }

    /// Deletes the document, or only its version `n`.
    pub async fn delete(&self, id: &str, version: Option<u64>) -> Result<()> {
        let path = match version {
            None => format!("/api/documents/{id}"),
            Some(n) => format!("/api/documents/{id}/versions/{n}"),
        };
        self.send(self.request(Method::DELETE, &path), id)
            .await
            .map(drop)
    }

    /// Version `n`'s PDF or PNG.
    pub async fn download(&self, id: &str, version: u64) -> Result<Vec<u8>> {
        let path = format!("/d/{id}/v/{version}/download");
        let response = self.send(self.request(Method::GET, &path), id).await?;
        Ok(response.bytes().await?.to_vec())
    }
}
