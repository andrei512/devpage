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

/// The corpus at scale 1: 1,000 documents, 5,000 versions — two and a half
/// times the versions memory holds. Scale 10 is the envelope's 10,000.
const DOCUMENTS: f64 = 1000.0;
/// The envelope's numbers, which the scale never raises.
const OPEN_PAGES: f64 = 2000.0;
const HERD: f64 = 500.0;
const VIEWS: f64 = 50.0;
const BURST: f64 = 500.0;
const WRITES: f64 = 1.0;
const MISSES: f64 = 10.0;

pub const ALL: [&str; 7] = ["views", "misses", "writes", "herd", "open", "knee", "churn"];

/// How long each scenario loads the server, and how big the run is. The
/// load is driven by the work it does, never by waiting: a whole run, three
/// of each, schedules about four minutes of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    pub documents: usize,
    pub open_pages: usize,
    pub herd: usize,
    /// Seconds of each.
    pub views: f64,
    pub burst: f64,
    pub misses: f64,
    pub writes: f64,
    pub open: f64,
    /// Seconds at each rate the knee tries.
    pub knee_step: f64,
}

/// The rates the knee tries, doubling from 50 a second.
const KNEE_RATES: [f64; 8] = [50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0];

impl Plan {
    /// The seconds of load `runs` runs schedule, at most: the knee as if
    /// every kind met every rate; the herd and the churn as long as the
    /// server takes, so not counted.
    pub fn scheduled(&self, runs: usize) -> f64 {
        let knee = Kind::ALL.len() as f64 * KNEE_RATES.len() as f64 * self.knee_step;
        runs as f64 * (self.views + self.burst + self.misses + self.writes + self.open + knee)
    }
}

/// What the server held, sampled during the churn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub memory: Option<f64>,
    pub threads: Option<f64>,
    pub in_memory: Option<f64>,
}

