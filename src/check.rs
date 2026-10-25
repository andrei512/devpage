//! Link checking: every link, image and diagram target a page holds must
//! reach a file in the served folder — and a heading on it, when it names one —
//! and every external address must answer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::broadcast;

use crate::document::{Document, Kind};
use crate::render;

/// One broken thing, where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Problem {
    /// The file it is in, from the top of the served folder.
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
    /// The link as written, so the page can mark it where it stands.
    pub target: Option<String>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// The pages a document is made of.
fn pages(document: &Document) -> Vec<PathBuf> {
    match &document.kind {
        Kind::Site { root } => vec![document.base.join(root)],
        Kind::Doc { chapters, .. } => chapters.clone(),
        Kind::Slides { slides } => slides.clone(),
    }
}

/// Checks the document and every page and document reachable from it by
/// links inside the served folder.
pub async fn document(document: &Document, check_external: bool) -> Vec<Problem> {
    let mut external = Vec::new();
    let mut problems = internal(document, &mut external);
    if check_external {
        problems.extend(external_links(external).await);
    }
    problems
}

/// One external link: the page it is on, its line, and the address.
type External = (String, Option<usize>, String);

/// The client every external check uses. It asks for HTML as a browser does —
/// some sites answer other requests with an error a reader never sees.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("devpage/", env!("CARGO_PKG_VERSION")))
        .default_headers(reqwest::header::HeaderMap::from_iter([(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("text/html,*/*;q=0.8"),
        )]))
        .build()
        .expect("an HTTP client")
});

/// Why an external address is broken, or `None` when it answers below 400,
/// following redirects, within 10 seconds.
async fn answer(address: &str) -> Option<String> {
    match CLIENT.get(address).send().await {
        Ok(response) if response.status().as_u16() < 400 => None,
        Ok(response) => Some(response.status().to_string()),
        Err(error) if error.is_timeout() => Some("no answer in 10 seconds".into()),
        // reqwest's own message repeats the address; the root cause says why.
        Err(error) => {
            let mut cause: &dyn std::error::Error = &error;
            while let Some(source) = cause.source() {
                cause = source;
            }
            Some(cause.to_string())
        }
    }
}

/// Checks each external address once, however many pages link to it, and
/// reports a problem for every link to a broken one.
async fn external_links(links: Vec<External>) -> Vec<Problem> {
    let mut by_address: HashMap<String, Vec<(String, Option<usize>)>> = HashMap::new();
    for (file, line, address) in links {
        by_address.entry(address).or_default().push((file, line));
    }
    let checks = by_address.into_iter().map(|(address, links)| async move {
        let Some(why) = answer(&address).await else {
            return Vec::new();
        };
        links
            .into_iter()
            .map(|(file, line)| Problem {
                file,
                line,
                message: format!("{address} — {why}"),
                target: Some(address.clone()),
            })
            .collect()
    });
    futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect()
}

fn internal(document: &Document, external: &mut Vec<External>) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<PathBuf> = pages(document).into();
    while let Some(page) = queue.pop_front() {
        if !seen.insert(page.clone()) {
            continue;
        }
        let (found, reached) = page_links_with(document, &page, external);
        problems.extend(found);
        for target in reached {
            if target.extension().is_some_and(|ext| ext == "md") {
                queue.push_back(target);
            } else if target
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
            {
                // An invalid one is a problem on the page linking to it.
                if let Ok(linked) = Document::load(&target, &document.served) {
                    queue.extend(pages(&linked));
                }
            }
        }
    }
    problems
}

