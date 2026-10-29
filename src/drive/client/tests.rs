//! The server commands against a real `devpage server` on a `file://`
//! bucket, with every request it hears recorded — so the CLI is held to the
//! requests it sends and to the answers the server really gives.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use clap::Parser;
use devpage_core::comments::{Anchor, Message};

use super::api::refused;
use super::contexts::{Home, test_home};
use super::{Command, run, threads_text};
use crate::drive::api_types::{NewThread, Refusal, Started, Thread};
use crate::drive::server_mode::{Config, router};
use crate::served::ServedFolder;

/// The CLI's own arguments, for parsing a command line in a test.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn command(line: &str) -> Command {
    Cli::try_parse_from(std::iter::once("devpage").chain(line.split_whitespace()))
        .unwrap()
        .command
}

/// One request the server heard.
#[derive(Debug, Clone)]
struct Heard {
    method: String,
    /// Its path and query.
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

/// A real server on a free port, and every request it heard.
struct Server {
    address: String,
    bucket: PathBuf,
    heard: Arc<Mutex<Vec<Heard>>>,
}

impl Server {
    fn heard(&self) -> Vec<Heard> {
        self.heard.lock().unwrap().clone()
    }

    /// What the server answers `GET path`, as JSON — read past the CLI.
    async fn json(&self, path: &str) -> serde_json::Value {
        reqwest::get(format!("{}{path}", self.address))
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    /// Starts a thread on `page` of document `id`, as a page would.
    async fn comment(&self, id: &str, page: &str, anchor: Anchor, text: &str) -> Started {
        let new = NewThread {
            page: page.into(),
            anchor,
            text: text.into(),
        };
        let answer = reqwest::Client::new()
            .post(format!("{}/api/documents/{id}/comments", self.address))
            .json(&new)
            .send()
            .await
            .unwrap();
        assert_eq!(answer.status(), reqwest::StatusCode::CREATED);
        answer.json().await.unwrap()
    }
}

/// Keeps each request — method, path and query, token, body — then passes
/// it on unchanged.
async fn record(
    State(heard): State<Arc<Mutex<Vec<Heard>>>>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    heard.lock().unwrap().push(Heard {
        method: parts.method.to_string(),
        path: parts
            .uri
            .path_and_query()
            .map(|p| p.to_string())
            .unwrap_or_default(),
        authorization: parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        body: body.to_vec(),
    });
    next.run(Request::from_parts(parts, Body::from(body))).await
}

/// `devpage server` with its store and bucket in `dir`, and a home whose
/// current context is it.
async fn server(dir: &Path, token_command: Option<&str>) -> (Server, Home) {
    let bucket = dir.join("server/bucket");
    let config = Config::local(&dir.join("server/data"), &bucket, 1 << 20);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let app = router(&config)
        .unwrap()
        .layer(middleware::from_fn_with_state(heard.clone(), record));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let home = test_home(&dir.file_name().unwrap().to_string_lossy());
    let add = Command::Context {
        action: super::ContextAction::Add {
            name: "real".into(),
            server: address.clone(),
            token_command: token_command.map(str::to_string),
        },
    };
    run(add, &home, &served(dir), None).await.unwrap();
    run(command("context use real"), &home, &served(dir), None)
        .await
        .unwrap();
    let server = Server {
        address,
        bucket,
        heard,
    };
    (server, home)
}

/// `dir`, served: what a command reading a document reads it in.
fn served(dir: &Path) -> ServedFolder {
    ServedFolder::new(dir).unwrap()
}

async fn ok(home: &Home, dir: &Path, line: &str) -> String {
    let output = run(command(line), home, &served(dir), None).await.unwrap();
    assert_eq!(output.code, 0, "{}", output.text);
    output.text
}

/// Publishes `path` in `dir`: the document's id.
async fn publish(home: &Home, dir: &Path, path: &str) -> String {
    let answer = ok(
        home,
        dir,
        &format!("publish {}/{path} --json", dir.display()),
    )
    .await;
    let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
    answer["id"].as_str().unwrap().to_string()
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A site in `dir/launch`, with a draft and a secret beside it.
fn site(dir: &Path) {
    std::fs::create_dir_all(dir.join("launch")).unwrap();
    std::fs::write(
        dir.join("launch/devpage.yml"),
        "kind: site\ntitle: Launch\nroot: README.md\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("launch/README.md"),
        "# Launch\n\n[Plan](plan.md)\n",
    )
    .unwrap();
    std::fs::write(dir.join("launch/plan.md"), "# Plan\n").unwrap();
    std::fs::write(dir.join("launch/draft.md"), "# Draft\n").unwrap();
    std::fs::write(dir.join("launch/secret.txt"), "hunter2").unwrap();
}

fn zip_names(zip: &[u8]) -> Vec<String> {
    let archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
    names.sort();
    names
}

#[tokio::test]
async fn contexts_are_added_used_and_listed_with_the_current_one_marked() {
    let home = test_home("contexts");
    let here = Path::new(".");
    ok(&home, here, "context add work https://docs.example.com").await;
    ok(&home, here, "context add lab http://localhost:3030").await;
    assert!(
        run(command("context use nope"), &home, &served(here), None)
            .await
            .is_err()
    );
    assert!(
        run(
            command("context add bad docs.example.com"),
            &home,
            &served(here),
            None
        )
        .await
        .is_err()
    );
    ok(&home, here, "context use work").await;
    assert_eq!(
        ok(&home, here, "context list").await,
        "  lab  http://localhost:3030\n* work  https://docs.example.com\n"
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, here, "context list --json").await).unwrap();
    assert_eq!(json["current-context"], "work");
}

/// The first publish creates the document and remembers it for this file
/// on this context; the next adds a version to it. The zip holds the site's
/// files and nothing beside them, names the document file, and is what the
/// bucket keeps; the link renders the new version.
#[tokio::test]
async fn publish_remembers_the_document_per_context_and_sends_only_the_checked_files() {
    let dir = folder("remember");
    site(&dir);
    let (server, home) = server(&dir, None).await;
    let first = ok(&home, &dir, &format!("publish {}/launch", dir.display())).await;
    let link = first
        .strip_prefix("Launch — version 1\n")
        .unwrap_or_else(|| panic!("{first}"))
        .trim_end();
    let id = link
        .strip_prefix(&format!("{}/d/", server.address))
        .unwrap_or_else(|| panic!("{first}"));
    std::fs::write(dir.join("launch/plan.md"), "# Plan, revised\n").unwrap();
    let second = ok(
        &home,
        &dir,
        &format!("publish {}/launch/devpage.yml", dir.display()),
    )
    .await;
    assert_eq!(second, format!("Launch — version 2\n{link}\n"));

    let heard = server.heard();
    assert_eq!(heard[0].method, "POST");
    assert_eq!(heard[0].path, "/api/documents?entry=devpage.yml");
    assert_eq!(
        zip_names(&heard[0].body),
        ["README.md", "devpage.yml", "plan.md"]
    );
    assert_eq!(
        heard[1].path,
        format!("/api/documents/{id}/versions?entry=devpage.yml")
    );
    let kept = server.bucket.join(format!("documents/{id}/versions/2.zip"));
    assert_eq!(std::fs::read(kept).unwrap(), heard[1].body);
    let published = std::fs::read_to_string(home.0.join("contexts/real/published.yml")).unwrap();
    assert!(
        published.contains(&format!(
            "{}: {id}",
            dir.join("launch/devpage.yml").display()
        )),
        "{published}"
    );
    let page = reqwest::get(format!("{link}/plan.md")).await.unwrap();
    assert_eq!(page.status(), reqwest::StatusCode::OK);
    assert!(page.text().await.unwrap().contains("Plan, revised"));
}

/// `--id` publishes to that document, `--new` always creates one, and an
/// id the server doesn't know says how to create one instead.
#[tokio::test]
async fn id_publishes_to_that_document_and_new_always_creates_one() {
    let dir = folder("flags");
    site(&dir);
    let (server, home) = server(&dir, None).await;
    let id = publish(&home, &dir, "launch --new").await;
    ok(
        &home,
        &dir,
        &format!("publish {}/launch --id {id}", dir.display()),
    )
    .await;
    let other = publish(&home, &dir, "launch --new").await;
    assert_ne!(other, id);
    let paths: Vec<String> = server.heard().into_iter().map(|h| h.path).collect();
    assert_eq!(
        paths,
        [
            "/api/documents?entry=devpage.yml".to_string(),
            format!("/api/documents/{id}/versions?entry=devpage.yml"),
            "/api/documents?entry=devpage.yml".to_string(),
        ]
    );
    for gone in ["gone", "6f1c2a9e-0000-4000-8000-000000000001"] {
        let error = run(
            command(&format!("publish {}/launch --id {gone}", dir.display())),
            &home,
            &served(&dir),
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("--new"), "{error}");
    }
    assert!(
        Cli::try_parse_from(["devpage", "publish", "x", "--id", "y", "--new"]).is_err(),
        "--id and --new together"
    );
}

/// A broken link stops the publish before any request, and says so.
#[tokio::test]
async fn a_broken_link_stops_publish_before_any_request() {
    let dir = folder("broken");
    site(&dir);
    std::fs::write(dir.join("launch/plan.md"), "# Plan\n\n[Gone](gone.md)\n").unwrap();
    let (server, home) = server(&dir, None).await;
    let output = run(
        command(&format!("publish {}/launch", dir.display())),
        &home,
        &served(&dir),
        None,
    )
    .await
    .unwrap();
    assert_eq!(output.code, 1);
    assert!(
        output.text.contains("broken link gone.md"),
        "{}",
        output.text
    );
    assert!(
        output.text.ends_with("nothing published\n"),
        "{}",
        output.text
    );
    let json = run(
        command(&format!("publish {}/launch --json", dir.display())),
        &home,
        &served(&dir),
        None,
    )
    .await
    .unwrap();
    let problems: serde_json::Value = serde_json::from_str(&json.text).unwrap();
    assert_eq!(problems["problems"].as_array().unwrap().len(), 1);
    assert!(server.heard().is_empty());
}

/// The token command runs before each request, and what it prints is the
/// bearer token; devpage stores none.
#[tokio::test]
async fn the_token_commands_output_is_the_bearer_token() {
    let dir = folder("token");
    let (server, home) = server(&dir, Some("echo secret-token")).await;
    ok(&home, &dir, "list").await;
    let heard = server.heard();
    assert_eq!(
        heard[0].authorization.as_deref(),
        Some("Bearer secret-token")
    );
    let config = std::fs::read_to_string(home.0.join("config.yml")).unwrap();
    assert!(
        !config
            .replace("echo secret-token", "")
            .contains("secret-token"),
        "{config}"
    );
}

/// `list` and `versions` print what the server answers; with `--json`
/// they print it unchanged.
#[tokio::test]
async fn list_and_versions_print_text_and_json() {
    let dir = folder("list");
    site(&dir);
    let (server, home) = server(&dir, None).await;
    let id = publish(&home, &dir, "launch").await;
    publish(&home, &dir, "launch").await;
    assert_eq!(
        ok(&home, &dir, "list").await,
        format!("{id}  site  Launch  v2  {}/d/{id}\n", server.address)
    );
    let listed: serde_json::Value =
        serde_json::from_str(&ok(&home, &dir, "list --json").await).unwrap();
    assert_eq!(listed, server.json("/api/documents").await);

    let described = server.json(&format!("/api/documents/{id}")).await;
    let versions = &described["versions"];
    assert_eq!(
        ok(&home, &dir, &format!("versions {id}")).await,
        format!(
            "v1  {}  anonymous\nv2  {}  anonymous\n",
            versions[0]["published"].as_str().unwrap(),
            versions[1]["published"].as_str().unwrap()
        )
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, &dir, &format!("versions {id} --json")).await).unwrap();
    assert_eq!(&json, versions);
}

/// The threads the server lists, grouped by page; a detached one is marked
/// with the version whose publish found its text gone — not the latest.
/// A reply and a resolve reach the thread.
#[tokio::test]
async fn comments_are_listed_replied_to_and_resolved_on_the_server() {
    let dir = folder("comments");
    site(&dir);
    std::fs::write(
        dir.join("launch/plan.md"),
        "# Plan\n\nThe client retries every write.\n\nRetries are capped at five.\n",
    )
    .unwrap();
    let (server, home) = server(&dir, None).await;
    let id = publish(&home, &dir, "launch").await;
    let anchor = |quote: &str| Anchor {
        quote: quote.into(),
        prefix: String::new(),
        suffix: String::new(),
    };
    let kept = server
        .comment(&id, "/plan.md", anchor("retries every write"), "Every one?")
        .await;
    let capped = server
        .comment(&id, "/plan.md", anchor("capped at five"), "Why five?")
        .await;
    std::fs::write(
        dir.join("launch/plan.md"),
        "# Plan\n\nThe client retries every write.\n",
    )
    .unwrap();
    publish(&home, &dir, "launch").await;
    std::fs::write(
        dir.join("launch/plan.md"),
        "# Plan\n\nThe client retries every write.\n\nDone.\n",
    )
    .unwrap();
    publish(&home, &dir, "launch").await;

    let listed = server.json(&format!("/api/documents/{id}/comments")).await;
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, &dir, &format!("comments {id} --json")).await).unwrap();
    assert_eq!(json, listed);
    let threads: Vec<Thread> = serde_json::from_value(listed).unwrap();
    assert_eq!(threads[1].id, capped.id);
    assert_eq!(threads[1].detached_in, Some(2));
    let text = ok(&home, &dir, &format!("comments {id}")).await;
    assert_eq!(text, threads_text(&threads, 3));
    assert!(text.starts_with("plan.md\n"), "{text}");
    assert!(
        text.contains(&format!(
            "  {}  detached in v2  \"capped at five\"\n",
            capped.id
        )),
        "{text}"
    );

    assert_eq!(
        ok(
            &home,
            &dir,
            &format!("comments reply {id} {} Agreed", kept.id)
        )
        .await,
        format!("replied to {}\n", kept.id)
    );
    let replied = server
        .heard()
        .into_iter()
        .find(|h| h.path.ends_with("/replies"))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&replied.body).unwrap(),
        serde_json::json!({ "text": "Agreed" })
    );
    let threads: Vec<Thread> =
        serde_json::from_value(server.json(&format!("/api/documents/{id}/comments")).await)
            .unwrap();
    assert_eq!(threads[0].thread.len(), 2);
    assert_eq!(threads[0].thread[1].text, "Agreed");

    ok(&home, &dir, &format!("comments resolve {id} {}", kept.id)).await;
    assert!(
        server.heard().iter().any(|h| h.method == "DELETE"
            && h.path == format!("/api/documents/{id}/comments/{}", kept.id))
    );
    let threads: Vec<Thread> =
        serde_json::from_value(server.json(&format!("/api/documents/{id}/comments")).await)
            .unwrap();
    assert_eq!(
        threads.iter().map(|t| &t.id).collect::<Vec<_>>(),
        [&capped.id]
    );
}

