//! The scenarios: each loads the server one way, at the envelope's rates,
//! and files what it measured under its name. The corpus is published once,
//! first; the scenarios then read it, add to it and listen to it.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use serde_json::json;
use tokio::time::Instant;

use crate::corpus::{self, Kind, Seeded};
use crate::load::{self, Latencies, Listeners};
use crate::metrics;
use crate::report::{Results, Target, Unit};

/// A page view's target, at p99, whether its version is in the cache or not.
const VIEW: f64 = 100.0;
/// A publish's, at p99.
const PUBLISH: f64 = 2000.0;
/// A comment's, at p99.
const COMMENT: f64 = 200.0;
/// How long after a publish answers every open page shows the new version.
const SHOWN: f64 = 1000.0;
/// The share of requests that may fail.
const ERRORS: f64 = 0.001;

/// The envelope's numbers, before the scale.
const DOCUMENTS: f64 = 10_000.0;
const OPEN_PAGES: f64 = 2000.0;
const HERD: f64 = 500.0;
const VIEWS: f64 = 50.0;
const BURST: f64 = 500.0;
const WRITES: f64 = 1.0;
const MISSES: f64 = 10.0;

pub const ALL: [&str; 7] = ["views", "misses", "writes", "herd", "open", "knee", "soak"];

/// A published document.
pub struct Doc {
    pub id: String,
    /// Its number in the corpus; `None` for a folder of real documents.
    pub n: Option<u64>,
    pub kind: Kind,
    pub pages: Vec<String>,
    /// Its latest version's number.
    pub versions: AtomicU64,
}

pub struct Bench {
    pub client: reqwest::Client,
    /// For live-update streams, which never end: no timeout.
    pub streams: reqwest::Client,
    pub server: String,
    pub metrics: String,
    pub seed: u64,
    pub scale: f64,
    pub memory_limit: f64,
    pub docs: Vec<Doc>,
    /// Cumulative popularity, by the docs' order: a few read often, most
    /// rarely — Zipf, exponent 1.
    weights: Vec<f64>,
    rng: Mutex<Seeded>,
    /// The versions read so far, which a miss must not be.
    read: Mutex<HashSet<(usize, u64)>>,
}

impl Bench {
    pub fn new(
        server: String,
        metrics: String,
        seed: u64,
        scale: f64,
        memory_limit: f64,
    ) -> Result<Self> {
        Ok(Bench {
            client: reqwest::Client::builder()
                .pool_max_idle_per_host(1024)
                .timeout(Duration::from_secs(60))
                .build()?,
            streams: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .build()?,
            server: server.trim_end_matches('/').to_string(),
            metrics,
            seed,
            scale,
            memory_limit,
            docs: Vec::new(),
            weights: Vec::new(),
            rng: Mutex::new(Seeded::new(seed ^ 0xBEEF)),
            read: Mutex::default(),
        })
    }

    /// `base` seconds of the envelope's, scaled, never under `floor`.
    fn seconds(&self, base: f64, floor: f64) -> f64 {
        (base * self.scale).max(floor)
    }

    /// The envelope's `count` of something, scaled, never under `floor`.
    fn count(&self, base: f64, floor: f64) -> usize {
        (base * self.scale).max(floor).round() as usize
    }

    fn random(&self, n: usize) -> usize {
        self.rng.lock().unwrap().below(n as u64) as usize
    }

    /// A document by popularity, among those `keep` allows.
    fn popular(&self, keep: impl Fn(&Doc) -> bool) -> Option<usize> {
        let at = self.rng.lock().unwrap().unit() * self.weights.last().copied().unwrap_or(1.0);
        let start = self.weights.partition_point(|w| *w < at);
        (start..self.docs.len())
            .chain(0..start)
            .find(|&i| keep(&self.docs[i]))
    }

    fn link(&self, doc: &Doc) -> String {
        format!("{}/d/{}", self.server, doc.id)
    }

    async fn publish(&self, id: Option<&str>, version: corpus::Version) -> Result<String> {
        let url = match id {
            None => format!("{}/api/documents?entry={}", self.server, version.entry),
            Some(id) => format!(
                "{}/api/documents/{id}/versions?entry={}",
                self.server, version.entry
            ),
        };
        let answer = self
            .client
            .post(url)
            .header("content-type", "application/zip")
            .body(version.zip)
            .send()
            .await?;
        if !answer.status().is_success() {
            bail!("publish: {} {}", answer.status(), answer.text().await?);
        }
        let body: serde_json::Value = answer.json().await?;
        Ok(match id {
            Some(id) => id.to_string(),
            None => body["id"].as_str().context("publish: no id")?.to_string(),
        })
    }

