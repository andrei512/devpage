//! What the real `devpage server` holds as versions are read, measured on
//! the process: a published version holds no search index until it is
//! searched, so it has as many threads after hundreds of versions are read as
//! after the first; and its memory holds at most so many versions' routers.
//! Each server runs in its own process, so its counts are the server's.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// `devpage server` on a free port with a bucket in a folder, stopped when
/// dropped.
struct Server {
    child: Child,
    address: String,
    /// Where it serves `/metrics`.
    metrics: String,
}

impl Server {
    fn start(dir: &std::path::Path) -> Self {
        Server::start_with(dir, &[])
    }

    /// The server, with `env` set besides.
    fn start_with(dir: &std::path::Path, env: &[(&str, &str)]) -> Self {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("bucket")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_devpage"))
            .arg("server")
            .env("DEVPAGE_DATA_DIR", dir.join("data"))
            .env(
                "DEVPAGE_BUCKET_URL",
                format!("file://{}", dir.join("bucket").display()),
            )
            .env("DEVPAGE_PORT", "0")
            .env("DEVPAGE_METRICS_PORT", "0")
            .envs(env.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        // It prints `devpage server on http://<address>` once it listens.
        let address = lines
            .find_map(|line| {
                line.ok()?
                    .strip_prefix("devpage server on http://")
                    .map(String::from)
            })
            .expect("the server prints its address");
        // Then `metrics on http://<address>/metrics`.
        let metrics = lines
            .find_map(|line| line.ok()?.strip_prefix("metrics on ").map(String::from))
            .expect("the server prints its metrics' address");
        Server {
            child,
            address,
            metrics,
        }
    }

    /// The total `name` from its metrics.
    async fn total(&self, http: &reqwest::Client, name: &str) -> f64 {
        let text = http
            .get(&self.metrics)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.trim().parse().ok())
            .unwrap_or_else(|| panic!("no {name} in {text}"))
    }

    /// The server's threads, as the system counts them.
    fn threads(&self) -> usize {
        let pid = self.child.id();
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
            return status
                .lines()
                .find_map(|line| line.strip_prefix("Threads:"))
                .and_then(|n| n.trim().parse().ok())
                .unwrap();
        }
        // macOS: one line per thread, after a header.
        let out = Command::new("ps")
            .args(["-M", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count() - 1
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn site(n: usize) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, text) in [
        (
            "devpage.yml",
            format!("kind: site\ntitle: Site {n}\nroot: README.md\n"),
        ),
        (
            "README.md",
            format!("# Site {n}\n\nA page.\n\n## Topics\n\n- [More](more.md)\n"),
        ),
        ("more.md", format!("# More {n}\n\nWords.\n")),
    ] {
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[tokio::test]
async fn reading_hundreds_of_versions_adds_no_threads() {
    let dir = std::env::temp_dir().join(format!("devpage-server-threads-{}", std::process::id()));
    let server = Server::start(&dir);
    let http = reqwest::Client::new();
    let read = |n: usize| {
        let (http, address) = (http.clone(), server.address.clone());
        async move {
            let published: serde_json::Value = http
                .post(format!("http://{address}/api/documents?entry=devpage.yml"))
                .body(site(n))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let id = published["id"].as_str().unwrap().to_string();
            let page = http
                .get(format!("http://{address}/d/{id}/README.md"))
                .send()
                .await
                .unwrap();
            assert_eq!(page.status(), 200);
            id
        }
    };
    // The first few versions start whatever the server starts once.
    for n in 0..5 {
        read(n).await;
    }
    let before = server.threads();
    let mut last = String::new();
    for n in 5..205 {
        last = read(n).await;
    }
    let after = server.threads();
    assert!(
        after <= before + 8,
        "{before} threads before 200 versions were read, {after} after"
    );
    // A search still answers, and a frozen index leaves no thread behind.
    let found: serde_json::Value = http
        .get(format!(
            "http://{}/d/{last}/__devpage/search?q=words",
            server.address
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(found["pages"][0]["url"], format!("/d/{last}/more.md"));
    assert!(server.threads() <= before + 8);
}

/// The real `devpage server`, its memory holding at most
/// `DEVPAGE_MEMORY_CACHE_VERSIONS` versions: past it, the routers it reports
/// stay at the limit however many more versions are read, and each still
/// answers.
#[tokio::test]
async fn past_its_memory_limit_the_routers_stay_flat() {
    let dir = std::env::temp_dir().join(format!("devpage-server-memory-{}", std::process::id()));
    let server = Server::start_with(&dir, &[("DEVPAGE_MEMORY_CACHE_VERSIONS", "20")]);
    let http = reqwest::Client::new();
    let mut ids = Vec::new();
    for n in 0..60 {
        let published: serde_json::Value = http
            .post(format!(
                "http://{}/api/documents?entry=devpage.yml",
                server.address
            ))
            .body(site(n))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = published["id"].as_str().unwrap().to_string();
        let page = http
            .get(format!("http://{}/d/{id}/README.md", server.address))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), 200);
        ids.push(id);
        let routers = server.total(&http, "devpage_versions_in_memory").await;
        assert!(routers <= 20.0, "{routers} versions in memory after {n}");
    }
    // The first, long let go, answers again from its files.
    let page = http
        .get(format!("http://{}/d/{}/README.md", server.address, ids[0]))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.text().await.unwrap().contains("Site 0"));
}
