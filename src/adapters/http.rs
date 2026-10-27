//! The network and the runtime behind the core's external-link checker:
//! [`Http`] asks an address over HTTP, [`Tokio`] runs the preview's
//! background asking on devpage's runtime, and [`external`] puts them
//! together with the default waits.

use std::sync::Arc;
use std::time::Duration;

use devpage_core::external::{Ask, Background, External, Outcome, Waits};
use futures::future::BoxFuture;

use crate::changes::Changes;

/// The checker devpage uses: the network, the default waits, announcing on
/// `changes`.
pub fn external(changes: Option<Changes>) -> Arc<External> {
    External::new(
        Arc::new(Http::new()),
        Arc::new(Tokio),
        Waits::default(),
        changes.map(|changes| Arc::new(changes) as _),
    )
}

/// Runs on the tokio runtime the caller is on.
pub struct Tokio;

impl Background for Tokio {
    fn spawn(&self, task: BoxFuture<'static, ()>) -> bool {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return false;
        };
        runtime.spawn(task);
        true
    }

    fn sleep(&self, pause: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(pause))
    }
}

/// Asks the network. It asks for HTML as a browser does — some sites answer
/// other requests with an error a reader never sees.
pub struct Http(reqwest::Client);

impl Http {
    pub fn new() -> Self {
        Http(
            reqwest::Client::builder()
                .user_agent(concat!("devpage/", env!("CARGO_PKG_VERSION")))
                .default_headers(reqwest::header::HeaderMap::from_iter([(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("text/html,*/*;q=0.8"),
                )]))
                .build()
                .expect("an HTTP client"),
        )
    }
}

impl Ask for Http {
    fn ask<'a>(&'a self, address: &'a str, wait: Duration) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            match self.0.get(address).timeout(wait).send().await {
                Ok(response) if response.status().as_u16() < 400 => Outcome::Fine,
                Ok(response) => Outcome::Broken(response.status().to_string()),
                Err(error) if error.is_timeout() => {
                    Outcome::Unreachable(format!("no answer in {}", seconds(wait)))
                }
                // reqwest's own message repeats the address; the root cause says why.
                Err(error) => {
                    let mut cause: &dyn std::error::Error = &error;
                    while let Some(source) = cause.source() {
                        cause = source;
                    }
                    Outcome::Unreachable(cause.to_string())
                }
            }
        })
    }
}

fn seconds(wait: Duration) -> String {
    let millis = wait.as_millis();
    if millis.is_multiple_of(1000) {
        format!("{} seconds", millis / 1000)
    } else {
        format!("{millis} ms")
    }
}

