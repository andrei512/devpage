//! An extension's `live/` address: every request under it, and every
//! connection upgraded there — a WebSocket — passed to the extension's
//! process as it is, with the token that proves it came through devpage and
//! the name of who is at the browser. And its `pages/` address: each page
//! under it is Markdown the process answers, which devpage draws.

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use super::AppState;
use crate::adapters::processes::Endpoint;

/// The header a process finds devpage's token in.
const TOKEN: &str = "x-devpage-token";
/// The header a process finds who is at the browser in.
const USER: &str = "x-devpage-user";
/// The most Markdown a process may answer a page with, in bytes.
const LONGEST_PAGE: usize = 8 << 20;

/// Any request under `/__devpage/extensions/<owner>/<id>/live/`.
pub(super) async fn live(State(state): State<AppState>, request: Request) -> Response {
    let Some(processes) = &state.processes else {
        return (
            StatusCode::NOT_FOUND,
            "extension processes run only under `devpage` on a loopback address",
        )
            .into_response();
    };
    if let Err(why) = same_origin(request.headers()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    let path = request.uri().path().to_string();
    let Some(live) = devpage_core::extensions::live(&path, state.folder.served()) else {
        return (
            StatusCode::NOT_FOUND,
            "no extension is bound at this address",
        )
            .into_response();
    };
    let endpoint = match processes.endpoint(&live).await {
        Ok(endpoint) => endpoint,
        Err(why) if live.process.is_none() => {
            return (StatusCode::NOT_FOUND, why).into_response();
        }
        Err(why) => return (StatusCode::BAD_GATEWAY, why).into_response(),
    };
    match forward(&endpoint, processes.user(), &live.rest, request).await {
        Ok(response) => response,
        Err(error) => {
            let died = processes.died(&live.key).await;
            let why = match died {
                Some(output) => format!("extension {}: its process died\n{output}", live.id),
                None => format!("extension {}: {error}", live.id),
            };
            (StatusCode::BAD_GATEWAY, why).into_response()
        }
    }
}

/// A page of an extension: `/__devpage/extensions/<owner>/<id>/pages/<page>`.
/// The extension's process is asked for `pages/<page>`, as a request under
/// `live/` is passed on, and the Markdown it answers is drawn as a page in
/// the frame of the document that binds the extension. A page the process
/// says it has not is not found, as any address with nothing at it.
pub(super) async fn page(
    State(state): State<AppState>,
    Path((owner, id, page)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    let Some(processes) = &state.processes else {
        return (
            StatusCode::NOT_FOUND,
            "extension processes run only under `devpage` on a loopback address",
        )
            .into_response();
    };
    if let Err(why) = same_origin(request.headers()) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    let headers = request.headers().clone();
    // The address as it was decoded names the page; the process is asked
    // for it as it was written.
    let path = format!("/__devpage/extensions/{owner}/{id}/pages/{page}");
    let missing = {
        let path = path.clone();
        move |answers: devpage_core::answer::Answers<'_>| answers.missing(&path)
    };
    let entry = state.folder.entry().path();
    let asked = request.uri().path().to_string();
    let Some((live, _, document)) =
        devpage_core::extensions::page(&asked, state.folder.served(), &entry)
    else {
        return super::respond(&state, &headers, super::resolved(&state, missing).await).await;
    };
    let endpoint = match processes.endpoint(&live).await {
        Ok(endpoint) => endpoint,
        Err(why) if live.process.is_none() => {
            return (StatusCode::NOT_FOUND, why).into_response();
        }
        Err(why) => return (StatusCode::BAD_GATEWAY, why).into_response(),
    };
    let answered = async {
        let response = forward(&endpoint, processes.user(), &live.rest, request).await?;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), LONGEST_PAGE).await?;
        anyhow::Ok((status, String::from_utf8_lossy(&body).into_owned()))
    };
    let answer = match answered.await {
        Ok((StatusCode::NOT_FOUND, _)) => super::resolved(&state, missing).await,
        Ok((status, markdown)) if status.is_success() => {
            super::resolved(&state, move |answers| {
                answers.extension_page(&path, &document, &page, markdown)
            })
            .await
        }
        Ok((status, said)) => {
            let why = format!(
                "extension {}: its process answered {status} for {}\n{said}",
                live.id, live.rest
            );
            return (StatusCode::BAD_GATEWAY, why).into_response();
        }
        Err(error) => {
            let why = match processes.died(&live.key).await {
                Some(output) => format!("extension {}: its process died\n{output}", live.id),
                None => format!("extension {}: {error}", live.id),
            };
            return (StatusCode::BAD_GATEWAY, why).into_response();
        }
    };
    super::respond(&state, &headers, answer).await
}