/// How much memory and threads grew from when the memory cache was full —
/// the first sample holding the most versions it held — to the last
/// sample. Before that, memory fills with versions and the blocking pool
/// warms up, which is no leak; after it, anything that grows is one.
pub fn once_full(samples: &[Sample]) -> Option<(f64, f64)> {
    let most = samples
        .iter()
        .filter_map(|s| s.in_memory)
        .reduce(f64::max)?;
    let full = samples
        .iter()
        .position(|s| s.in_memory.is_some_and(|n| n >= most * 0.95))?;
    let last = samples.last()?;
    let ratio = |of: fn(&Sample) -> Option<f64>| match (of(&samples[full]), of(last)) {
        (Some(at), Some(end)) if at > 0.0 => Some(end / at),
        _ => None,
    };
    Some((ratio(|s| s.memory)?, ratio(|s| s.threads)?))
}

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
    /// The envelope's pages, opened once after the seeding and listening
    /// throughout the run.
    held: std::sync::OnceLock<Listeners>,
    /// What the server held, as the churn or the soak samples it.
    samples: Arc<Mutex<Vec<Sample>>>,
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
            held: std::sync::OnceLock::new(),
            samples: Arc::default(),
        })
    }

    /// The run's sizes and durations. The scale sets the corpus; above 1 it
    /// never stretches the time or raises the envelope's pages, and below 1
    /// it shrinks them too, for a short run.
    pub fn plan(&self) -> Plan {
        let short = self.scale.min(1.0);
        let seconds = |base: f64, floor: f64| (base * short).max(floor);
        let count = |base: f64, floor: f64| (base * short).max(floor).round() as usize;
        Plan {
            documents: (DOCUMENTS * self.scale).max(4.0).round() as usize,
            open_pages: count(OPEN_PAGES, 10.0),
            herd: count(HERD, 5.0),
            views: seconds(10.0, 2.0),
            burst: seconds(4.0, 1.0),
            misses: seconds(8.0, 2.0),
            writes: seconds(10.0, 3.0),
            open: seconds(10.0, 2.0),
            knee_step: seconds(1.0, 0.5),
        }
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
        let documents = self.plan().documents as u64;
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
            "churn" => self.churn(results).await,
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
        let steady = self.plan().views;
        let this = self.clone();
        let answers = load::open(VIEWS, (VIEWS * steady) as usize, |_| this.view()).await;
        file(results, "views", &answers);
        let burst = self.plan().burst;
        let answers = load::open(BURST, (BURST * burst) as usize, |_| this.view()).await;
        let answers = answers
            .into_iter()
            .map(|(class, answers)| (burst_of(class), answers))
            .collect();
        file(results, "views", &answers);
    }

    async fn misses(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.plan().misses;
        let this = self.clone();
        let answers = load::open(MISSES, (MISSES * seconds) as usize, |_| this.miss()).await;
        file(results, "misses", &answers);
    }

    async fn writes(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.plan().writes;
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
        let pages = self.plan().herd;
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

    /// Opens the envelope's 2,000 pages, each on a popular document, to
    /// listen for the rest of the run.
    pub async fn listen(&self, results: &mut Results) {
        let pages = self.plan().open_pages;
        let urls = (0..pages)
            .map(|_| self.events(&self.docs[self.popular(|_| true).unwrap_or(0)]))
            .collect();
        let listeners = Listeners::open(&self.streams, urls, Duration::from_secs(60)).await;
        results.record(
            "open.connected",
            Unit::Count,
            Some(Target::AtLeast(pages as f64)),
            listeners.open_now() as f64,
        );
        let _ = self.held.set(listeners);
    }

    /// The pages opened by [`Bench::listen`] still open, filed as
    /// `<scenario>.still open`.
    fn still_open(&self, results: &mut Results, scenario: &str) {
        let pages = self.plan().open_pages;
        results.record(
            &format!("{scenario}.still open"),
            Unit::Count,
            Some(Target::AtLeast(pages as f64)),
            self.held.get().map_or(0, Listeners::open_now) as f64,
        );
    }

    /// Readers reading while the pages listen.
    async fn open_pages(self: &Arc<Self>, results: &mut Results) {
        let seconds = self.plan().open;
        let this = self.clone();
        let answers = load::open(VIEWS, (VIEWS * seconds) as usize, |_| this.view()).await;
        file(results, "open", &answers);
        self.still_open(results, "open");
    }

    /// Views of each kind of document, doubled from 50 a second until their
    /// p99 crosses the target: the last rate that met it.
    async fn knee(self: &Arc<Self>, results: &mut Results) {
        let step = self.plan().knee_step;
        for kind in Kind::ALL {
            if !self.docs.iter().any(|doc| doc.kind == kind) {
                continue;
            }
            let mut met = 0.0;
            for rate in KNEE_RATES {
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
            }
            results.record(&format!("knee.{}", kind.name()), Unit::PerSecond, None, met);
        }
    }

    /// Every version read once, a page of each, as fast as the server
    /// answers, while the pages listen: memory and threads from when the
    /// memory cache is full to the end, and the pages still open.
    async fn churn(self: &Arc<Self>, results: &mut Results) {
        let urls: Vec<String> = self
            .docs
            .iter()
            .flat_map(|doc| {
                let link = self.link(doc);
                let page = doc.pages[0].clone();
                (1..=doc.versions.load(Ordering::SeqCst))
                    .map(move |v| format!("{link}/v/{v}/{page}"))
            })
            .collect();
        let started = Instant::now();
        let sampler = self.sample(Duration::from_secs(1));
        let reads: Vec<(Duration, bool)> = futures::stream::iter(urls)
            .map(|url| {
                let client = self.client.clone();
                async move {
                    let at = Instant::now();
                    let ok = load::get(client, url).await;
                    (at.elapsed(), ok)
                }
            })
            .buffer_unordered(16)
            .collect()
            .await;
        let reads = Latencies::of(reads, started.elapsed());
        sampler.abort();
        let samples = self.samples.lock().unwrap().clone();
        results.record(
            "churn.versions read",
            Unit::Count,
            None,
            reads.sorted.len() as f64,
        );
        results.record("churn.read", Unit::PerSecond, None, reads.per_second());
        results.record("churn.read p99", Unit::Ms, None, ms(reads.at(0.99)));
        results.record(
            "churn.read errors",
            Unit::Ratio,
            Some(Target::AtMost(ERRORS)),
            reads.error_ratio(),
        );
        if let Some(most) = samples.iter().filter_map(|s| s.in_memory).reduce(f64::max) {
            results.record("churn.versions in memory, most", Unit::Count, None, most);
        }
        self.held_memory(results, "churn", &samples);
        self.still_open(results, "churn");
    }

    /// Samples what the server holds every `every`, into `samples`, until
    /// aborted.
    fn sample(&self, every: Duration) -> tokio::task::JoinHandle<()> {
        self.samples.lock().unwrap().clear();
        let (client, url) = (self.client.clone(), self.metrics.clone());
        let samples = self.samples.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(scrape) = metrics::scrape(&client, &url).await {
                    samples.lock().unwrap().push(Sample {
                        memory: metrics::total(&scrape, "resident"),
                        threads: metrics::total(&scrape, "thread"),
                        in_memory: metrics::total(&scrape, "versions_in_memory"),
                    });
                }
                tokio::time::sleep(every).await;
            }
        })
    }

    /// Memory at its highest, and memory and threads grown from when the
    /// memory cache was full ([`once_full`]), filed under `scenario`.
    fn held_memory(&self, results: &mut Results, scenario: &str, samples: &[Sample]) {
        let key = |number: &str| format!("{scenario}.{number}");
        let limit = Some(Target::AtMost(self.memory_limit));
        match samples.iter().filter_map(|s| s.memory).reduce(f64::max) {
            Some(max) => results.record(&key("memory max"), Unit::Bytes, limit, max),
            None => results.missing(&key("memory max"), Unit::Bytes, limit),
        }
        let growth = Some(Target::AtMost(1.1));
        match once_full(samples) {
            Some((memory, threads)) => {
                results.record(&key("memory growth once full"), Unit::Ratio, growth, memory);
                results.record(
                    &key("thread growth once full"),
                    Unit::Ratio,
                    growth,
                    threads,
                );
            }
            None => {
                results.missing(&key("memory growth once full"), Unit::Ratio, growth);
                results.missing(&key("thread growth once full"), Unit::Ratio, growth);
            }
        }
    }

    /// Asked for by its length: the whole envelope at its rates for
    /// `minutes` — readers, misses, publishes and comments, with the pages
    /// listening — for leaks the work-driven scenarios can't reach.
    pub async fn soak(self: &Arc<Self>, results: &mut Results, minutes: f64) {
        let seconds = minutes * 60.0;
        let sampler = self.sample(Duration::from_secs_f64((seconds / 60.0).max(2.0)));
        let (a, b, c, d) = (self.clone(), self.clone(), self.clone(), self.clone());
        let (views, publishes, comments, misses) = tokio::join!(
            load::open(VIEWS, (VIEWS * seconds) as usize, |_| a.view()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| b.publish_one()),
            load::open(WRITES, (WRITES * seconds) as usize, |_| c.comment()),
            load::open(2.0, (2.0 * seconds) as usize, |_| d.miss()),
        );
        sampler.abort();
        file(results, "soak", &views);
        file(results, "soak", &publishes);
        file(results, "soak", &comments);
        let mut misses = misses;
        if let Some(page) = misses.remove("page") {
            file(results, "soak", &BTreeMap::from([("miss", page)]));
        }
        self.still_open(results, "soak");
        let samples = self.samples.lock().unwrap().clone();
        self.held_memory(results, "soak", &samples);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(scale: f64) -> Bench {
        Bench::new("http://127.0.0.1:1".into(), String::new(), 1, scale, 4e9).unwrap()
    }

    #[test]
    fn the_scale_sets_the_corpus_and_never_the_envelopes_pages() {
        let plan = bench(1.0).plan();
        assert_eq!(plan.documents, 1000);
        assert_eq!((plan.open_pages, plan.herd), (2000, 500));
        let plan = bench(10.0).plan();
        assert_eq!(plan.documents, 10_000);
        assert_eq!((plan.open_pages, plan.herd), (2000, 500));
        assert_eq!(
            plan.views,
            bench(1.0).plan().views,
            "the scale never stretches time"
        );
        // Below 1, a short run: the pages and the durations shrink too.
        let short = bench(0.01).plan();
        assert_eq!(short.documents, 10);
        assert!(short.open_pages < 2000 && short.views < bench(1.0).plan().views);
    }

    #[test]
    fn a_default_run_schedules_about_four_minutes_of_load() {
        let load = bench(1.0).plan().scheduled(3);
        assert!(
            (120.0..=240.0).contains(&load),
            "three runs schedule {load:.0} s of load"
        );
    }

    fn sample(memory: f64, threads: f64, in_memory: f64) -> Sample {
        Sample {
            memory: Some(memory),
            threads: Some(threads),
            in_memory: Some(in_memory),
        }
    }

    #[test]
    fn growth_is_judged_from_when_memory_is_full() {
        // Threads grow while the blocking pool warms up, memory while the
        // versions fill it; once full, both hold.
        let flat = [
            sample(100.0, 4.0, 10.0),
            sample(400.0, 16.0, 1000.0),
            sample(600.0, 17.0, 2000.0),
            sample(610.0, 17.0, 2000.0),
            sample(620.0, 18.0, 2000.0),
        ];
        let (memory, threads) = once_full(&flat).unwrap();
        assert!(memory < 1.1 && threads < 1.1, "{memory} {threads}");
        // Memory still climbing once full is a leak.
        let leaking = [
            sample(100.0, 4.0, 10.0),
            sample(600.0, 16.0, 2000.0),
            sample(900.0, 16.0, 2000.0),
        ];
        assert!(once_full(&leaking).unwrap().0 > 1.1);
        // Full only at the last sample, as when the corpus fits in memory:
        // flat from there by definition; the report says how many were read.
        assert_eq!(
            once_full(&[sample(1.0, 1.0, 1.0), sample(2.0, 1.0, 5.0)]),
            Some((1.0, 1.0))
        );
        assert!(once_full(&[]).is_none());
    }
}