/// One page's problems and reached files; its external links are collected
/// in `external` as (file, line, address).
fn page_links_with(
    document: &Document,
    page: &Path,
    external: &mut Vec<External>,
) -> (Vec<Problem>, Vec<PathBuf>) {
    let source = std::fs::read_to_string(page).unwrap_or_default();
    let html = render::markdown(&source);
    let file = document.url(page)[1..].to_string();
    let mut problems = Vec::new();
    let mut reached = Vec::new();
    let mut targets = attributes(&html, "href");
    targets.extend(attributes(&html, "src"));
    // A diagram's `click … href "…"` lines, from real Mermaid blocks only.
    for block in html.split("<pre lang=\"mermaid\"><code>").skip(1) {
        let block = block
            .split("</code>")
            .next()
            .unwrap_or_default()
            .replace("&quot;", "\"");
        targets.extend(block.lines().filter_map(|line| {
            let rest = line.trim().strip_prefix("click ")?;
            let href = rest.split("href \"").nth(1)?.split('"').next()?;
            Some(href.to_string())
        }));
    }
    for target in targets {
        let line = source
            .find(&target)
            .map(|at| source[..at].lines().count().max(1));
        if target.starts_with("http://") || target.starts_with("https://") {
            external.push((file.clone(), line, target));
            continue;
        }
        if target.contains(':') {
            continue; // mailto: and other schemes are not checked
        }
        let (path, fragment) = target.split_once('#').unwrap_or((&target, ""));
        let resolved = if path.is_empty() {
            Some(page.to_path_buf())
        } else {
            page.parent()
                .unwrap_or(Path::new("."))
                .join(path)
                .canonicalize()
                .ok()
                .filter(|resolved| resolved.starts_with(&document.served))
        };
        let Some(resolved) = resolved else {
            problems.push(Problem {
                file: file.clone(),
                line,
                message: format!("broken link {target} — no such file"),
                target: Some(target.clone()),
            });
            continue;
        };
        if !fragment.is_empty()
            && resolved.extension().is_some_and(|ext| ext == "md")
            && fragment.parse::<usize>().is_err()
        {
            let target_html =
                render::markdown(&std::fs::read_to_string(&resolved).unwrap_or_default());
            if !target_html.contains(&format!("id=\"{fragment}\"")) {
                problems.push(Problem {
                    file: file.clone(),
                    line,
                    message: format!("broken link {target} — no heading #{fragment}"),
                    target: Some(target.clone()),
                });
            }
        }
        if crate::document::kind_of(&resolved).is_some() {
            match Document::load(&resolved, &document.served) {
                Err(error) => {
                    problems.push(Problem {
                        file: file.clone(),
                        line,
                        message: format!(
                            "broken link {target} — {}",
                            relative(&format!("{error:#}"), &document.served)
                        ),
                        target: Some(target.clone()),
                    });
                    continue;
                }
                // `#n` on a deck must name one of its slides.
                Ok(Document {
                    kind: Kind::Slides { slides },
                    ..
                }) if !fragment.is_empty()
                    && !fragment
                        .parse::<usize>()
                        .is_ok_and(|n| (1..=slides.len()).contains(&n)) =>
                {
                    problems.push(Problem {
                        file: file.clone(),
                        line,
                        message: format!(
                            "broken link {target} — the deck has {} slide{}",
                            slides.len(),
                            if slides.len() == 1 { "" } else { "s" }
                        ),
                        target: Some(target.clone()),
                    });
                }
                Ok(_) => {}
            }
        }
        reached.push(resolved);
    }
    // In a site, a Topics entry outside the site's folder is left out of the sidebar.
    if matches!(document.kind, Kind::Site { .. }) {
        for topic in crate::nav::topics(&source) {
            if let crate::nav::Target::Problem(message) =
                crate::nav::resolve(document, page, &topic.href)
                && message.ends_with("outside the site's folder")
            {
                problems.push(Problem {
                    file: file.clone(),
                    line: source
                        .find(&topic.href)
                        .map(|at| source[..at].lines().count().max(1)),
                    message,
                    target: Some(topic.href.clone()),
                });
            }
        }
    }
    problems.extend(crate::comments::problems(document, page));
    (problems, reached)
}

/// An error's text with the served folder's absolute path taken out, so
/// files read as their addresses do.
fn relative(text: &str, served: &Path) -> String {
    text.replace(&format!("{}/", served.display()), "")
}

/// What the preview knows about each external address.
enum Answer {
    Pending,
    Fine,
    Broken(String),
}

/// Every external address the preview has asked about, for as long as
/// devpage runs.
static EXTERNAL: LazyLock<Mutex<HashMap<String, Answer>>> = LazyLock::new(Mutex::default);

/// Where the preview announces a broken external address, so open pages
/// fetch themselves again and show it.
static CHANGES: OnceLock<broadcast::Sender<String>> = OnceLock::new();

/// Lets the preview's background checks refresh open pages.
pub fn announce_on(changes: broadcast::Sender<String>) {
    let _ = CHANGES.set(changes);
}

/// The problems on the given pages, for the preview: each is printed on the
/// terminal the first time it is found, and returned for the page to list.
/// External addresses are checked in the background, once each; a broken one
/// shows from the next render, which its answer triggers.
pub fn preview(document: &Document, pages: &[PathBuf]) -> Vec<Problem> {
    static REPORTED: LazyLock<Mutex<HashSet<Problem>>> = LazyLock::new(Mutex::default);
    let mut external = Vec::new();
    let mut problems: Vec<Problem> = pages
        .iter()
        .flat_map(|page| page_links_with(document, page, &mut external).0)
        .collect();
    let mut answers = EXTERNAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (file, line, address) in external {
        match answers.get(&address) {
            Some(Answer::Broken(why)) => problems.push(Problem {
                file,
                line,
                message: format!("{address} — {why}"),
                target: Some(address),
            }),
            Some(Answer::Pending | Answer::Fine) => {}
            None => {
                // Outside a runtime (a unit test rendering a page) nothing is
                // checked in the background.
                let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                    continue;
                };
                answers.insert(address.clone(), Answer::Pending);
                runtime.spawn(async move {
                    let result = answer(&address).await;
                    let broken = result.is_some();
                    EXTERNAL
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(address, result.map_or(Answer::Fine, Answer::Broken));
                    if broken && let Some(changes) = CHANGES.get() {
                        let _ = changes.send("change".into());
                    }
                });
            }
        }
    }
    drop(answers);
    let mut reported = REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for problem in &problems {
        if reported.insert(problem.clone()) {
            eprintln!("{problem}");
        }
    }
    problems
}

