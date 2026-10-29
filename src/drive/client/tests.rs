//! The server commands against a fake server answering the API as the
//! reference pins it, and `publish` against a real `devpage server`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use clap::Parser;

use super::contexts::{Home, test_home};
use super::{Command, run};
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

/// One request the fake server heard.
#[derive(Debug, Clone)]
struct Heard {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone, Default)]
struct Fake {
    heard: Arc<Mutex<Vec<Heard>>>,
}

const ID: &str = "6f1c2a9e-0000-4000-8000-000000000001";

async fn answer(State(fake): State<Fake>, request: Request) -> Response {
    let method = request.method().to_string();
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    fake.heard.lock().unwrap().push(Heard {
        method: method.clone(),
        path: path.clone(),
        authorization,
        body,
    });
    let route = path.split('?').next().unwrap_or_default();
    let json = |value: serde_json::Value| axum::Json(value).into_response();
    match (method.as_str(), route) {
        ("POST", "/api/documents") => (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({ "id": ID, "version": 1, "url": format!("/d/{ID}") })),
        )
            .into_response(),
        ("POST", r) if r == format!("/api/documents/{ID}/versions") => (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({ "version": 2, "url": format!("/d/{ID}") })),
        )
            .into_response(),
        ("GET", "/api/documents") => json(serde_json::json!([
            { "id": ID, "kind": "doc", "title": "Launch", "latest": 2 }
        ])),
        ("GET", r) if r == format!("/api/documents/{ID}") => json(serde_json::json!({
            "id": ID, "kind": "doc", "title": "Launch", "latest": 4,
            "versions": [
                { "number": 1, "author": "ana@example.com", "published": "2026-10-25T18:00:00Z", "size": 10 },
                { "number": 4, "author": "anonymous", "published": "2026-10-26T09:30:00Z", "size": 12 }
            ]
        })),
        ("GET", r) if r == format!("/api/documents/{ID}/comments") => json(serde_json::json!([
            {
                "id": "20261025T181502Z-7f3a", "page": "/chapters/rollout.md", "version": 3,
                "detached": false,
                "anchor": { "quote": "the client retries every write", "prefix": "On reconnect, ", "suffix": " until the server acknowledges it." },
                "thread": [
                    { "author": "Andrei Puni", "at": "2026-10-25T18:15:02Z", "text": "Every write, or only the ones still in the outbox?" },
                    { "author": "ana@…", "at": "2026-10-25T18:40:00Z", "text": "Only the outbox. I'll fix the sentence." }
                ]
            },
            {
                "id": "20261025T190344Z-0b91", "page": "/chapters/rollout.md", "version": 2,
                "detached": true, "detached_in": 3,
                "anchor": { "quote": "retries are capped at five", "prefix": "", "suffix": "" },
                "thread": [ { "author": "Bo", "at": "2026-10-25T19:03:44Z", "text": "Why five?" } ]
            }
        ])),
        ("GET", r) if r == format!("/d/{ID}/v/4/download") => {
            (StatusCode::OK, Bytes::from_static(b"%PDF-fake")).into_response()
        }
        ("POST", r) if r.ends_with("/replies") => StatusCode::CREATED.into_response(),
        ("DELETE", _) => StatusCode::NO_CONTENT.into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A fake server on a free port, and a home whose current context is it.
async fn fake(name: &str, token_command: Option<&str>) -> (Fake, Home) {
    let fake = Fake::default();
    let app = Router::new()
        .route("/{*rest}", any(answer))
        .route("/", get(|| async { "devpage" }))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let home = test_home(name);
    let add = Command::Context {
        action: super::ContextAction::Add {
            name: "fake".into(),
            server: format!("http://{address}"),
            token_command: token_command.map(str::to_string),
        },
    };
    run(add, &home, &here(), None).await.unwrap();
    run(command("context use fake"), &home, &here(), None)
        .await
        .unwrap();
    (fake, home)
}

/// The current folder, served: what the commands that read no document
/// are given.
fn here() -> ServedFolder {
    ServedFolder::new(Path::new(".")).unwrap()
}

async fn ok(home: &Home, served: &ServedFolder, line: &str) -> String {
    let output = run(command(line), home, served, None).await.unwrap();
    assert_eq!(output.code, 0, "{}", output.text);
    output.text
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
    let served = &here();
    ok(&home, served, "context add work https://docs.example.com").await;
    ok(&home, served, "context add lab http://localhost:3030").await;
    assert!(
        run(command("context use nope"), &home, served, None)
            .await
            .is_err()
    );
    assert!(
        run(
            command("context add bad docs.example.com"),
            &home,
            served,
            None
        )
        .await
        .is_err()
    );
    ok(&home, served, "context use work").await;
    assert_eq!(
        ok(&home, served, "context list").await,
        "  lab  http://localhost:3030\n* work  https://docs.example.com\n"
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, served, "context list --json").await).unwrap();
    assert_eq!(json["current-context"], "work");
}

