//! The server's API, as the CLI calls it: one request per method, each
//! carrying the context's token when it has one.

use anyhow::{Context as _, Result};
use reqwest::Method;
use serde::Deserialize;

use super::contexts::Context;
use crate::drive::api_types::{Described, Listed, Published, Refusal, Reply, Thread};

/// A server, reached with a token or without one.
pub struct Api {
    server: String,
    token: Option<String>,
    http: reqwest::Client,
}

/// A request the server refused: what was asked, of which server, and the
/// refusal it answered.
#[derive(Debug)]
pub struct Refused {
    pub what: String,
    pub server: String,
    pub refusal: Refusal,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Refused {
            what,
            server,
            refusal,
        } = self;
        match refusal {
            Refusal::NotFound => write!(f, "{what}: not found on {server}"),
            Refusal::Unprocessable(problems) => {
                write!(f, "{what}: the server refused it:\n{problems}")
            }
            Refusal::Unauthorized(_) | Refusal::Failed { status: 403, .. } => {
                write!(
                    f,
                    "{what}: {refusal} — does the context need a token command?"
                )
            }
            Refusal::Failed { .. } => write!(f, "{what}: {refusal}"),
        }
    }
}

impl std::error::Error for Refused {}

/// Whether `error` is a request the server answered with `refusal`.
pub fn refused(error: &anyhow::Error, refusal: &Refusal) -> bool {
    error
        .downcast_ref::<Refused>()
        .is_some_and(|refused| &refused.refusal == refusal)
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
        let text = response.text().await.unwrap_or_default();
        Err(Refused {
            what: what.to_string(),
            server: self.server.clone(),
            refusal: Refusal::read(status.as_u16(), &text),
        }
        .into())
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
        serde_json::from_slice(&bytes).context("publish: reading the answer")
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
            .body(serde_json::to_vec(&Reply {
                text: text.to_string(),
            })?);
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
