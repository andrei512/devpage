//! devpage: renders a document — Markdown files described by a YAML file — as a
//! live website. Usage: `devpage [document.yml | folder]`.

mod document;
mod highlight;
mod nav;
mod render;
mod server;
mod watch;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::document::Document;

/// The first port tried; when it is taken, the next one up, until one is free.
const FIRST_PORT: u16 = 3030;

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let document = Arc::new(Document::load(&path)?);

    let (changes, _) = broadcast::channel(16);
    let _watcher = watch::start(document.clone(), changes.clone())?;

    let listener = bind(FIRST_PORT).await?;
    let port = listener.local_addr()?.port();
    println!("{} → http://localhost:{port}", document.title);
    axum::serve(listener, server::router(document, changes)).await?;
    Ok(())
}

/// Binds the first free port at or above `first`.
async fn bind(first: u16) -> Result<TcpListener> {
    for port in first..=u16::MAX {
        if let Ok(listener) = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
        {
            return Ok(listener);
        }
    }
    None.context("no free port")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn takes_the_next_port_when_the_first_is_busy() {
        let busy = bind(41000).await.unwrap();
        let taken = busy.local_addr().unwrap().port();
        let next = bind(taken).await.unwrap();
        assert!(next.local_addr().unwrap().port() > taken);
    }
}
