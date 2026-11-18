//! An extension's `live/` address: every request under it, and every
//! connection upgraded there — a WebSocket — passed to the extension's
//! process as it is, with the token that proves it came through devpage and
//! the name of who is at the browser.

use axum::body::Body;
use axum::extract::{Request, State};
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
    async fn where_no_process_is_started_live_is_not_found() {
        let dir = site("off");
        let (address, live) = serve(&dir, false).await;
        let got = reqwest::get(format!("http://{address}{live}hello"))
            .await
            .unwrap();
        assert_eq!(got.status(), 404);
        assert!(got.text().await.unwrap().contains("loopback"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
