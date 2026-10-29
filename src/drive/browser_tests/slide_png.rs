//! A slide's PNG, shot by chrome-headless-shell: drawn without its comments
//! and its problems, without waiting for a design on another slide, never shooting itself,
//! kept until a file its deck is drawn from changes, and Chrome closed and
//! started again.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::adapters::changes::Changes;
use crate::adapters::chrome::testing;
use crate::drive::http;
use crate::drive::slide_png::Shots;
use devpage_core::document::Document;
use devpage_core::folder::ServedFolder;
use devpage_core::watch::SlidePngs;

/// A design that paints.
const BOX: &str = "<Frame width={400} height={300} fill=\"#2563eb\" />";

/// A folder of `files`, served with `talk.yml` as its entry; its address,
/// the folder, and the slide PNGs, to tell of a changed file as the watcher
/// would.
async fn deck(name: &str, files: &[(&str, &str)]) -> (String, PathBuf, Shots) {
    let dir = std::env::temp_dir().join(format!("devpage-shots-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    let dir = dir.canonicalize().unwrap();
    let document =
        Arc::new(Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap());
    let app = http::App::testing(document, Changes::default())
        .with_comments_on_disk(devpage_core::comments::Identity::Local("T".into()));
    let shots = app.shots();
    (testing::serve(http::router(app)).await, dir, shots)
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_is_shot_without_its_comments() {
    let (address, dir, shots) = deck(
        "comments",
        &[
            ("one.md", "# One\n\nThe client retries every write.\n"),
            ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
        ],
    )
    .await;
    let url = format!("{address}/talk.yml/1.png");
    let before = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
    let client = reqwest::Client::new();
    let made = client
        .post(format!("{address}/__devpage/comments"))
        .json(&serde_json::json!({
            "page": "/one.md",
            "anchor": { "quote": "retries", "prefix": "The client ", "suffix": " every write." },
            "text": "Bounded?",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(made.status(), 200);
    // Drawn again, as it is now: the thread is not on it.
    shots.changed(&[dir.join("one.md")]);
    let after = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
    assert!(after == before, "the comment shows in the slide's PNG");
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_is_shot_without_its_problems() {
    let (address, dir, shots) = deck(
        "problems",
        &[
            ("one.md", "# One\n\nSee [the plan](plan.md) first.\n"),
            ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
        ],
    )
    .await;
    // The deck's page lists the broken link, and marks it …
    let page = reqwest::get(format!("{address}/talk.yml"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("broken link plan.md"), "{page}");
    // … but its slide's PNG shows neither: it is the slide drawn once the
    // link is fixed.
    let url = format!("{address}/talk.yml/1.png");
    let broken = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
    std::fs::write(dir.join("plan.md"), "# Plan\n").unwrap();
    shots.changed(&[dir.join("one.md")]);
    let fixed = reqwest::get(&url).await.unwrap().bytes().await.unwrap();
    assert!(broken == fixed, "the problem shows in the slide's PNG");
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_beside_a_design_is_drawn_without_waiting_for_it() {
    let (address, _, _) = deck(
        "beside",
        &[
            ("box.design", BOX),
            ("one.md", "# One\n"),
            ("two.md", "# Two\n\n![The box](box.design)\n"),
            (
                "talk.yml",
                "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
            ),
        ],
    )
    .await;
    // The design on slide 2 is hidden on slide 1: not waited for.
    let started = Instant::now();
    let first = reqwest::get(format!("{address}/talk.yml/1.png"))
        .await
        .unwrap();
    assert_eq!(first.status(), 200);
    assert!(first.headers().get("x-devpage-ready").is_none());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    // On its own slide it is painted before the shot.
    let second = reqwest::get(format!("{address}/talk.yml/2.png"))
        .await
        .unwrap();
    assert_eq!(second.status(), 200);
    assert!(second.headers().get("x-devpage-ready").is_none());
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_showing_its_own_png_returns_promptly() {
    let (address, _, _) = deck(
        "itself",
        &[
            ("one.md", "# One\n\n![Itself](talk.yml/1.png)\n"),
            ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
        ],
    )
    .await;
    let started = Instant::now();
    let response = reqwest::get(format!("{address}/talk.yml/1.png"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_repeat_request_comes_from_the_cache_until_a_file_changes() {
    let (address, dir, shots) = deck(
        "cache",
        &[
            ("one.md", "# One\n"),
            ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
        ],
    )
    .await;
    let url = format!("{address}/talk.yml/1.png");
    let client = reqwest::Client::new();
    let shot = |held: Option<String>, inside: bool| {
        let mut request = client.get(&url);
        if let Some(etag) = held {
            request = request.header("if-none-match", etag);
        }
        if inside {
            request = request.header(crate::adapters::chrome::SHOT_HEADER, "1");
        }
        request.send()
    };
    // Inside another shot, a slide not yet drawn isn't drawn.
    assert_eq!(shot(None, true).await.unwrap().status(), 508);

    let first = shot(None, false).await.unwrap();
    let etag = first.headers()["etag"].to_str().unwrap().to_string();
    let first = first.bytes().await.unwrap();

    // Drawn already: no Chrome, and nothing sent to a holder of it.
    let started = Instant::now();
    let again = shot(Some(etag.clone()), false).await.unwrap();
    assert_eq!(again.status(), 304);
    assert_eq!(again.headers()["etag"], etag.as_str());
    let cached = shot(None, true).await.unwrap();
    assert_eq!(cached.status(), 200);
    assert_eq!(cached.bytes().await.unwrap(), first);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "{:?}",
        started.elapsed()
    );

    // A change: the slide is drawn again, as it is now.
    std::fs::write(dir.join("one.md"), "# Changed\n").unwrap();
    shots.changed(&[dir.join("one.md")]);
    let changed = shot(Some(etag.clone()), false).await.unwrap();
    assert_eq!(changed.status(), 200);
    assert_ne!(changed.headers()["etag"], etag.as_str());
    assert_ne!(changed.bytes().await.unwrap(), first);
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn chrome_closes_when_told_and_starts_again_for_the_next_shot() {
    let shots = Shots::new();
    let dir = std::env::temp_dir().join(format!("devpage-shots-{}-close", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.md"), "# One\n").unwrap();
    std::fs::write(
        dir.join("talk.yml"),
        "kind: slides\ntitle: Talk\nslides: [one.md]\n",
    )
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let document =
        Arc::new(Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap());
    let serve = || http::router(http::App::printing(document.clone()));
    let deck = dir.join("talk.yml");
    let drawn_from = |deck: &PathBuf| {
        let deck = deck.clone();
        move || vec![deck]
    };
    shots
        .png("/talk.yml", 1, drawn_from(&deck), serve)
        .await
        .unwrap();
    assert!(shots.running().await);
    shots.close().await;
    assert!(!shots.running().await);
    shots.changed(std::slice::from_ref(&deck));
    shots
        .png("/talk.yml", 1, drawn_from(&deck), serve)
        .await
        .unwrap();
    assert!(shots.running().await);
    shots.close().await;
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slide_is_served_as_a_1920_by_1080_png() {
    let dir = std::env::temp_dir().join(format!("devpage-slide-png-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.md"), "# One\n").unwrap();
    std::fs::write(dir.join("two.md"), "# Two\n\n<Notes>Never drawn.</Notes>\n").unwrap();
    std::fs::write(
        dir.join("talk.yml"),
        "kind: slides\ntitle: Talk\nslides: [one.md, two.md]\n",
    )
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let document =
        Arc::new(Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap());
    let changes = Changes::default();
    let address = testing::serve(http::router(
        http::App::testing(document, changes)
            .with_comments_on_disk(devpage_core::comments::Identity::Local("T".into())),
    ))
    .await;

    let response = reqwest::get(format!("{address}/talk.yml/2.png"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "image/png");
    let png = response.bytes().await.unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // The IHDR chunk's width and height, big-endian.
    let size = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    assert_eq!((size(16), size(20)), (1920, 1080));

    // Slide 1 and slide 2 are different pictures.
    let first = reqwest::get(format!("{address}/talk.yml/1.png"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_ne!(first, png);

    for path in ["talk.yml/3.png", "talk.yml/0.png", "one.md/1.png"] {
        let status = reqwest::get(format!("{address}/{path}"))
            .await
            .unwrap()
            .status();
        assert_eq!(status, 404, "{path}");
    }
}