/// The first publish creates the document and remembers it for this file
/// on this context; the next adds a version to it. The zip holds the site's
/// files and nothing beside them, and names the document file.
#[tokio::test]
async fn publish_remembers_the_document_per_context_and_sends_only_the_checked_files() {
    let dir = folder("remember");
    let served = &ServedFolder::new(&dir).unwrap();
    site(&dir);
    let (fake, home) = fake("remember", None).await;
    let first = ok(&home, served, &format!("publish {}/launch", dir.display())).await;
    assert!(first.starts_with("Launch — version 1\nhttp://"), "{first}");
    assert!(first.trim_end().ends_with(&format!("/d/{ID}")), "{first}");
    let second = ok(
        &home,
        served,
        &format!("publish {}/launch/devpage.yml", dir.display()),
    )
    .await;
    assert!(second.starts_with("Launch — version 2\n"), "{second}");

    let heard = fake.heard.lock().unwrap().clone();
    assert_eq!(heard[0].method, "POST");
    assert_eq!(heard[0].path, "/api/documents?entry=devpage.yml");
    assert_eq!(
        zip_names(&heard[0].body),
        ["README.md", "devpage.yml", "plan.md"]
    );
    assert_eq!(
        heard[1].path,
        format!("/api/documents/{ID}/versions?entry=devpage.yml")
    );
    let published = std::fs::read_to_string(home.0.join("contexts/fake/published.yml")).unwrap();
    assert!(
        published.contains(&format!(
            "{}: {ID}",
            dir.join("launch/devpage.yml").display()
        )),
        "{published}"
    );
}

#[tokio::test]
async fn id_publishes_to_that_document_and_new_always_creates_one() {
    let dir = folder("flags");
    let served = &ServedFolder::new(&dir).unwrap();
    site(&dir);
    let (fake, home) = fake("flags", None).await;
    ok(
        &home,
        served,
        &format!("publish {}/launch --id {ID}", dir.display()),
    )
    .await;
    ok(
        &home,
        served,
        &format!("publish {}/launch --new", dir.display()),
    )
    .await;
    let paths: Vec<String> = fake
        .heard
        .lock()
        .unwrap()
        .iter()
        .map(|h| h.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            format!("/api/documents/{ID}/versions?entry=devpage.yml"),
            "/api/documents?entry=devpage.yml".to_string(),
        ]
    );
    let error = run(
        command(&format!("publish {}/launch --id gone", dir.display())),
        &home,
        served,
        None,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("--new"), "{error}");
    assert!(
        Cli::try_parse_from(["devpage", "publish", "x", "--id", ID, "--new"]).is_err(),
        "--id and --new together"
    );
}

/// A broken link stops the publish before any request, and says so.
#[tokio::test]
async fn a_broken_link_stops_publish_before_any_request() {
    let dir = folder("broken");
    let served = &ServedFolder::new(&dir).unwrap();
    site(&dir);
    std::fs::write(dir.join("launch/plan.md"), "# Plan\n\n[Gone](gone.md)\n").unwrap();
    let (fake, home) = fake("broken", None).await;
    let output = run(
        command(&format!("publish {}/launch", dir.display())),
        &home,
        served,
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
        served,
        None,
    )
    .await
    .unwrap();
    let problems: serde_json::Value = serde_json::from_str(&json.text).unwrap();
    assert_eq!(problems["problems"].as_array().unwrap().len(), 1);
    assert!(fake.heard.lock().unwrap().is_empty());
}

/// The token command runs before each request, and what it prints is the
/// bearer token; devpage stores none.
#[tokio::test]
async fn the_token_commands_output_is_the_bearer_token() {
    let (fake, home) = fake("token", Some("echo secret-token")).await;
    ok(&home, &here(), "list").await;
    let heard = fake.heard.lock().unwrap().clone();
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

#[tokio::test]
async fn list_and_versions_print_text_and_json() {
    let (_, home) = fake("list", None).await;
    let served = &here();
    let list = ok(&home, served, "list").await;
    assert!(
        list.starts_with(&format!("{ID}  doc  Launch  v2  http://")),
        "{list}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, served, "list --json").await).unwrap();
    assert_eq!(json[0]["id"], ID);
    assert_eq!(
        ok(&home, served, &format!("versions {ID}")).await,
        "v1  2026-10-25T18:00:00Z  ana@example.com\nv4  2026-10-26T09:30:00Z  anonymous\n"
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, served, &format!("versions {ID} --json")).await).unwrap();
    assert_eq!(json[1]["number"], 4);
}

/// Comments group by page, each thread with its quote in context and its
/// messages; a detached one is marked so, with the version its text left —
/// not the latest, which is 4 here.
#[tokio::test]
async fn comments_print_as_the_reading_page_shows_them() {
    let (fake, home) = fake("comments", None).await;
    let served = &here();
    assert_eq!(
        ok(&home, served, &format!("comments {ID}")).await,
        "chapters/rollout.md\n\
         \x20 20261025T181502Z-7f3a  on v3  \"the client retries every write\"\n\
         \x20   …On reconnect, [the client retries every write] until the server acknowledges it.\n\
         \x20   Andrei Puni  18:15  Every write, or only the ones still in the outbox?\n\
         \x20   ana@…        18:40  Only the outbox. I'll fix the sentence.\n\
         \x20 20261025T190344Z-0b91  detached in v3  \"retries are capped at five\"\n"
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, served, &format!("comments {ID} --json")).await).unwrap();
    assert_eq!(json[1]["detached"], true);
    assert_eq!(json[0]["page"], "/chapters/rollout.md");

    let reply = run(
        Command::Comments(super::CommentsArgs {
            action: Some(super::CommentAction::Reply {
                id: ID.into(),
                thread: "20261025T181502Z-7f3a".into(),
                text: "Fixed in v5".into(),
                remote: Default::default(),
            }),
            id: None,
            remote: Default::default(),
        }),
        &home,
        served,
        None,
    )
    .await
    .unwrap();
    assert_eq!(reply.text, "replied to 20261025T181502Z-7f3a\n");
    ok(
        &home,
        served,
        &format!("comments resolve {ID} 20261025T181502Z-7f3a"),
    )
    .await;
    let heard = fake.heard.lock().unwrap().clone();
    let replied = heard.iter().find(|h| h.path.ends_with("/replies")).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&replied.body).unwrap(),
        serde_json::json!({ "text": "Fixed in v5" })
    );
    assert!(heard.iter().any(|h| h.method == "DELETE"
        && h.path == format!("/api/documents/{ID}/comments/20261025T181502Z-7f3a")));
}

