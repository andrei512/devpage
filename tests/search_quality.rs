//! How well search finds what a reader means, measured: a set of queries
//! over a frozen copy of devpage's own docs (`tests/fixtures/search-docs/`),
//! each naming the address it must find first, in the top three, or never.
//! It starts the real `devpage` on a free port, asks its search endpoint,
//! asserts every query, and prints the set's MRR and success@3, which must
//! not fall below their floors.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// The least the query set may score: its mean reciprocal rank, and the
/// share of queries whose address is in the top three.
const MRR_FLOOR: f64 = 0.91;
const SUCCESS_AT_3_FLOOR: f64 = 1.0;

/// Where a query's address must be.
#[derive(Clone, Copy, PartialEq)]
enum Want {
    First,
    Top3,
    Never,
}
use Want::*;

/// The queries: what a reader types, and the address it must lead to. An
/// address without `#` is a page, found by any of its sections; one with `#`
/// is that section, or the page result it is.
const QUERIES: &[(&str, Want, &str)] = &[
    // A word that is a page's title opens that page.
    ("design", First, "/docs/boxes/viewer/designs/README.md"),
    ("designs", First, "/docs/boxes/viewer/designs/README.md"),
    ("search", First, "/docs/boxes/viewer/search/README.md"),
    ("glossary", First, "/docs/boxes/viewer/glossary/README.md"),
    ("themes", First, "/docs/boxes/viewer/themes/README.md"),
    (
        "live updates",
        First,
        "/docs/boxes/viewer/live-updates/README.md",
    ),
    ("vision", First, "/docs/vision.md"),
    ("getting started", First, "/docs/getting-started.md"),
    (
        "comment store",
        First,
        "/docs/boxes/viewer/comments/the-comment-store/README.md",
    ),
    (
        "project widget",
        First,
        "/docs/boxes/viewer/widgets/project-widgets/README.md",
    ),
    (
        "code map",
        First,
        "/docs/boxes/engineering/code-map/README.md",
    ),
    // A word a section is about leads to that section.
    (
        "outbox",
        Top3,
        "/docs/boxes/viewer/comments/the-comment-store/README.md",
    ),
    (
        "frame ids",
        First,
        "/docs/boxes/viewer/rendering-a-page/README.md",
    ),
    (
        "gitignore watcher",
        First,
        "/docs/boxes/viewer/live-updates/README.md",
    ),
    (
        "speaker notes",
        First,
        "/docs/boxes/viewer/making-slides/README.md#speaker-notes",
    ),
    (
        "auth proxy",
        First,
        "/docs/boxes/viewer/deploying/README.md#the-auth-proxy",
    ),
    (
        "hidden tabs",
        First,
        "/docs/boxes/viewer/page-script/README.md#hidden-tabs",
    ),
    (
        "gpu atlas",
        First,
        "/docs/boxes/viewer/design-engine/README.md#the-gpus-image-atlas",
    ),
    (
        "widget fails",
        First,
        "/docs/boxes/viewer/widgets/using-widgets/README.md#when-a-widget-fails",
    ),
    (
        "callout",
        First,
        "/docs/boxes/viewer/widgets/system-widgets/README.md#callout",
    ),
    (
        "retro",
        First,
        "/docs/boxes/engineering/sdlc/stages/README.md#retro",
    ),
    (
        "decision log",
        First,
        "/docs/boxes/engineering/sdlc/constraints/decisions-live-in-box-docs.md",
    ),
    ("bm25", First, "/docs/boxes/viewer/search/README.md"),
    ("pele", First, "/docs/boxes/viewer/vendored-pele/README.md"),
    (
        "topics section",
        Top3,
        "/docs/boxes/viewer/organizing-navigation/README.md#a-topics-section",
    ),
    (
        "narrator",
        Top3,
        "/docs/boxes/viewer/making-a-video/README.md",
    ),
    (
        "slide as an image",
        Top3,
        "/docs/boxes/viewer/making-slides/README.md#a-slide-as-an-image",
    ),
    (
        "served folder",
        Top3,
        "/docs/boxes/viewer/writing-a-document/README.md#the-served-folder",
    ),
    ("tantivy", Top3, "/docs/boxes/viewer/search/README.md"),
    ("mermaid", Top3, "/docs/boxes/viewer/diagrams/README.md"),
    ("pdf", Top3, "/docs/boxes/viewer/pdf/README.md"),
    ("video", Top3, "/docs/boxes/viewer/making-a-video/README.md"),
    ("install", Top3, "/docs/installing.md"),
    ("cargo install devpage", Top3, "/docs/installing.md"),
    (
        "devpage check",
        Top3,
        "/docs/boxes/viewer/checking-links/README.md",
    ),
    (
        "x-forwarded-email",
        Top3,
        "/docs/boxes/viewer/deploying/README.md",
    ),
    // A typo, and a word still being typed.
    (
        "navigaton tree",
        First,
        "/docs/boxes/viewer/navigation-tree/README.md",
    ),
    ("deplying", Top3, "/docs/boxes/viewer/deploying/README.md"),
    (
        "prototyp",
        Top3,
        "/docs/boxes/viewer/designs/README.md#prototypes",
    ),
    (
        "hidden ta",
        First,
        "/docs/boxes/viewer/page-script/README.md#hidden-tabs",
    ),
    // A code identifier.
    (
        "Search::changed",
        First,
        "/docs/boxes/viewer/search/README.md#how-it-stays-current",
    ),
    (
        "with_search",
        Top3,
        "/docs/boxes/viewer/search/README.md#how-it-stays-current",
    ),
    // A phrase, and a word left out.
    (
        "\"one stream per tab\"",
        First,
        "/docs/boxes/viewer/live-updates/README.md#why-one-stream-per-tab",
    ),
    (
        "design -engine",
        First,
        "/docs/boxes/viewer/designs/README.md",
    ),
    (
        "design -engine",
        Never,
        "/docs/boxes/viewer/design-engine/README.md",
    ),
    // A feature-catalog chapter is at the doc's address, never as a page.
    ("designs", Never, "/docs/features/designs.md"),
];

