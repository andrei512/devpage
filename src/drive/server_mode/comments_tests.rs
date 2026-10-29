//! A published document's comments through the server: the API, the page's
//! own comment requests, authors, re-anchoring on publish, and deletion.

use std::io::Write;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::{Config, router};

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn config(name: &str) -> Config {
    let dir = std::env::temp_dir().join(format!("devpage-threads-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Config::local(&dir.join("data"), &dir.join("bucket"), 1 << 20)
}

/// A site of two pages, `README.md` and `rollout.md`, with these texts.
fn site(readme: &str, rollout: Option<&str>) -> Vec<u8> {
    let yml = "kind: site\ntitle: Launch\nroot: README.md\n";
    let readme = format!("# Launch\n\n{readme}\n");
    match rollout {
        Some(rollout) => zip_of(&[
            ("devpage.yml", yml),
            ("README.md", &readme),
            ("rollout.md", &format!("# Rollout\n\n{rollout}\n")),
        ]),
        None => zip_of(&[("devpage.yml", yml), ("README.md", &readme)]),
    }
}

async fn ask(app: &Router, request: Request) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into()));
    (status, value)
}

fn json_request(method: &str, path: &str, body: Value, user: Option<&str>) -> Request {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(user) = user {
        request = request.header("x-user", user);
    }
    request.body(Body::from(body.to_string())).unwrap()
}

fn get(path: &str) -> Request {
    Request::get(path).body(Body::empty()).unwrap()
}

async fn publish(app: &Router, path: &str, zip: Vec<u8>) -> Value {
    let request = Request::post(path)
        .header("x-user", "ana@example.com")
        .body(Body::from(zip))
        .unwrap();
    let (status, answer) = ask(app, request).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    answer
}

fn anchor(prefix: &str, quote: &str, suffix: &str) -> Value {
    json!({ "quote": quote, "prefix": prefix, "suffix": suffix })
}

