//! Every test suite that drives Chrome, one module per suite, each on
//! [`chrome::testing`](crate::adapters::chrome::testing): the browser launched as
//! devpage launches it, a router served on loopback for it to load, and the
//! waits a test needs on a page. They are `#[ignore]`d; run them with
//! `cargo test -- --ignored`.

/// The canvas: painting on the GPU and the CPU against the server's PNG,
/// interactions, embedding and printing.
mod canvas;
/// The page script's side of comments.
mod comments;
/// The page script: widgets, live updates, navigation, the navbar, the
/// sidebar and its search, decks and themes.
mod page_script;
/// `devpage pdf`: what a doc and a deck print to.
mod pdf;
/// A published document: every request its pages make stays under their
/// mount.
mod server_mode;
/// Comments on a published document.
mod server_mode_comments;
/// A published document's pages: versions, the banner, the menu and the
/// download button.
mod server_mode_pages;
/// A slide's PNG.
mod slide_png;
/// `devpage video`: a deck recorded, with a stand-in voice and with
/// GPT-Live.
mod video;

use axum::Router;

use crate::drive::server_mode::{Config, router};

/// A zip of `files`, each `(name, text)`, as `devpage publish` sends one.
fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// `devpage server`'s router, its store and its bucket in a fresh folder
/// named for `name`.
fn server_mode(name: &str) -> Router {
    let dir =
        std::env::temp_dir().join(format!("devpage-server-mode-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bucket")).unwrap();
    let config = Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 30);
    router(&config).unwrap()
}

/// Publishes `zip` to the server at `base` — to document `id`, or as a new
/// one — and answers the document's id.
async fn publish(base: &str, id: Option<&str>, zip: Vec<u8>) -> String {
    let url = match id {
        Some(id) => format!("{base}/api/documents/{id}/versions"),
        None => format!("{base}/api/documents"),
    };
    let response = reqwest::Client::new()
        .post(url)
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let answer: serde_json::Value = response.json().await.unwrap();
    id.map(str::to_string)
        .unwrap_or_else(|| answer["id"].as_str().unwrap().to_string())
}
