//! devpage-bench: publishes a seeded corpus to a server, loads it with page
//! views sent at their scheduled times, and reports the numbers beside the
//! envelope's targets, with the time the server says each stage took.

mod corpus;
mod report;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// The envelope's target for a page view, at p99.
const VIEW_TARGET: Duration = Duration::from_millis(100);

struct Args {
    server: String,
    metrics: String,
    seed: u64,
    documents: u64,
    rate: u64,
    seconds: u64,
}

fn args() -> Result<Args> {
    let mut given: HashMap<String, String> = HashMap::new();
    let mut words = std::env::args().skip(1);
    while let Some(flag) = words.next() {
        let name = flag.strip_prefix("--").context("flags are --name value")?;
        given.insert(
            name.to_string(),
            words.next().context("a flag needs a value")?,
        );
    }
    let number = |name: &str, default: u64| -> Result<u64> {
        given
            .get(name)
            .map(|v| {
                v.parse()
                    .with_context(|| format!("--{name} is not a number"))
            })
            .unwrap_or(Ok(default))
    };
    Ok(Args {
        server: given
            .get("server")
            .cloned()
            .unwrap_or("http://127.0.0.1:3030".into()),
        metrics: given
            .get("metrics")
            .cloned()
            .unwrap_or("http://127.0.0.1:9030/metrics".into()),
        seed: number("seed", 1)?,
        documents: number("documents", 100)?,
        rate: number("rate", 50)?,
        seconds: number("seconds", 20)?,
    })
}

/// A published document: its link and the pages a reader opens.
struct Published {
    link: String,
    pages: Vec<String>,
}

async fn publish(
    client: &reqwest::Client,
    server: &str,
    doc: corpus::Generated,
) -> Result<(Published, Duration)> {
    let started = Instant::now();
    let answer = client
        .post(format!("{server}/api/documents?entry={}", doc.entry))
        .header("content-type", "application/zip")
        .body(doc.zip)
        .send()
        .await?;
    let took = started.elapsed();
    if !answer.status().is_success() {
        bail!("publish: {} {}", answer.status(), answer.text().await?);
    }
    let body: serde_json::Value = answer.json().await?;
    let id = body["id"].as_str().context("publish: no id")?;
    Ok((
        Published {
            link: format!("{server}/d/{id}/"),
            pages: doc.pages,
        },
        took,
    ))
}

/// A document picked by a popularity curve — Zipf, exponent 1 — given its
/// cumulative `weights`: a few read often, most rarely.
fn popular(rng: &mut corpus::Seeded, weights: &[f64]) -> usize {
    let total = weights.last().copied().unwrap_or(1.0);
    let at = (rng.next() as f64 / u64::MAX as f64) * total;
    weights.partition_point(|w| *w < at).min(weights.len() - 1)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = args()?;
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(256)
        .build()?;

    // The corpus, published eight at a time.
    let started = Instant::now();
    let mut published = Vec::new();
    let mut publishes = Vec::new();
    let mut n = 0;
    while n < args.documents {
        let batch: Vec<_> = (n..(n + 8).min(args.documents))
            .map(|i| publish(&client, &args.server, corpus::document(args.seed, i)))
            .collect();
        for done in futures::future::join_all(batch).await {
            let (doc, took) = done?;
            published.push(doc);
            publishes.push(took);
        }
        n += 8;
    }
    let seeding = started.elapsed();

    // Every page read once, so the views below measure the served path and
    // not the first unpack.
    for doc in &published {
        for page in &doc.pages {
            client
                .get(format!("{}{page}", doc.link))
                .send()
                .await?
                .bytes()
                .await?;
        }
    }

    let before = report::scrape(&client, &args.metrics).await?;
    let views = views(&client, &published, &args).await;
    let after = report::scrape(&client, &args.metrics).await?;

    println!(
        "{}",
        report::render(&report::Run {
            documents: published.len(),
            seeding,
            publishes,
            views,
            rate: args.rate,
            seconds: args.seconds,
            target: VIEW_TARGET,
            stages: report::stages(&before, &after),
        })
    );
    Ok(())
}

/// Page views at `rate` a second for `seconds`, each sent at its scheduled
/// time whether or not the ones before have answered, timed from then.
async fn views(
    client: &reqwest::Client,
    published: &[Published],
    args: &Args,
) -> report::Latencies {
    let mut rng = corpus::Seeded::new(args.seed ^ 0xBEEF);
    let mut sum = 0.0;
    let weights: Vec<f64> = (1..=published.len())
        .map(|rank| {
            sum += 1.0 / rank as f64;
            sum
        })
        .collect();
    let urls: Vec<String> = (0..args.rate * args.seconds)
        .map(|_| {
            let doc = &published[popular(&mut rng, &weights)];
            let page = &doc.pages[rng.below(doc.pages.len() as u64) as usize];
            format!("{}{page}", doc.link)
        })
        .collect();
    let gap = Duration::from_secs_f64(1.0 / args.rate as f64);
    let start = tokio::time::Instant::now();
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut tasks = Vec::new();
    for (i, url) in urls.into_iter().enumerate() {
        let scheduled = start + gap * i as u32;
        tokio::time::sleep_until(scheduled).await;
        let client = client.clone();
        let results = results.clone();
        tasks.push(tokio::spawn(async move {
            let ok = match client.get(url).send().await {
                Ok(answer) => answer.status().is_success() && answer.bytes().await.is_ok(),
                Err(_) => false,
            };
            results.lock().unwrap().push((scheduled.elapsed(), ok));
        }));
    }
    for task in tasks {
        let _ = task.await;
    }
    let wall = start.elapsed();
    let results = std::mem::take(&mut *results.lock().unwrap());
    report::Latencies::of(results, wall)
}