/// Refuses a request another site's page made, or one made under a name that
/// isn't this machine's own: a process acts as the person running devpage, so
/// only devpage's own pages may reach it. A client that is no browser sends
/// neither header checked here, and passes.
fn same_origin(headers: &HeaderMap) -> Result<(), &'static str> {
    let said = |name: header::HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
    let host = said(header::HOST).unwrap_or_default();
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    if !matches!(name, "localhost" | "127.0.0.1" | "[::1]") {
        return Err("an extension's process answers only on localhost");
    }
    if let Some(origin) = said(header::ORIGIN)
        && origin != format!("http://{host}")
    {
        return Err("an extension's process answers only devpage's own pages");
    }
    if let Some(site) = said(header::HeaderName::from_static("sec-fetch-site"))
        && !matches!(site, "same-origin" | "none")
    {
        return Err("an extension's process answers only devpage's own pages");
    }
    Ok(())
}

/// `request` sent to the process at `endpoint` as a request for `rest`, and
/// its response; when the process switches protocols, the two connections
/// are joined until either closes.
async fn forward(
    endpoint: &Endpoint,
    user: &str,
    rest: &str,
    mut request: Request,
) -> anyhow::Result<Response> {
    let stream = TcpStream::connect(("127.0.0.1", endpoint.port)).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(connection.with_upgrades());
    let asked = match request.uri().query() {
        Some(query) => format!("/{rest}?{query}"),
        None => format!("/{rest}"),
    };
    *request.uri_mut() = asked.parse::<Uri>()?;
    let headers = request.headers_mut();
    headers.insert(TOKEN, HeaderValue::from_str(&endpoint.token)?);
    match HeaderValue::from_bytes(user.as_bytes()) {
        Ok(user) => headers.insert(USER, user),
        Err(_) => headers.remove(USER),
    };
    let upgrading = request.headers().contains_key(header::UPGRADE);
    let ours = upgrading.then(|| hyper::upgrade::on(&mut request));
    let mut response = sender.send_request(request).await?;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(ours) = ours
    {
        let theirs = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            let (Ok(ours), Ok(theirs)) = tokio::join!(ours, theirs) else {
                return;
            };
            let (mut ours, mut theirs) = (TokioIo::new(ours), TokioIo::new(theirs));
            let _ = tokio::io::copy_bidirectional(&mut ours, &mut theirs).await;
        });
    }
    Ok(response.map(Body::new))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use devpage_core::document::Document;
    use devpage_core::folder::ServedFolder;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::adapters::processes::tests::{keeping, serving};
    use crate::drive::http::{App, router};

    /// A site in a new folder named for `name`, with the extension `toy`: a
    /// package that only runs the tests' process. Returns the folder.
    fn site(name: &str) -> std::path::PathBuf {
        crate::adapters::extensions::install();
        let dir = std::env::temp_dir().join(format!("devpage-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("toy")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let run = serde_json::to_string(&serving(&dir).command).unwrap();
        std::fs::write(
            dir.join("toy/extension.yml"),
            format!("id: toy\nversion: \"1\"\napi: 1\nrun: {run}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# Page\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nextensions:\n  toy:\n    package: ./toy\n    config:\n      greeting: hi\n",
        )
        .unwrap();
        dir
    }

    /// Serves the site in `dir`, starting processes or not, and returns the
    /// server's address and the address of the extension's `live/`.
    async fn serve(dir: &std::path::Path, processes: bool) -> (String, String) {
        let document = Arc::new(
            Document::load(&dir.join("devpage.yml"), &ServedFolder::new(dir).unwrap()).unwrap(),
        );
        let (_, base) = document.extensions[0].address();
        let app = App::testing(document, Default::default());
        let app = if processes {
            app.with_processes(keeping())
        } else {
            app
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, router(app)).await.ok() });
        (address, format!("{base}live/"))
    }

    #[tokio::test]
    async fn a_request_under_live_reaches_the_process_with_the_token_and_the_user() {
        let dir = site("request");
        let (address, live) = serve(&dir, true).await;
        let client = reqwest::Client::new();
        let got = client
            .get(format!("http://{address}{live}hello/there?x=1"))
            .header(TOKEN, "forged")
            .send()
            .await
            .unwrap();
        assert_eq!(got.status(), 200);
        assert_eq!(
            got.text().await.unwrap(),
            "GET /hello/there?x=1 token=true user=Ada config={\"greeting\":\"hi\"} url=http://localhost:1 root=true body="
        );
        let posted = client
            .post(format!("http://{address}{live}"))
            .body("said")
            .send()
            .await
            .unwrap();
        assert!(
            posted
                .text()
                .await
                .unwrap()
                .starts_with("POST /? token=true user=Ada")
        );
        // Another site's page is refused, and so is a name that isn't this
        // machine's own.
        for (name, value) in [
            ("origin", "http://elsewhere.example"),
            ("sec-fetch-site", "cross-site"),
            ("sec-fetch-site", "same-site"),
            ("host", "elsewhere.example"),
        ] {
            let refused = client
                .get(format!("http://{address}{live}hello"))
                .header(name, value)
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 403, "{name}: {value}");
        }
        let own = client
            .get(format!("http://{address}{live}hello"))
            .header("origin", format!("http://{address}"))
            .header("sec-fetch-site", "same-origin")
            .send()
            .await
            .unwrap();
        assert_eq!(own.status(), 200);
        let nowhere = client
            .get(format!(
                "http://{address}/__devpage/extensions/0000/toy/live/hello"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(nowhere.status(), 404);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_upgraded_connection_is_joined_to_the_process() {
        let dir = site("upgrade");
        let (address, live) = serve(&dir, true).await;
        let mut stream = TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(
                format!(
                    "GET {live}socket HTTP/1.1\r\nHost: {address}\r\nConnection: upgrade\r\nUpgrade: echo\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.unwrap());
        }
        let head = String::from_utf8(head).unwrap().to_lowercase();
        assert!(head.starts_with("http/1.1 101"), "{head}");
        assert!(head.contains("upgrade: echo"), "{head}");
        for said in ["first", "second"] {
            stream.write_all(said.as_bytes()).await.unwrap();
            let mut heard = vec![0; said.len()];
            stream.read_exact(&mut heard).await.unwrap();
            assert_eq!(heard, said.as_bytes());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_extensions_page_is_what_its_process_wrote_drawn_in_the_sites_frame() {
        let dir = site("page");
        let (address, live) = serve(&dir, true).await;
        let pages = format!(
            "http://{address}{}pages/",
            live.strip_suffix("live/").unwrap()
        );
        let client = reqwest::Client::new();
        let asked = |page: &str| {
            client
                .get(format!("{pages}{page}"))
                .header("accept", "text/html")
        };
        let got = asked("pads/how%20it.md").send().await.unwrap();
        assert_eq!(got.status(), 200);
        let html = got.text().await.unwrap();
        // The process was asked for the page as a request under `live/` is.
        assert!(
            html.contains(
                "Asked for <code>/pages/pads/how%20it.md</code> by Ada with the token: true."
            ),
            "{html}"
        );
        // In the site's frame, its parts named as the extension names the
        // page, and with no comments.
        assert!(html.contains("<body class=\"site"), "{html}");
        assert!(html.contains("<title>Written · T</title>"), "{html}");
        assert!(
            html.contains("<article data-source=\"pads/how it.md\">"),
            "{html}"
        );
        assert!(html.contains("data-no-comments"), "{html}");
        // A page the process has not is devpage's page for nothing there.
        let gone = asked("gone.md").send().await.unwrap();
        assert_eq!(gone.status(), 404);
        assert!(gone.text().await.unwrap().contains("Page not found"));
        let gone = reqwest::get(format!("{pages}gone.md")).await.unwrap();
        assert_eq!(gone.status(), 404);
        assert!(gone.text().await.unwrap().starts_with("nothing at "));
        let broken = asked("broken.md").send().await.unwrap();
        assert_eq!(broken.status(), 502);
        let why = broken.text().await.unwrap();
        assert!(why.contains("answered 418"), "{why}");
        // Only devpage's own pages reach it, and only an extension's own.
        let refused = asked("pads/how.md").header("sec-fetch-site", "cross-site");
        assert_eq!(refused.send().await.unwrap().status(), 403);
        let nowhere = format!("http://{address}/__devpage/extensions/0000/toy/pages/how.md");
        assert_eq!(reqwest::get(nowhere).await.unwrap().status(), 404);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_doc_draws_an_extensions_page_as_a_doc_of_one_chapter() {
        let dir = site("doc-page");
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: doc\ntitle: T\nchapters: [README.md]\nextensions:\n  toy:\n    package: ./toy\n",
        )
        .unwrap();
        let (address, live) = serve(&dir, true).await;
        let page = format!(
            "http://{address}{}pages/pads/how.md",
            live.strip_suffix("live/").unwrap()
        );
        let html = reqwest::get(page).await.unwrap().text().await.unwrap();
        assert!(html.contains("<body class=\"doc\" "), "{html}");
        assert!(
            html.contains("<section class=\"chapter\" data-source=\"pads/how.md\">"),
            "{html}"
        );
        assert!(html.contains("<h1 id=\"written\">Written"), "{html}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn the_live_session_is_on_every_page_of_the_served_folder_and_its_program_answers() {
        let dir = site("session");
        let run = serde_json::to_string(&serving(&dir).command).unwrap();
        std::fs::create_dir_all(dir.join("program")).unwrap();
        std::fs::write(
            dir.join("program/extension.yml"),
            format!("id: live\nversion: \"1\"\napi: 1\nrun: {run}\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        let scope = ServedFolder::new(&dir).unwrap();
        // Off, a page names none of the session's widgets.
        let off = Arc::new(Document::load(&dir.join("devpage.yml"), &scope).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let without = listener.local_addr().unwrap();
        let app = App::testing(off, Default::default()).with_processes(keeping());
        tokio::spawn(async move { axum::serve(listener, router(app)).await.ok() });
        let html = reqwest::get(format!("http://{without}/"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(html.contains("<h1 id=\"page\">Page"), "{html}");
        assert!(!html.contains("window.devpagePlaces"), "{html}");
        assert!(!html.contains("window.devpageExtensions"), "{html}");
        let preloaded = |html: &str, widget: &str| {
            html.lines()
                .any(|line| line.contains("modulepreload") && line.contains(widget))
        };
        assert!(!preloaded(&html, "DevpageLive"), "{html}");

        let program = crate::adapters::extensions::load(&dir.join("program"), "live").unwrap();
        devpage_core::extensions::serve_live(&scope, program);
        let (address, live) = serve(&dir, true).await;
        let html = reqwest::get(format!("http://{address}/"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            html.contains(
                "window.devpagePlaces = {\"float\":[\"DevpageLiveCanvas\",\"DevpageLiveOrb\"],\
                 \"navbar\":[\"DevpageLiveTasksButton\"],\"panel\":[\"DevpageLiveTasks\"]};"
            ),
            "{html}"
        );
        assert!(preloaded(&html, "DevpageLiveOrb.tsx"), "{html}");
        assert!(html.contains(live.strip_suffix("live/").unwrap()), "{html}");
        // Its program answers what is under its address, and writes its
        // pages.
        let said = reqwest::get(format!("http://{address}{live}state"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(said.starts_with("GET /state?"), "{said}");
        let pad = format!("{}pages/pads/how.md", live.strip_suffix("live/").unwrap());
        let html = reqwest::get(format!("http://{address}{pad}"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            html.contains("<article data-source=\"pads/how.md\">"),
            "{html}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn where_no_process_is_started_live_is_not_found() {
        let dir = site("off");
        let (address, live) = serve(&dir, false).await;
        let got = reqwest::get(format!("http://{address}{live}hello"))
            .await
            .unwrap();
        assert_eq!(got.status(), 404);
        assert!(got.text().await.unwrap().contains("loopback"));
        let page = live.replace("/live/", "/pages/how.md");
        let got = reqwest::get(format!("http://{address}{page}"))
            .await
            .unwrap();
        assert_eq!(got.status(), 404);
        assert!(got.text().await.unwrap().contains("loopback"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