/// `devpage` serving the frozen docs on a free port, stopped when dropped.
struct Server {
    child: Child,
    address: String,
}

impl Server {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/search-docs");
        let mut child = Command::new(env!("CARGO_BIN_EXE_devpage"))
            .current_dir(&root)
            .args(["docs", "--port", "0", "--no-comments"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("devpage starts");
        // It prints `<title> → http://localhost:<port>` once it listens.
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout is piped"))
            .read_line(&mut line)
            .expect("devpage prints its address");
        let address = line
            .split_whitespace()
            .last()
            .expect("an address")
            .to_string();
        Self { child, address }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One page of the answer: its address and its sections' addresses.
struct Page {
    url: String,
    sections: Vec<String>,
}

/// Whether `page` is where `want` points: the page itself, or — for an
/// address with `#` — that section of it.
fn holds(page: &Page, want: &str) -> bool {
    let bare = |url: &str| url.split('#').next().unwrap_or(url).to_string();
    if want.contains('#') {
        page.url == want || page.sections.iter().any(|section| section == want)
    } else {
        page.url == want || bare(&page.url) == want
    }
}

/// `text` as a URL's query value: every byte but a letter or a digit escaped.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[tokio::test]
async fn search_finds_what_a_reader_means() {
    let server = Server::start();
    let client = reqwest::Client::new();
    let mut failures = Vec::new();
    let (mut reciprocal, mut top3, mut ranked) = (0.0, 0, 0);
    println!("{:<26} {:>5}  first result", "query", "rank");
    for &(query, want, url) in QUERIES {
        let text = client
            .get(format!(
                "{}/__devpage/search?q={}",
                server.address,
                encode(query)
            ))
            .send()
            .await
            .expect("the endpoint answers")
            .text()
            .await
            .expect("the answer is text");
        let answer: serde_json::Value = serde_json::from_str(&text).expect("the answer is JSON");
        let pages: Vec<Page> = answer["pages"]
            .as_array()
            .expect("the answer has pages")
            .iter()
            .map(|page| Page {
                url: page["url"].as_str().unwrap_or_default().to_string(),
                sections: page["sections"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|section| section["url"].as_str().unwrap_or_default().to_string())
                    .collect(),
            })
            .collect();
        let rank = pages
            .iter()
            .position(|page| holds(page, url))
            .map(|i| i + 1);
        let first = pages.first().map(|page| page.url.as_str()).unwrap_or("—");
        let passed = match want {
            First => rank == Some(1),
            Top3 => rank.is_some_and(|rank| rank <= 3),
            Never => rank.is_none(),
        };
        if want != Never {
            ranked += 1;
            reciprocal += rank.map_or(0.0, |rank| 1.0 / rank as f64);
            top3 += usize::from(rank.is_some_and(|rank| rank <= 3));
        }
        let shown = rank.map_or("—".to_string(), |rank| rank.to_string());
        println!("{query:<26} {shown:>5}  {first}");
        if !passed {
            let top: Vec<&str> = pages.iter().take(3).map(|page| page.url.as_str()).collect();
            failures.push(format!("{query:?}: {url} at {shown}; top {top:?}"));
        }
    }
    let mrr = reciprocal / ranked as f64;
    let success = top3 as f64 / ranked as f64;
    println!("MRR {mrr:.3}, success@3 {success:.3}, over {ranked} queries");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(mrr >= MRR_FLOOR, "MRR {mrr:.3} below {MRR_FLOOR}");
    assert!(
        success >= SUCCESS_AT_3_FLOOR,
        "success@3 {success:.3} below {SUCCESS_AT_3_FLOOR}"
    );
}
