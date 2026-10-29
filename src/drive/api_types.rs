//! The server API's JSON: what `devpage server` answers and what the CLI
//! reads, written once. Server mode serializes these and the client
//! deserializes the same types, so the two cannot drift apart.
//!
//! A refusal is a type too: the status and text a refused request gets,
//! made by the server and read back by the client as the same `Refusal`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use devpage_core::comments::{Anchor, Message, Refused};
use serde::{Deserialize, Serialize};

/// What a publish answers: `POST /api/documents` and
/// `POST /api/documents/<id>/versions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Published {
    pub id: String,
    pub version: u64,
    /// The latest version's link, from the top of the server: `/d/<id>`.
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
    pub created: String,
    pub versions: Vec<Version>,
}

/// One publish of a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Version {
    pub number: u64,
    pub kind: String,
    pub title: String,
    pub author: String,
    pub published: String,
    pub size: u64,
    pub sha256: String,
}

/// A thread, as `GET /api/documents/<id>/comments` lists it: its page, the
/// version it was started on, whether its text is gone from the latest
/// version, the version it left, its anchor and its messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thread {
    pub id: String,
    pub page: String,
    pub version: u64,
    pub detached: bool,
    /// The version whose publish found its text gone; `None` while attached.
    pub detached_in: Option<u64>,
    pub anchor: Anchor,
    pub thread: Vec<Message>,
}

/// A thread to start: `POST /api/documents/<id>/comments`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewThread {
    pub page: String,
    pub anchor: Anchor,
    pub text: String,
}

/// What starting a thread answers: its id and the version it is on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Started {
    pub id: String,
    pub version: u64,
}

/// A reply: `POST /api/documents/<id>/comments/<thread>/replies`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reply {
    pub text: String,
}

/// Why the API refused a request, as its status and text.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// `404`: no such document, version or thread.
    NotFound,
    /// `401`: a write without the header naming its author; why.
    Unauthorized(String),
    /// `422`: what is wrong with the request — a publish's problems.
    Unprocessable(String),
    /// Any other status, with its text: the server failing (`500`), an
    /// upload over the limit (`413`), or a proxy in front of the server.
    Failed { status: u16, text: String },
}

impl Refusal {
    /// The refusal a request answered `status` with `text` was given.
    pub fn read(status: u16, text: &str) -> Refusal {
        let text = text.trim().to_string();
        match status {
            404 => Refusal::NotFound,
            401 => Refusal::Unauthorized(text),
            422 => Refusal::Unprocessable(text),
            status => Refusal::Failed { status, text },
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Refusal::NotFound => StatusCode::NOT_FOUND,
            Refusal::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Refusal::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Refusal::Failed { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    fn text(&self) -> &str {
        match self {
            Refusal::NotFound => "",
            Refusal::Unauthorized(text)
            | Refusal::Unprocessable(text)
            | Refusal::Failed { text, .. } => text,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.text() {
            "" => write!(f, "{}", self.status()),
            text => write!(f, "{}: {text}", self.status()),
        }
    }
}

impl std::error::Error for Refusal {}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            eprintln!("server error: {}", self.text());
        }
        match self {
            Refusal::NotFound => status.into_response(),
            refusal => (status, refusal.text().to_string()).into_response(),
        }
    }
}

/// A failure inside the server: `500`, with the error's whole chain.
impl From<anyhow::Error> for Refusal {
    fn from(error: anyhow::Error) -> Refusal {
        Refusal::Failed {
            status: 500,
            text: format!("{error:#}"),
        }
    }
}

/// A refused comment request: `401` with no author, `422` for a page that
/// isn't a file in the version, `404` for no such thread, `500` when the
/// store failed.
impl From<Refused> for Refusal {
    fn from(refused: Refused) -> Refusal {
        match refused {
            Refused::Unidentified(_) => Refusal::Unauthorized(refused.to_string()),
            Refused::NotAPage(_) => Refusal::Unprocessable(refused.to_string()),
            Refused::NoThread(_) => Refusal::NotFound,
            Refused::Failed(error) => error.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every refusal the server answers is read back as the same refusal.
    #[tokio::test]
    async fn a_refusal_reads_back_as_the_one_the_server_answered() {
        for refusal in [
            Refusal::NotFound,
            Refusal::Unauthorized("a write needs the x-user header".into()),
            Refusal::Unprocessable("plan.md:3: broken link gone.md".into()),
            Refusal::Failed {
                status: 413,
                text: "too large".into(),
            },
            Refusal::from(anyhow::anyhow!("the bucket is gone")),
        ] {
            let response = refusal.clone().into_response();
            let status = response.status().as_u16();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let text = String::from_utf8(body.to_vec()).unwrap();
            assert_eq!(Refusal::read(status, &text), refusal);
        }
    }
}