/// The values of every `name="…"` attribute in rendered HTML.
fn attributes(html: &str, name: &str) -> Vec<String> {
    let pattern = format!(" {name}=\"");
    html.split(&pattern)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|value| value.replace("&amp;", "&"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_missing_files_headings_and_climbs_out_of_the_served_folder() {
        let dir = std::env::temp_dir().join(format!("devpage-check-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Home\n\n[ok](b.md#two) [gone](nope.md)\n\n[bad](b.md#three) [out](../../x.md)\n\n```mermaid\nflowchart LR\n  a\n  click a href \"missing.md\"\n```\n",
        )
        .unwrap();
        std::fs::write(dir.join("b.md"), "# B\n\n## Two\n\n![logo](logo.png)\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        let doc = Document::load(&dir, &dir).unwrap();
        let found: Vec<String> = internal(&doc, &mut Vec::new())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 5, "{found:#?}");
        assert!(
            found.contains(&"README.md:3: broken link nope.md — no such file".to_string()),
            "{found:#?}"
        );
        assert!(found.iter().any(|p| p.contains("b.md#three — no heading")));
        assert!(found.iter().any(|p| p.contains("../../x.md")));
        assert!(found.iter().any(|p| p.contains("missing.md")));
        assert!(
            found
                .iter()
                .any(|p| p.starts_with("b.md:5: broken link logo.png")),
            "{found:#?}"
        );
    }
}

/// A local web server for external-link tests, so the suite needs no
/// network: `/ok` answers 200, anything else 404, and it counts the requests
/// for each path.
#[cfg(test)]
pub(crate) mod web {
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
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let _ = reader.read_line(&mut request);
                let path = request.split(' ').nth(1).unwrap_or("/").to_string();
                *counted.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
                let status = if path == "/ok" {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
            }
        });
        Server { address, hits }
    }
}

#[cfg(test)]
mod external {
    use super::*;

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

    #[tokio::test]
    async fn a_slide_number_must_be_one_the_deck_has() {
        let doc = folder(
            "deck-number",
            "# Home\n\n[two](deck/devpage.yml#2) [nine](deck/devpage.yml#9) [word](deck/devpage.yml#intro)\n",
        );
        std::fs::create_dir_all(doc.base.join("deck")).unwrap();
        std::fs::write(doc.base.join("deck/a.md"), "# A").unwrap();
        std::fs::write(doc.base.join("deck/b.md"), "# B").unwrap();
        std::fs::write(
            doc.base.join("deck/devpage.yml"),
            "kind: slides\ntitle: D\nslides: [a.md, b.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, false)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                "README.md:3: broken link deck/devpage.yml#9 — the deck has 2 slides".to_string(),
                "README.md:3: broken link deck/devpage.yml#intro — the deck has 2 slides"
                    .to_string(),
            ],
            "{found:#?}"
        );
    }

    #[tokio::test]
    async fn a_broken_linked_document_is_named_by_its_address() {
        let doc = folder("linked-relative", "# Home\n\n[plan](plan/devpage.yml)\n");
        std::fs::create_dir_all(doc.base.join("plan")).unwrap();
        std::fs::write(
            doc.base.join("plan/devpage.yml"),
            "kind: doc\ntitle: P\nchapters: [missing.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, false)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            !found[0].contains(&doc.served.display().to_string()),
            "{found:#?}"
        );
        assert!(found[0].contains("in plan/devpage.yml"), "{found:#?}");
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
        let found: Vec<String> = document(&doc, true)
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
        let (changes, mut heard) = broadcast::channel(16);
        announce_on(changes);
        let pages = [doc.base.join("README.md")];

        // The first render holds nothing up: the answer isn't in yet.
        assert!(preview(&doc, &pages).is_empty());
        // A broken address, once answered, asks open pages to fetch again …
        let message = tokio::time::timeout(Duration::from_secs(5), heard.recv())
            .await
            .expect("a change")
            .unwrap();
        assert_eq!(message, "change");
        // … and the next render lists it, marking the link as written.
        let found = preview(&doc, &pages);
        assert_eq!(found.len(), 1, "{found:#?}");
        let missing = format!("{}/missing", web.address);
        assert_eq!(found[0].target.as_deref(), Some(missing.as_str()));
        assert_eq!(found[0].line, Some(3));
        // Each address is asked once for as long as devpage runs.
        preview(&doc, &pages);
        let hits = web.hits.lock().unwrap();
        assert_eq!(hits.get("/missing"), Some(&1));
        assert_eq!(hits.get("/ok"), Some(&1));
    }
}

#[cfg(test)]
mod examples {
    use super::*;

    #[test]
    fn the_examples_have_no_broken_links() {
        for example in ["examples/sync-design", "examples/launch-talk"] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let doc = Document::load(&root.join(example), root).unwrap();
            let problems = internal(&doc, &mut Vec::new());
            assert!(problems.is_empty(), "{example}: {problems:#?}");
        }
    }

    /// devpage's own docs, from their root page: every internal link and
    /// anchor must hold. External links aren't checked, so no network.
    #[test]
    fn devpage_docs_have_no_broken_links() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let doc = Document::load(&root.join("docs"), root).unwrap();
        let problems: Vec<String> = internal(&doc, &mut Vec::new())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
