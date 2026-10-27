//! The PVC is only a cache, checked on the example deploy: against each kind
//! of bucket, a server from `deploy/server/` is published to and commented
//! on, its volume is deleted, and a server started on an empty one — the
//! store restored by Litestream, the versions fetched from the bucket — finds
//! every document, version and comment. It runs `docker compose`, builds the
//! image from this checkout the first time, and starts both emulators.

use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

const COMPOSE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/deploy/server/compose.yaml");
const PROJECT: &str = "devpage-restore";

#[tokio::test]
#[ignore = "runs docker compose: builds the server image and starts the bucket emulators"]
async fn an_s3_server_on_an_empty_volume_restores_everything() {
    restores("devpage-s3", "data-s3", 3031).await;
}

#[tokio::test]
#[ignore = "runs docker compose: builds the server image and starts the bucket emulators"]
async fn an_azure_server_on_an_empty_volume_restores_everything() {
    restores("devpage-azure", "data-azure", 3032).await;
}

async fn restores(service: &str, volume: &str, port: u16) {
    let _down = Down;
    compose(&["up", "--build", "--detach", "--wait", service]);
    let server = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();

    // Two versions and a thread on the latest.
    let first = publish(&http, &format!("{server}/api/documents"), "first").await;
    let id = first["id"].as_str().unwrap().to_string();
    let second = publish(
        &http,
        &format!("{server}/api/documents/{id}/versions"),
        "second",
    )
    .await;
    assert_eq!(second["version"], 2);
    let thread = http
        .post(format!("{server}/api/documents/{id}/comments"))
        .json(&serde_json::json!({
            "page": "README.md",
            "anchor": {"quote": "every write", "prefix": "", "suffix": ""},
            "text": "Only the outbox?",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(thread.status(), 201, "{}", thread.text().await.unwrap());
    // Litestream ships the store's last writes about once a second.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // The PVC goes.
    compose(&["stop", service]);
    compose(&["rm", "--force", service]);
    docker(&["volume", "rm", &format!("{PROJECT}_{volume}")]);
    compose(&["up", "--detach", "--wait", service]);

    let document: serde_json::Value = get(&http, &format!("{server}/api/documents/{id}"))
        .await
        .json()
        .await
        .unwrap();
    let versions: Vec<u64> = document["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["number"].as_u64().unwrap())
        .collect();
    assert_eq!(versions, [1, 2], "{document}");
    let comments: serde_json::Value = get(&http, &format!("{server}/api/documents/{id}/comments"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(comments.as_array().unwrap().len(), 1, "{comments}");
    assert!(
        comments.to_string().contains("Only the outbox?"),
        "{comments}"
    );
    let latest = get(&http, &format!("{server}/d/{id}/"))
        .await
        .text()
        .await
        .unwrap();
    assert!(latest.contains("second"), "the latest is version 2");
    let pinned = get(&http, &format!("{server}/d/{id}/v/1/"))
        .await
        .text()
        .await
        .unwrap();
    assert!(pinned.contains("first"), "version 1 is still version 1");
}

/// A one-page site whose page says `word`, published to `url`.
async fn publish(http: &reqwest::Client, url: &str, word: &str) -> serde_json::Value {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("devpage.yml", options).unwrap();
    zip.write_all(b"kind: site\ntitle: Rollout\nroot: README.md\n")
        .unwrap();
    zip.start_file("README.md", options).unwrap();
    zip.write_all(
        format!("# Rollout\n\nThe {word} draft: the client retries every write.\n").as_bytes(),
    )
    .unwrap();
    let zip = zip.finish().unwrap().into_inner();
    let answer = http.post(url).body(zip).send().await.unwrap();
    assert_eq!(answer.status(), 201, "{}", answer.text().await.unwrap());
    answer.json().await.unwrap()
}

/// `url`, retried while the server is still starting.
async fn get(http: &reqwest::Client, url: &str) -> reqwest::Response {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match http.get(url).send().await {
            Ok(answer) if answer.status().is_success() => return answer,
            Ok(answer) if Instant::now() > deadline => panic!("{url}: {}", answer.status()),
            Err(error) if Instant::now() > deadline => panic!("{url}: {error}"),
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

fn compose(args: &[&str]) {
    let mut all = vec!["compose", "--project-name", PROJECT, "--file", COMPOSE];
    all.extend_from_slice(args);
    docker(&all);
}

fn docker(args: &[&str]) {
    let status = Command::new("docker").args(args).status().unwrap();
    assert!(status.success(), "docker {}", args.join(" "));
}

/// Takes the stack and its volumes down however the test ends.
struct Down;

impl Drop for Down {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["compose", "--project-name", PROJECT, "--file", COMPOSE])
            .args(["down", "--volumes"])
            .status();
    }
}