#[tokio::test]
async fn download_saves_the_latest_versions_pdf_and_delete_names_what_it_deletes() {
    let dir = folder("download");
    let served = &ServedFolder::new(&dir).unwrap();
    let (fake, home) = fake("download", None).await;
    let file = dir.join("out.pdf");
    let text = ok(
        &home,
        served,
        &format!("download {ID} -o {}", file.display()),
    )
    .await;
    assert_eq!(text, format!("{}\n", file.display()));
    assert_eq!(std::fs::read(&file).unwrap(), b"%PDF-fake");
    assert_eq!(
        ok(&home, served, &format!("delete {ID} --version 3")).await,
        format!("deleted version 3 of {ID}\n")
    );
    let json: serde_json::Value =
        serde_json::from_str(&ok(&home, served, &format!("delete {ID} --json")).await).unwrap();
    assert_eq!(json["deleted"], true);
    let heard = fake.heard.lock().unwrap().clone();
    assert!(
        heard
            .iter()
            .any(|h| h.path == format!("/api/documents/{ID}/versions/3"))
    );
    assert!(
        heard
            .iter()
            .any(|h| h.method == "DELETE" && h.path == format!("/api/documents/{ID}"))
    );
}

#[tokio::test]
async fn an_unknown_document_and_no_context_are_errors_that_say_what_to_do() {
    let (_, home) = fake("unknown", None).await;
    let error = run(command("versions nope"), &home, &here(), None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not found"), "{error}");
    let empty = test_home("no-context");
    let error = run(command("list"), &empty, &here(), None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("devpage context add"), "{error}");
    // DEVPAGE_CONTEXT, passed in, picks a context that isn't current.
    let error = run(command("list"), &home, &here(), Some("other".into()))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "no context named other");
}

/// Against a real `devpage server` on a `file://` bucket: the first publish
/// creates the document, the second adds version 2, and its page renders.
#[tokio::test]
async fn publish_creates_and_updates_a_document_on_a_real_server() {
    let dir = folder("real");
    let served = &ServedFolder::new(&dir).unwrap();
    site(&dir);
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let config = crate::drive::server_mode::Config::local(&data, &dir.join("bucket"), 1 << 20);
    std::fs::create_dir_all(dir.join("bucket")).unwrap();
    let app = crate::drive::server_mode::router(&config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let home = test_home("real");
    ok(
        &home,
        served,
        &format!("context add local http://{address}"),
    )
    .await;
    ok(&home, served, "context use local").await;

    let first = ok(
        &home,
        served,
        &format!("publish {}/launch --json", dir.display()),
    )
    .await;
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["version"], 1);
    std::fs::write(dir.join("launch/plan.md"), "# Plan, revised\n").unwrap();
    let second: serde_json::Value = serde_json::from_str(
        &ok(
            &home,
            served,
            &format!("publish {}/launch --json", dir.display()),
        )
        .await,
    )
    .unwrap();
    assert_eq!(second["version"], 2);
    assert_eq!(second["id"], first["id"]);
    let id = first["id"].as_str().unwrap();
    let kept = dir.join(format!("bucket/documents/{id}/versions/2.zip"));
    assert_eq!(
        zip_names(&std::fs::read(kept).unwrap()),
        ["README.md", "devpage.yml", "plan.md"]
    );
    // The link renders the new version's pages.
    let link = second["url"].as_str().unwrap();
    let client = reqwest::Client::new();
    let page = client.get(link).send().await.unwrap();
    assert_eq!(page.status(), reqwest::StatusCode::OK);
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