/// The checker over the network: `devpage check` and the preview against a
/// local web server, so the suite needs no network.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use devpage_core::check::{Preview, Report, document};
    use devpage_core::document::Document;
    use devpage_core::problem::Problem;
    use devpage_core::widgets::{Modules, Widgets};

    use super::*;
    use crate::changes::{Change, Scope, Subscription};

    /// A local web server for external-link tests, so the suite needs no
    /// network: `/ok` answers 200, anything else 404, and it counts the requests
    /// for each path.
    mod web {
        use std::collections::HashMap;
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        pub struct Server {
            pub address: String,
            pub hits: Arc<Mutex<HashMap<String, usize>>>,
        }

        pub fn start() -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let hits = Arc::new(Mutex::new(HashMap::new()));
            let counted = hits.clone();
            // `/ok` answers 200 and anything else 404, except: `/slow-<ms>` answers
            // 200 after that long, `/drop` closes without answering, and
            // `/flaky-<n>` closes without answering its first n times, then 200.
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let counted = counted.clone();
                    std::thread::spawn(move || {
                        let mut request = String::new();
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let _ = reader.read_line(&mut request);
                        let path = request.split(' ').nth(1).unwrap_or("/").to_string();
                        let hit = {
                            let mut hits = counted.lock().unwrap();
                            let n = hits.entry(path.clone()).or_insert(0);
                            *n += 1;
                            *n
                        };
                        let status = if path == "/ok" {
                            "200 OK"
                        } else if let Some(ms) = path.strip_prefix("/slow-") {
                            std::thread::sleep(std::time::Duration::from_millis(
                                ms.parse().unwrap(),
                            ));
                            "200 OK"
                        } else if path == "/drop" {
                            return;
                        } else if let Some(n) = path.strip_prefix("/flaky-") {
                            if hit <= n.parse::<usize>().unwrap() {
                                return;
                            }
                            "200 OK"
                        } else {
                            "404 Not Found"
                        };
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        );
                    });
                }
            });
            Server { address, hits }
        }
    }

    /// `document`'s widget set, read through a cache of its own.
    fn widgets_of(document: &Document) -> Widgets {
        Widgets::of(document, &Modules::default())
    }

    fn folder(name: &str, page: &str) -> Document {
        let dir = std::env::temp_dir().join(format!("devpage-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), page).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        Document::load(&dir, &dir).unwrap()
    }

    /// A reporter that keeps what it is told.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Report for Recorder {
        fn problem(&self, problem: &Problem) {
            self.0.lock().unwrap().push(problem.to_string());
        }
    }

    /// The external-link checker over the network, with these waits in
    /// milliseconds, announcing on nothing.
    fn checker(first: u64, retry: u64, retry_after: u64) -> Arc<External> {
        External::new(
            Arc::new(Http::new()),
            Arc::new(Tokio),
            waits(first, retry, retry_after),
            None,
        )
    }

    fn waits(first: u64, retry: u64, retry_after: u64) -> Waits {
        Waits {
            first: Duration::from_millis(first),
            retry: Duration::from_millis(retry),
            retry_after: Duration::from_millis(retry_after),
        }
    }

    /// A preview asking the network with these waits, and the live-update
    /// channel it announces on.
    fn previewing(waits: Waits) -> (Preview, Subscription) {
        let changes = Changes::default();
        let heard = changes.subscribe(Scope::root());
        let external = External::new(
            Arc::new(Http::new()),
            Arc::new(Tokio),
            waits,
            Some(Arc::new(changes)),
        );
        (Preview::new(external, Arc::new(Recorder::default())), heard)
    }

    /// Renders until `done` holds of the problems found, or five seconds pass.
    async fn render_until(
        preview: &Preview,
        doc: &Document,
        pages: &[PathBuf],
        done: impl Fn(&[Problem]) -> bool,
    ) -> Vec<Problem> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let found = preview.problems(doc, &widgets_of(doc), pages);
            if done(&found) || tokio::time::Instant::now() > deadline {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn check_asks_each_address_once_and_reports_every_link_to_it() {
        let web = web::start();
        let doc = folder(
            "check-external",
            &format!(
                "# Home\n\n[a]({0}/gone) [b]({0}/gone)\n\n[c]({0}/ok)\n",
                web.address
            ),
        );
        let found: Vec<String> = document(&doc, Some(&checker(300, 3000, 50)))
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        let gone = format!("README.md:3: {}/gone — 404 Not Found", web.address);
        assert_eq!(found, vec![gone.clone(), gone], "{found:#?}");
        let hits = web.hits.lock().unwrap();
        assert_eq!(hits.get("/gone"), Some(&1));
        assert_eq!(hits.get("/ok"), Some(&1));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_preview_checks_in_the_background_and_refreshes_open_pages() {
        let web = web::start();
        let doc = folder(
            "preview-external",
            &format!("# Home\n\n[a]({0}/missing) [b]({0}/ok)\n", web.address),
        );
        let (preview, mut heard) = previewing(Waits::default());
        let pages = [doc.base.join("README.md")];

        // The first render holds nothing up: the answer isn't in yet.
        assert!(preview.problems(&doc, &widgets_of(&doc), &pages).is_empty());
        // A broken address, once answered, asks open pages to fetch again …
        let change = tokio::time::timeout(Duration::from_secs(5), heard.next())
            .await
            .expect("a change");
        assert_eq!(change, Some(Change::Refresh));
        // … and the next render lists it, marking the link as written.
        let found = render_until(&preview, &doc, &pages, |found| !found.is_empty()).await;
        assert_eq!(found.len(), 1, "{found:#?}");
        let missing = format!("{}/missing", web.address);
        assert_eq!(found[0].target.as_deref(), Some(missing.as_str()));
        assert_eq!(found[0].line, Some(3));
        // Each address is asked once for as long as devpage runs — the good
        // one may still be on its way when the broken one's answer is in.
        preview.problems(&doc, &widgets_of(&doc), &pages);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !web.hits.lock().unwrap().contains_key("/ok")
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let hits = web.hits.lock().unwrap().clone();
        assert_eq!(hits.get("/missing"), Some(&1));
        assert_eq!(hits.get("/ok"), Some(&1));
    }
}