    /// The corpus, published eight documents at a time — each document's
    /// versions in order — then every latest page read once.
    pub async fn seed(
        &mut self,
        results: &mut Results,
        folder: Option<corpus::Version>,
    ) -> Result<()> {
        let documents = self.count(DOCUMENTS, 4.0) as u64;
        let before = metrics::scrape(&self.client, &self.metrics).await.ok();
        let started = Instant::now();
        let this = &*self;
        let mut published: Vec<(Doc, Vec<Duration>)> = futures::stream::iter(0..documents)
            .map(|n| async move {
                let (kind, versions) = corpus::shape(this.seed, n);
                let mut took = Vec::new();
                let mut id = None;
                for v in 1..=versions {
                    let at = Instant::now();
                    let got = this
                        .publish(id.as_deref(), corpus::version(this.seed, n, v))
                        .await?;
                    took.push(at.elapsed());
                    id = Some(got);
                }
                anyhow::Ok((
                    Doc {
                        id: id.context("a document has a version")?,
                        n: Some(n),
                        kind,
                        pages: corpus::pages(this.seed, n),
                        versions: AtomicU64::new(versions),
                    },
                    took,
                ))
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<_>>()?;
        if let Some(folder) = folder {
            let pages = folder.pages.clone();
            let at = Instant::now();
            let id = self.publish(None, folder).await?;
            published.push((
                Doc {
                    id,
                    n: None,
                    kind: Kind::Site,
                    pages,
                    versions: AtomicU64::new(1),
                },
                vec![at.elapsed()],
            ));
        }
        let wall = started.elapsed();
        // The corpus's order, whatever order the publishes finished in.
        published.sort_by_key(|(doc, _)| doc.n.unwrap_or(u64::MAX));
        let took: Vec<_> = published
            .iter()
            .flat_map(|(_, took)| took.iter().map(|d| (*d, true)))
            .collect();
        let publishes = Latencies::of(took, wall);
        results.record(
            "seeding.documents",
            Unit::Count,
            None,
            published.len() as f64,
        );
        results.record(
            "seeding.versions",
            Unit::Count,
            None,
            publishes.sorted.len() as f64,
        );
        results.record(
            "seeding.publishes",
            Unit::PerSecond,
            None,
            publishes.per_second(),
        );
        results.record(
            "seeding.publish p99",
            Unit::Ms,
            Some(Target::AtMost(PUBLISH)),
            ms(publishes.at(0.99)),
        );
        self.docs = published.into_iter().map(|(doc, _)| doc).collect();
        let mut sum = 0.0;
        self.weights = (1..=self.docs.len())
            .map(|rank| {
                sum += 1.0 / rank as f64;
                sum
            })
            .collect();
        // Every latest version read once: the views measure the served path,
        // the misses the versions no one has read.
        let urls: Vec<String> = self
            .docs
            .iter()
            .flat_map(|doc| {
                doc.pages
                    .iter()
                    .map(|page| format!("{}/{page}", self.link(doc)))
            })
            .collect();
        futures::stream::iter(urls)
            .map(|url| load::get(self.client.clone(), url))
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;
        {
            let mut read = self.read.lock().unwrap();
            for (i, doc) in self.docs.iter().enumerate() {
                read.insert((i, doc.versions.load(Ordering::SeqCst)));
            }
        }
        if let (Some(before), Ok(after)) =
            (before, metrics::scrape(&self.client, &self.metrics).await)
        {
            results.timed("seeding", metrics::between(&before, &after));
        }
        Ok(())
    }

    /// A reader's request: a page of a popular document's latest version,
    /// now and then a site's search, a design's PNG or a download.
    fn view(&self) -> (&'static str, impl Future<Output = bool> + Send + 'static) {
        let i = self.popular(|_| true).unwrap_or(0);
        let doc = &self.docs[i];
        // Read now, so never a miss later, once a newer version is out.
        self.read
            .lock()
            .unwrap()
            .insert((i, doc.versions.load(Ordering::SeqCst)));
        let link = self.link(doc);
        let page = &doc.pages[self.random(doc.pages.len())];
        let roll = self.random(20);
        let (class, url) = match doc.kind {
            Kind::Site if roll == 0 => ("search", format!("{link}/__devpage/search?q=version")),
            Kind::Design if roll < 5 => ("png", format!("{link}/{page}.png")),
            kind if kind.downloads() && roll == 0 => (
                "download",
                format!("{link}/v/{}/download", doc.versions.load(Ordering::SeqCst)),
            ),
            _ => ("page", format!("{link}/{page}")),
        };
        (class, load::get(self.client.clone(), url))
    }

    /// A first view of a version no one has read: from the bucket, unpacked.
    fn miss(&self) -> (&'static str, impl Future<Output = bool> + Send + 'static) {
        let mut url = None;
        for _ in 0..64 {
            let i = self.random(self.docs.len());
            let doc = &self.docs[i];
            let latest = doc.versions.load(Ordering::SeqCst);
            if latest < 2 {
                continue;
            }
            let v = 1 + self.random(latest as usize - 1) as u64;
            if self.read.lock().unwrap().insert((i, v)) {
                let page = &doc.pages[self.random(doc.pages.len())];
                url = Some(format!("{}/v/{v}/{page}", self.link(doc)));
                break;
            }
        }
        // Every old version read already: a repeat, counted as one.
        let url = url.unwrap_or_else(|| {
            let doc = &self.docs[self.random(self.docs.len())];
            format!("{}/{}", self.link(doc), doc.pages[0])
        });
        ("page", load::get(self.client.clone(), url))
    }

    /// A new version of a generated document.
    fn publish_one(
        self: &Arc<Self>,
    ) -> (&'static str, impl Future<Output = bool> + Send + 'static) {
        let i = self.popular(|doc| doc.n.is_some()).unwrap_or(0);
        let this = self.clone();
        ("publish", async move {
            let doc = &this.docs[i];
            let v = doc.versions.fetch_add(1, Ordering::SeqCst) + 1;
            let version = corpus::version(this.seed, doc.n.unwrap_or(0), v);
            this.publish(Some(&doc.id), version).await.is_ok()
        })
    }

    /// A thread started on a popular document's latest version, quoting
    /// "the", which every generated page has.
    fn comment(&self) -> (&'static str, impl Future<Output = bool> + Send + 'static) {
        let i = self
            .popular(|doc| doc.n.is_some() && doc.kind != Kind::Design)
            .unwrap_or(0);
        let doc = &self.docs[i];
        let body = json!({
            "page": "page-0.md",
            "anchor": { "quote": "the", "prefix": "", "suffix": "" },
            "text": "A reader's comment.",
        });
        let request = self
            .client
            .post(format!("{}/api/documents/{}/comments", self.server, doc.id))
            .json(&body);
        ("comment", async move {
            matches!(request.send().await, Ok(answer) if answer.status().is_success())
        })
    }

    fn events(&self, doc: &Doc) -> String {
        format!("{}/__devpage/events", self.link(doc))
    }

    /// Runs `scenario`, filing the stages `/metrics` says it spent its time in.
    pub async fn run(self: &Arc<Self>, scenario: &str, results: &mut Results) -> Result<()> {
        let before = metrics::scrape(&self.client, &self.metrics).await.ok();
        match scenario {
            "views" => self.views(results).await,
            "misses" => self.misses(results).await,
            "writes" => self.writes(results).await,
            "herd" => self.herd(results).await?,
            "open" => self.open_pages(results).await,
            "knee" => self.knee(results).await,
            "soak" => self.soak(results).await,
            other => bail!("no scenario {other}: the scenarios are {}", ALL.join(", ")),
        }
        let after = metrics::scrape(&self.client, &self.metrics).await.ok();
        match (before, after) {
            (Some(before), Some(after)) => {
                results.timed(scenario, metrics::between(&before, &after));
                results.totals = after.totals;
            }
            _ => eprintln!("{scenario}: no metrics at {}", self.metrics),
        }
        Ok(())
    }

    async fn views(self: &Arc<Self>, results: &mut Results) {
        let steady = self.seconds(60.0, 5.0);
        let this = self.clone();
        let answers = load::open(VIEWS, (VIEWS * steady) as usize, |_| this.view()).await;
        file(results, "views", &answers);
        let burst = self.seconds(30.0, 3.0);
        let answers = load::open(BURST, (BURST * burst) as usize, |_| this.view()).await;
        let answers = answers
            .into_iter()
            .map(|(class, answers)| (burst_of(class), answers))
            .collect();
        file(results, "views", &answers);
    }

    async fn misses(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.seconds(30.0, 5.0);
        let this = self.clone();
        let answers = load::open(MISSES, (MISSES * seconds) as usize, |_| this.miss()).await;
        file(results, "misses", &answers);
    }

    async fn writes(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.seconds(60.0, 10.0);
        let (a, b, c) = (self.clone(), self.clone(), self.clone());
        let (views, publishes, comments) = tokio::join!(
            load::open(VIEWS, (VIEWS * seconds) as usize, |_| a.view()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| b.publish_one()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| c.comment()),
        );
        file(results, "writes", &views);
        file(results, "writes", &publishes);
        file(results, "writes", &comments);
    }

    /// 500 pages open on one document, and a publish to it: how long after
    /// the publish answers every one of them has the new version.
    async fn herd(self: &Arc<Self>, results: &mut Results) -> Result<()> {
        let pages = self.count(HERD, 5.0);
        let i = self
            .docs
            .iter()
            .position(|doc| doc.kind == Kind::Site && doc.n.is_some())
            .context("the corpus has no site")?;
        let doc = &self.docs[i];
        let listeners = Listeners::open(
            &self.streams,
            vec![self.events(doc); pages],
            Duration::from_secs(30),
        )
        .await;
        results.record(
            "herd.connected",
            Unit::Count,
            Some(Target::AtLeast(pages as f64)),
            listeners.open_now() as f64,
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        let heard_before: Vec<usize> = listeners.heard.iter().map(|h| *h.borrow()).collect();
        let link = self.link(doc);
        let page = format!("{link}/{}", doc.pages[0]);
        let list = format!("{link}/__devpage/versions");
        // Each page, on hearing of the publish, fetches what a reader's page
        // does: the page again, and the versions.
        let refetches: Vec<_> = listeners
            .heard
            .iter()
            .zip(heard_before)
            .map(|(heard, before)| {
                let mut heard = heard.clone();
                let (client, page, list) = (self.client.clone(), page.clone(), list.clone());
                tokio::spawn(async move {
                    let told = tokio::time::timeout(Duration::from_secs(30), async {
                        while *heard.borrow_and_update() <= before {
                            if heard.changed().await.is_err() {
                                return false;
                            }
                        }
                        true
                    })
                    .await
                    .unwrap_or(false);
                    let ok = told
                        && load::get(client.clone(), page).await
                        && load::get(client, list).await;
                    (Instant::now(), ok)
                })
            })
            .collect();
        let at = Instant::now();
        let v = doc.versions.fetch_add(1, Ordering::SeqCst) + 1;
        let published = self
            .publish(
                Some(&doc.id),
                corpus::version(self.seed, doc.n.unwrap_or(0), v),
            )
            .await;
        let answered = Instant::now();
        results.record(
            "herd.publish",
            Unit::Ms,
            Some(Target::AtMost(PUBLISH)),
            ms(answered - at),
        );
        let mut shown = Vec::new();
        for refetch in refetches {
            if let Ok((done, ok)) = refetch.await {
                shown.push((done.saturating_duration_since(answered), ok));
            }
        }
        let shown = Latencies::of(shown, answered.elapsed());
        results.record(
            "herd.not shown",
            Unit::Count,
            Some(Target::AtMost(0.0)),
            (shown.errors + pages - shown.sorted.len()) as f64 + f64::from(published.is_err()),
        );
        results.record(
            "herd.all shown",
            Unit::Ms,
            Some(Target::AtMost(SHOWN)),
            ms(shown.at(1.0)),
        );
        Ok(())
    }

    /// 2,000 pages listening throughout, while readers read.
    async fn open_pages(self: &Arc<Self>, results: &mut Results) {
        let pages = self.count(OPEN_PAGES, 10.0);
        let urls = (0..pages)
            .map(|_| self.events(&self.docs[self.popular(|_| true).unwrap_or(0)]))
            .collect();
        let listeners = Listeners::open(&self.streams, urls, Duration::from_secs(60)).await;
        let target = Some(Target::AtLeast(pages as f64));
        results.record(
            "open.connected",
            Unit::Count,
            target,
            listeners.open_now() as f64,
        );
        let seconds = self.seconds(60.0, 5.0);
        let this = self.clone();
        let answers = load::open(VIEWS, (VIEWS * seconds) as usize, |_| this.view()).await;
        file(results, "open", &answers);
        results.record(
            "open.still open",
            Unit::Count,
            target,
            listeners.open_now() as f64,
        );
    }

    /// Views of each kind of document, doubled from 50 a second until their
    /// p99 crosses the target: the last rate that met it.
    async fn knee(self: &Arc<Self>, results: &mut Results) {
        let step = self.seconds(10.0, 2.0);
        for kind in Kind::ALL {
            if !self.docs.iter().any(|doc| doc.kind == kind) {
                continue;
            }
            let mut met = 0.0;
            let mut rate = 50.0;
            while rate <= 6400.0 {
                let this = self.clone();
                let answers = load::open(rate, (rate * step) as usize, |_| {
                    let i = this.popular(|doc| doc.kind == kind).unwrap_or(0);
                    let doc = &this.docs[i];
                    let page = &doc.pages[this.random(doc.pages.len())];
                    (
                        "page",
                        load::get(this.client.clone(), format!("{}/{page}", this.link(doc))),
                    )
                })
                .await;
                let page = &answers["page"];
                if ms(page.at(0.99)) > VIEW || page.error_ratio() > ERRORS {
                    break;
                }
                met = rate;
                rate *= 2.0;
            }
            results.record(&format!("knee.{}", kind.name()), Unit::PerSecond, None, met);
        }
    }

    /// The whole envelope at once for 30 minutes — pages open, readers,
    /// misses, publishes and comments — and what the server holds as it goes.
    async fn soak(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.seconds(1800.0, 20.0);
        let pages = self.count(OPEN_PAGES, 10.0);
        let urls = (0..pages)
            .map(|_| self.events(&self.docs[self.popular(|_| true).unwrap_or(0)]))
            .collect();
        let listeners = Listeners::open(&self.streams, urls, Duration::from_secs(60)).await;
        let every = Duration::from_secs_f64((seconds / 60.0).max(2.0));
        let until = Instant::now() + Duration::from_secs_f64(seconds);
        let sampler = {
            let (client, url) = (self.client.clone(), self.metrics.clone());
            tokio::spawn(async move {
                let mut samples = Vec::new();
                while Instant::now() < until {
                    if let Ok(scrape) = metrics::scrape(&client, &url).await {
                        samples.push((
                            metrics::total(&scrape, "resident"),
                            metrics::total(&scrape, "thread"),
                        ));
                    }
                    tokio::time::sleep(every).await;
                }
                samples
            })
        };
        let (a, b, c, d) = (self.clone(), self.clone(), self.clone(), self.clone());
        let (views, publishes, comments, misses) = tokio::join!(
            load::open(VIEWS, (VIEWS * seconds) as usize, |_| a.view()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| b.publish_one()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| c.comment()),
            load::open(2.0, (2.0 * seconds) as usize, |_| d.miss()),
        );
        file(results, "soak", &views);
        file(results, "soak", &publishes);
        file(results, "soak", &comments);
        let mut misses = misses;
        if let Some(page) = misses.remove("page") {
            file(results, "soak", &BTreeMap::from([("miss", page)]));
        }
        results.record(
            "soak.still open",
            Unit::Count,
            Some(Target::AtLeast(pages as f64)),
            listeners.open_now() as f64,
        );
        let samples = sampler.await.unwrap_or_default();
        let memory: Vec<f64> = samples.iter().filter_map(|(m, _)| *m).collect();
        let threads: Vec<f64> = samples.iter().filter_map(|(_, t)| *t).collect();
        let limit = Some(Target::AtMost(self.memory_limit));
        match memory.iter().copied().reduce(f64::max) {
            Some(max) => results.record("soak.memory max", Unit::Bytes, limit, max),
            None => results.missing("soak.memory max", Unit::Bytes, limit),
        }
        let growth = Some(Target::AtMost(1.1));
        match (threads.first(), threads.last()) {
            (Some(first), Some(last)) if *first > 0.0 => {
                results.record("soak.thread growth", Unit::Ratio, growth, last / first)
            }
            _ => results.missing("soak.thread growth", Unit::Ratio, growth),
        }
    }
}

/// A class of request, in the burst.
fn burst_of(class: &str) -> &'static str {
    match class {
        "page" => "burst page",
        "search" => "burst search",
        "png" => "burst png",
        _ => "burst download",
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Files each class of answers under `scenario`: p50, p99, the rate
/// answered and the share that failed.
fn file(results: &mut Results, scenario: &str, answers: &BTreeMap<&'static str, Latencies>) {
    for (class, answers) in answers {
        let target = match *class {
            "page" | "miss" | "burst page" => Some(Target::AtMost(VIEW)),
            "publish" => Some(Target::AtMost(PUBLISH)),
            "comment" => Some(Target::AtMost(COMMENT)),
            _ => None,
        };
        let key = |number: &str| format!("{scenario}.{class} {number}");
        results.record(&key("p50"), Unit::Ms, None, ms(answers.at(0.5)));
        results.record(&key("p99"), Unit::Ms, target, ms(answers.at(0.99)));
        results.record(
            &key("answered"),
            Unit::PerSecond,
            None,
            answers.per_second(),
        );
        results.record(
            &key("errors"),
            Unit::Ratio,
            Some(Target::AtMost(ERRORS)),
            answers.error_ratio(),
        );
    }
}