/// A design's download is its PNG, painted by the server; delete names
/// what it deleted, and the server no longer has it.
#[tokio::test]
async fn download_saves_the_latest_versions_png_and_delete_names_what_it_deletes() {
    let dir = folder("download");
    std::fs::write(
        dir.join("box.design"),
        "<Frame width={40} height={40} fill=\"#2563eb\" />\n",
    )
    .unwrap();
    let (server, home) = server(&dir, None).await;
    let id = publish(&home, &dir, "box.design").await;
    publish(&home, &dir, "box.design").await;
    let file = dir.join("out.png");
    let text = ok(&home, &dir, &format!("download {id} -o {}", file.display())).await;
    assert_eq!(text, format!("{}\n", file.display()));
    assert!(std::fs::read(&file).unwrap().starts_with(b"\x89PNG"));
    assert!(
        server
            .heard()
            .iter()
            .any(|h| h.path == format!("/d/{id}/v/2/download"))
    );

    assert_eq!(
        ok(&home, &dir, &format!("delete {id} --version 1")).await,
        format!("deleted version 1 of {id}\n")
    );
    assert!(
        ok(&home, &dir, &format!("versions {id}"))
            .await
            .starts_with("v2  ")
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, &dir, &format!("delete {id} --json")).await).unwrap();
    assert_eq!(json["deleted"], true);
    assert_eq!(ok(&home, &dir, "list").await, "");
    let heard = server.heard();
    assert!(
        heard
            .iter()
            .any(|h| h.method == "DELETE" && h.path == format!("/api/documents/{id}/versions/1"))
    );
    assert!(
        heard
            .iter()
            .any(|h| h.method == "DELETE" && h.path == format!("/api/documents/{id}"))
    );
}

