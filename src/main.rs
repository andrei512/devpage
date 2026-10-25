//! devpage: serves the documents of a repository — sites, docs and slides
//! written in Markdown — live in the browser, and prints them to PDF.

mod check;
mod comments;
mod designs;
mod document;
mod highlight;
mod index;
mod nav;
mod pdf;
mod render;
mod server;
mod watch;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::document::{Document, Entry};
use crate::index::Index;

/// The first port tried; when it is taken, the next one up, until one is free.
const FIRST_PORT: u16 = 3030;

#[derive(Parser)]
#[command(version, about = "Live documents from Markdown in a repository")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// A document file, a folder holding devpage.yml, a Markdown file, or a design (.design).
    path: Option<PathBuf>,
    /// The served folder: nothing outside it is served. The current folder by default.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// The address to listen on; any non-loopback address means deployed.
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    /// Listen on exactly this port instead of the first free one from 3030.
    #[arg(long)]
    port: Option<u16>,
    /// The header naming a comment's author when deployed.
    #[arg(long, default_value = "X-Forwarded-Email")]
    user_header: String,
}

#[derive(Subcommand)]
enum Command {
    /// Check every link; exit 1 if one is broken.
    Check { path: Option<PathBuf> },
    /// Print a doc or a deck to PDF.
    Pdf {
        path: PathBuf,
        /// Where to write the PDF; `<title>.pdf` by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();
    let served = cli.root.clone().unwrap_or_else(|| PathBuf::from("."));
    if let Some(Command::Check { path }) = &cli.command {
        let document = Document::load(
            path.as_deref().unwrap_or(std::path::Path::new(".")),
            &served,
        )?;
        let problems = check::document(&document, true).await;
        for problem in &problems {
            println!("{problem}");
        }
        std::process::exit(i32::from(!problems.is_empty()));
    }
    if let Some(Command::Pdf { path, output }) = cli.command.take() {
        let document = Arc::new(Document::load(&path, &served)?);
        let output = output.unwrap_or_else(|| pdf::default_output(&document.title));
        pdf::print(document, &output).await?;
        println!("{}", output.display());
        return Ok(());
    }
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let document = Arc::new(Document::load(&path, &served)?);
    let index = Arc::new(Index::build(&document.served));
    let entry = Arc::new(Entry::new(document.clone()));

    let designs = designs::Designs::new(document.served.clone());

    let (changes, _) = broadcast::channel(16);
    check::announce_on(changes.clone());
    let _watcher = watch::start(
        entry.clone(),
        index.clone(),
        designs.clone(),
        changes.clone(),
    )?;

    let listener = match cli.port {
        Some(port) => TcpListener::bind(SocketAddr::from((cli.host, port)))
            .await
            .with_context(|| format!("binding {}:{port}", cli.host))?,
        None => bind(cli.host, FIRST_PORT).await?,
    };
    let port = listener.local_addr()?.port();
    let shown = if cli.host.is_loopback() {
        "localhost".to_string()
    } else {
        cli.host.to_string()
    };
    println!("{} → http://{shown}:{port}", document.title);
    let identity = if cli.host.is_loopback() {
        server::Identity::Local(comments::local_author(&document.served))
    } else {
        server::Identity::Header(cli.user_header)
    };
    axum::serve(
        listener,
        server::router_with_index(entry, index, designs, changes, identity),
    )
    .await?;
    Ok(())
}

/// Binds the first free port at or above `first`.
async fn bind(host: IpAddr, first: u16) -> Result<TcpListener> {
    for port in first..=u16::MAX {
        if let Ok(listener) = TcpListener::bind(SocketAddr::from((host, port))).await {
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
        let host = IpAddr::from([127, 0, 0, 1]);
        let busy = bind(host, 41000).await.unwrap();
        let taken = busy.local_addr().unwrap().port();
        let next = bind(host, taken).await.unwrap();
        assert!(next.local_addr().unwrap().port() > taken);
    }
}