/// Starts a thread through the API: its id.
async fn comment(app: &Router, id: &str, page: &str, anchor: Value, text: &str) -> String {
    let (status, answer) = ask(
        app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments"),
            json!({ "page": page, "anchor": anchor, "text": text }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    answer["id"].as_str().unwrap().to_string()
}

async fn threads(app: &Router, id: &str) -> Vec<Value> {
    let (status, answer) = ask(app, get(&format!("/api/documents/{id}/comments"))).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer.as_array().unwrap().clone()
}

#[tokio::test]
async fn a_thread_is_started_replied_to_and_resolved_through_the_api_and_the_page() {
    let app = router(&config("flow")).unwrap();
    let id = publish(
        &app,
        "/api/documents",
        site(
            "The client retries every write until it is acknowledged.",
            None,
        ),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let from_api = comment(
        &app,
        &id,
        "/README.md",
        anchor("The client ", "retries every write", " until"),
        "Every write, or only the outbox?",
    )
    .await;
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments/{from_api}/replies"),
            json!({ "text": "Only the outbox." }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The page of the latest version asks with the page's unmounted path,
    // and sees the same thread.
    let (status, listed) = ask(
        &app,
        get(&format!("/d/{id}/__devpage/comments?page=/README.md")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed[0]["id"], from_api.as_str());
    assert_eq!(listed[0]["thread"][1]["text"], "Only the outbox.");
    assert!(listed[0].get("detached").is_none(), "{listed}");

    // A thread started on the page is the document's too.
    let (status, created) = ask(
        &app,
        json_request(
            "POST",
            &format!("/d/{id}/__devpage/comments"),
            json!({
                "page": "/README.md",
                "anchor": anchor("", "acknowledged", "."),
                "text": "By whom?",
            }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");

    let all = threads(&app, &id).await;
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(all[0]["id"], from_api.as_str());
    assert_eq!(all[0]["page"], "/README.md");
    assert_eq!(all[0]["version"], 1);
    assert_eq!(all[0]["detached"], false);
    assert_eq!(all[0]["thread"][0]["author"], "anonymous");
    assert_eq!(
        all[0]["thread"][0]["text"],
        "Every write, or only the outbox?"
    );
    assert_eq!(all[1]["thread"][0]["text"], "By whom?");

    // Resolving deletes it; a second resolve, or one of a made-up thread,
    // finds nothing.
    let resolve = |thread: &str| {
        Request::delete(format!("/api/documents/{id}/comments/{thread}"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        ask(&app, resolve(&from_api)).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(ask(&app, resolve(&from_api)).await.0, StatusCode::NOT_FOUND);
    assert_eq!(ask(&app, resolve("../x")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(threads(&app, &id).await.len(), 1);

    // A page outside the document, and a document that isn't, are refused.
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments"),
            json!({ "page": "/../x.md", "anchor": anchor("", "x", ""), "text": "no" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let nobody = uuid::Uuid::new_v4();
    assert_eq!(
        ask(&app, get(&format!("/api/documents/{nobody}/comments")))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn with_a_user_header_every_comment_write_needs_it_and_carries_its_author() {
    let mut config = config("authors");
    config.user_header = Some("X-User".into());
    let app = router(&config).unwrap();
    let id = publish(&app, "/api/documents", site("Some text here.", None)).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let new = json!({ "page": "/README.md", "anchor": anchor("", "text", ""), "text": "hi" });
    for path in [
        format!("/api/documents/{id}/comments"),
        format!("/d/{id}/__devpage/comments"),
    ] {
        let (status, _) = ask(&app, json_request("POST", &path, new.clone(), None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments"),
            new.clone(),
            Some("bo@example.com"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/d/{id}/__devpage/comments"),
            new,
            Some("cy@example.com"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let all = threads(&app, &id).await;
    let authors: Vec<&str> = all
        .iter()
        .map(|t| t["thread"][0]["author"].as_str().unwrap())
        .collect();
    assert_eq!(authors, ["bo@example.com", "cy@example.com"]);
    let thread = all[0]["id"].as_str().unwrap();
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments/{thread}/replies"),
            json!({ "text": "anonymous?" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let resolve = Request::delete(format!("/api/documents/{id}/comments/{thread}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(ask(&app, resolve).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn each_publish_carries_the_threads_onto_the_new_version() {
    let app = router(&config("carried")).unwrap();
    let id = publish(
        &app,
        "/api/documents",
        site(
            "The client retries every write until the server acknowledges it.",
            Some("Retries are capped at five."),
        ),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let kept = comment(
        &app,
        &id,
        "/README.md",
        anchor("The client ", "retries every write", " until"),
        "kept",
    )
    .await;
    let moved = comment(
        &app,
        &id,
        "/README.md",
        anchor("", "acknowledges it", "."),
        "moved",
    )
    .await;
    let gone = comment(
        &app,
        &id,
        "/README.md",
        anchor("", "the server", " "),
        "gone",
    )
    .await;
    let page_gone = comment(
        &app,
        &id,
        "/rollout.md",
        anchor("", "capped at five", "."),
        "page gone",
    )
    .await;

    // Version 2: the first sentence keeps its words around the first quote,
    // the second quote is now in another sentence, the third is gone, and
    // so is the whole of rollout.md.
    publish(
        &app,
        &format!("/api/documents/{id}/versions"),
        site(
            "The client retries every write until a peer acknowledges it, twice.",
            None,
        ),
    )
    .await;

    let all = threads(&app, &id).await;
    let by_id = |thread: &str| {
        all.iter()
            .find(|t| t["id"] == thread)
            .unwrap_or_else(|| panic!("{thread} in {all:?}"))
            .clone()
    };
    let kept = by_id(&kept);
    assert_eq!(kept["detached"], false);
    assert_eq!(kept["anchor"]["prefix"], "The client ");
    let moved = by_id(&moved);
    assert_eq!(moved["detached"], false);
    assert_eq!(moved["anchor"]["suffix"], ", twice. ");
    assert_eq!(by_id(&gone)["detached"], true);
    assert_eq!(by_id(&page_gone)["detached"], true);
    assert_eq!(by_id(&gone)["detached_in"], 2);
    assert_eq!(by_id(&page_gone)["detached_in"], 2);
    assert_eq!(kept["detached_in"], serde_json::Value::Null);
    // Each still says the version it was started on.
    assert!(all.iter().all(|t| t["version"] == 1), "{all:?}");

    // The latest's page lists the detached one as detached …
    let (_, listed) = ask(
        &app,
        get(&format!("/d/{id}/__devpage/comments?page=/README.md")),
    )
    .await;
    let detached: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["detached"] == true)
        .map(|t| t["thread"][0]["text"].as_str().unwrap())
        .collect();
    assert_eq!(detached, ["gone"]);
    // … a thread started now is on version 2 …
    comment(&app, &id, "/README.md", anchor("", "twice", "."), "new").await;
    let all = threads(&app, &id).await;
    assert_eq!(all.last().unwrap()["version"], 2);
    // … and an earlier version is read as it was, without comments.
    let (status, _) = ask(
        &app,
        get(&format!("/d/{id}/v/1/__devpage/comments?page=/README.md")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A detached thread stays detached, even when a later version brings
    // its words back: resolving it is the reader's call.
    publish(
        &app,
        &format!("/api/documents/{id}/versions"),
        site(
            "The client retries every write until the server acknowledges it.",
            None,
        ),
    )
    .await;
    assert_eq!(by_id(&gone)["detached"], true);
    let all = threads(&app, &id).await;
    let gone = all.iter().find(|t| t["id"] == gone.as_str()).unwrap();
    assert_eq!(gone["detached"], true);
    // … and it still says the version its text left, not the latest.
    assert_eq!(gone["detached_in"], 2);
}

#[tokio::test]
async fn deleting_a_document_deletes_its_threads() {
    let app = router(&config("deleted")).unwrap();
    let id = publish(&app, "/api/documents", site("Some text here.", None)).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    comment(&app, &id, "/README.md", anchor("", "text", ""), "hi").await;
    let delete = Request::delete(format!("/api/documents/{id}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(ask(&app, delete).await.0, StatusCode::NO_CONTENT);
    assert_eq!(
        ask(&app, get(&format!("/api/documents/{id}/comments")))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_thread_starts_only_on_a_file_in_the_latest_version() {
    let app = router(&config("pages")).unwrap();
    let zip = zip_of(&[
        (
            "devpage.yml",
            "kind: site\ntitle: Launch\nroot: README.md\n",
        ),
        ("README.md", "# Launch\n\nSome text here.\n"),
        // In the zip, but no page of the site links to it.
        ("notes/lone.md", "# Lone\n\nA lone page.\n"),
    ]);
    let id = publish(&app, "/api/documents", zip).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let new = |page: &str| json!({ "page": page, "anchor": anchor("", "lone", ""), "text": "hi" });
    for path in [
        format!("/api/documents/{id}/comments"),
        format!("/d/{id}/__devpage/comments"),
    ] {
        // A page that isn't in the version is refused, by both routes alike.
        for page in ["/missing.md", "/notes", "/../x.md"] {
            let (status, answer) = ask(&app, json_request("POST", &path, new(page), None)).await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{path} {page}: {answer}"
            );
        }
        let (status, answer) = ask(
            &app,
            json_request("POST", &path, new("/notes/lone.md"), None),
        )
        .await;
        assert!(status.is_success(), "{path}: {answer}");
    }
    let pages: Vec<Value> = threads(&app, &id)
        .await
        .into_iter()
        .map(|t| t["page"].clone())
        .collect();
    assert_eq!(pages, ["/notes/lone.md", "/notes/lone.md"]);
}

#[tokio::test]
async fn a_blank_author_is_refused_by_both_routes() {
    let mut config = config("blank");
    config.user_header = Some("X-User".into());
    let app = router(&config).unwrap();
    let id = publish(&app, "/api/documents", site("Some text here.", None)).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let new = json!({ "page": "/README.md", "anchor": anchor("", "text", ""), "text": "hi" });
    for path in [
        format!("/api/documents/{id}/comments"),
        format!("/d/{id}/__devpage/comments"),
    ] {
        let (status, _) = ask(&app, json_request("POST", &path, new.clone(), Some("   "))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        let (status, _) = ask(
            &app,
            json_request("POST", &path, new.clone(), Some("  cy@example.com ")),
        )
        .await;
        assert!(status.is_success(), "{path}");
    }
    let all = threads(&app, &id).await;
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .all(|t| t["thread"][0]["author"] == "cy@example.com"),
        "trimmed: {all:?}"
    );
}

#[tokio::test]
async fn a_detached_thread_still_takes_replies_by_its_id() {
    let app = router(&config("detached")).unwrap();
    let id = publish(
        &app,
        "/api/documents",
        site("Some text here.", Some("Retries are capped at five.")),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let thread = comment(
        &app,
        &id,
        "/rollout.md",
        anchor("", "capped at five", "."),
        "five?",
    )
    .await;
    // Version 2 drops rollout.md: the thread is detached.
    publish(
        &app,
        &format!("/api/documents/{id}/versions"),
        site("Some text here.", None),
    )
    .await;
    assert_eq!(threads(&app, &id).await[0]["detached"], true);

    let (status, answer) = ask(
        &app,
        json_request(
            "POST",
            &format!("/d/{id}/__devpage/comments/reply"),
            json!({ "id": thread, "text": "from the page" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{answer}");
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments/{thread}/replies"),
            json!({ "text": "from the API" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let texts: Vec<Value> = threads(&app, &id).await[0]["thread"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].clone())
        .collect();
    assert_eq!(texts, ["five?", "from the page", "from the API"]);

    // A reply to a made-up thread finds nothing.
    let (status, _) = ask(
        &app,
        json_request(
            "POST",
            &format!("/api/documents/{id}/comments/20261025T000000Z-0000/replies"),
            json!({ "text": "x" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn carry_reads_each_page_as_it_is_shown() {
    let app = router(&config("shown")).unwrap();
    let page = "---\ndescription: The launch.\n---\n\n# Launch\n\n<Callout>\nThe client **retries** every write.\n</Callout>\n";
    let zip = || {
        zip_of(&[
            (
                "devpage.yml",
                "kind: site\ntitle: Launch\nroot: README.md\n",
            ),
            ("README.md", page),
        ])
    };
    let id = publish(&app, "/api/documents", zip()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Anchored as the page shows the text: bold, inside the widget.
    let thread = comment(
        &app,
        &id,
        "/README.md",
        anchor("The client ", "retries every write", "."),
        "every one?",
    )
    .await;
    publish(&app, &format!("/api/documents/{id}/versions"), zip()).await;
    let all = threads(&app, &id).await;
    let carried = all.iter().find(|t| t["id"] == thread.as_str()).unwrap();
    assert_eq!(carried["detached"], false, "{all:?}");
}

/// A carry that fails publishes nothing: the version is not recorded, its
/// zip is not left in the bucket, every thread stays where it was, and the
/// next publish takes the same number.
#[tokio::test]
async fn a_carry_that_fails_publishes_nothing() {
    let config = config("carry-fails");
    let app = router(&config).unwrap();
    let id = publish(
        &app,
        "/api/documents",
        site("The client retries every write.", None),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    comment(
        &app,
        &id,
        "/README.md",
        anchor("The client ", "retries every write", "."),
        "detached by the next version",
    )
    .await;

    // The store refuses to move a thread, as a full disk or a lost lock
    // would.
    let store = rusqlite::Connection::open(config.data_dir.join("store.db")).unwrap();
    store
        .execute_batch(
            "CREATE TRIGGER no_carry BEFORE UPDATE ON threads
             BEGIN SELECT RAISE(ABORT, 'the carry failed'); END;",
        )
        .unwrap();
    let next = site("Every write is retried.", None);
    let request = Request::post(format!("/api/documents/{id}/versions"))
        .body(Body::from(next.clone()))
        .unwrap();
    let (status, answer) = ask(&app, request).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{answer}");
    assert!(
        answer.as_str().unwrap().contains("the carry failed"),
        "{answer}"
    );

    let (_, document) = ask(&app, get(&format!("/api/documents/{id}"))).await;
    assert_eq!(document["latest"], 1);
    assert_eq!(document["versions"].as_array().unwrap().len(), 1);
    let bucket = std::path::PathBuf::from(config.bucket_url.strip_prefix("file://").unwrap());
    assert!(
        !bucket
            .join(format!("documents/{id}/versions/2.zip"))
            .exists()
    );
    let listed = threads(&app, &id).await;
    assert_eq!(listed[0]["detached"], false);
    assert_eq!(listed[0]["detached_in"], Value::Null);

    store.execute_batch("DROP TRIGGER no_carry;").unwrap();
    let again = publish(&app, &format!("/api/documents/{id}/versions"), next).await;
    assert_eq!(again["version"], 2, "the number nothing took");
    let listed = threads(&app, &id).await;
    assert_eq!(listed[0]["detached_in"], 2);
}