/// An unknown document is the server's typed refusal, and says where it
/// was looked for; no context, or an unknown one, says what to do.
#[tokio::test]
async fn an_unknown_document_and_no_context_are_errors_that_say_what_to_do() {
    let dir = folder("unknown");
    let (server, home) = server(&dir, None).await;
    let error = run(
        command("versions 6f1c2a9e-0000-4000-8000-000000000001"),
        &home,
        &served(&dir),
        None,
    )
    .await
    .unwrap_err();
    assert!(refused(&error, &Refusal::NotFound), "{error}");
    assert_eq!(
        error.to_string(),
        format!(
            "6f1c2a9e-0000-4000-8000-000000000001: not found on {}",
            server.address
        )
    );
    let empty = test_home("no-context");
    let error = run(command("list"), &empty, &served(&dir), None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("devpage context add"), "{error}");
    // DEVPAGE_CONTEXT, passed in, picks a context that isn't current.
    let error = run(command("list"), &home, &served(&dir), Some("other".into()))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "no context named other");
}

/// The threads as text: each page once, each thread's id, where it is and
/// its quote; an attached one's quote in its context and its messages,
/// authors aligned.
#[test]
fn threads_print_as_the_reading_page_shows_them() {
    let message = |author: &str, at: &str, text: &str| Message {
        author: author.into(),
        at: at.into(),
        text: text.into(),
    };
    let threads = [
        Thread {
            id: "20261025T181502Z-7f3a".into(),
            page: "/chapters/rollout.md".into(),
            version: 3,
            detached: false,
            detached_in: None,
            anchor: Anchor {
                quote: "the client retries every write".into(),
                prefix: "On reconnect, ".into(),
                suffix: " until the server acknowledges it.".into(),
            },
            thread: vec![
                message(
                    "Andrei Puni",
                    "2026-10-25T18:15:02Z",
                    "Every write, or only the ones still in the outbox?",
                ),
                message(
                    "ana@…",
                    "2026-10-25T18:40:00Z",
                    "Only the outbox. I'll fix the sentence.",
                ),
            ],
        },
        Thread {
            id: "20261025T190344Z-0b91".into(),
            page: "/chapters/rollout.md".into(),
            version: 2,
            detached: true,
            detached_in: Some(3),
            anchor: Anchor {
                quote: "retries are capped at five".into(),
                prefix: String::new(),
                suffix: String::new(),
            },
            thread: vec![message("Bo", "2026-10-25T19:03:44Z", "Why five?")],
        },
    ];
    assert_eq!(
        threads_text(&threads, 4),
        "chapters/rollout.md\n\
         \x20 20261025T181502Z-7f3a  on v3  \"the client retries every write\"\n\
         \x20   …On reconnect, [the client retries every write] until the server acknowledges it.\n\
         \x20   Andrei Puni  18:15  Every write, or only the ones still in the outbox?\n\
         \x20   ana@…        18:40  Only the outbox. I'll fix the sentence.\n\
         \x20 20261025T190344Z-0b91  detached in v3  \"retries are capped at five\"\n"
    );
}

#[test]
fn every_server_command_parses_as_the_reference_writes_it() {
    for line in [
        "context add work https://docs.example.com --token-command az",
        "context use work",
        "context list",
        "publish launch/devpage.yml --id x",
        "publish launch --new --context work --json",
        "list --json",
        "versions x",
        "comments x --json",
        "comments reply x t hello",
        "comments resolve x t --context work",
        "download x --version 3 -o a.pdf",
        "delete x --version 3",
        "delete x",
    ] {
        Cli::try_parse_from(std::iter::once("devpage").chain(line.split_whitespace()))
            .unwrap_or_else(|e| panic!("{line}: {e}"));
    }
}
