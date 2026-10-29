//! devpage-bench: publishes a seeded corpus to a server, loads it the ways
//! the envelope says — views and bursts, misses, writes, a herd, open pages,
//! the knee, a soak — and reports each number beside its target and a
//! baseline's, with the time the server says each stage took.
//!
//!     devpage-bench --server http://127.0.0.1:3031 --metrics http://127.0.0.1:9031/metrics \
//!       [--seed 1] [--scale 1] [--runs 3] [--scenarios views,herd] [--docs docs] \
//!       [--memory-limit 4294967296] [--out results.json] [--report report.md] \
//!       [--baseline main.json]

mod corpus;
mod load;
mod metrics;
mod report;
mod scenarios;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;

struct Args {
    server: String,
    metrics: String,
    seed: u64,
    scale: f64,
    runs: usize,
    scenarios: Vec<String>,
    docs: Option<PathBuf>,
    memory_limit: f64,
    out: Option<PathBuf>,
    report: Option<PathBuf>,
    baseline: Option<PathBuf>,
}

fn args() -> Result<Args> {
    let mut given: HashMap<String, String> = HashMap::new();
    let mut words = std::env::args().skip(1);
    while let Some(flag) = words.next() {
        let name = flag.strip_prefix("--").context("flags are --name value")?;
        given.insert(
            name.to_string(),
            words
                .next()
                .with_context(|| format!("--{name} needs a value"))?,
        );
    }
    let number = |name: &str, default: f64| -> Result<f64> {
        given
            .get(name)
            .map(|v| {
                v.parse()
                    .with_context(|| format!("--{name} is not a number"))
            })
            .unwrap_or(Ok(default))
    };
    let scale = number("scale", 1.0)?;
    anyhow::ensure!(scale > 0.0, "--scale is above 0");
    let scenarios: Vec<String> = match given.get("scenarios") {
        Some(list) => list.split(',').map(str::to_string).collect(),
        None => scenarios::ALL.iter().map(|s| s.to_string()).collect(),
    };
    for scenario in &scenarios {
        anyhow::ensure!(
            scenarios::ALL.contains(&scenario.as_str()),
            "no scenario {scenario}: the scenarios are {}",
            scenarios::ALL.join(", ")
        );
    }
    let path = |name: &str| given.get(name).map(PathBuf::from);
    Ok(Args {
        server: given
            .get("server")
            .cloned()
            .unwrap_or("http://127.0.0.1:3030".into()),
        metrics: given
            .get("metrics")
            .cloned()
            .unwrap_or("http://127.0.0.1:9030/metrics".into()),
        seed: number("seed", 1.0)? as u64,
        scale,
        runs: (number("runs", 3.0)? as usize).max(1),
        scenarios,
        docs: path("docs"),
        memory_limit: number("memory-limit", 4.0 * 1024.0 * 1024.0 * 1024.0)?,
        out: path("out"),
        report: path("report"),
        baseline: path("baseline"),
    })
}

/// Open files enough for the pages held open: as many as the system allows.
#[cfg(unix)]
fn raise_open_files() {
    // SAFETY: getrlimit and setrlimit only read and write the struct given.
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            return;
        }
        for want in [limit.rlim_max.min(65_536), 10_240] {
            limit.rlim_cur = want.max(limit.rlim_cur);
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) == 0 {
                return;
            }
        }
    }
}

#[cfg(not(unix))]
fn raise_open_files() {}

#[tokio::main]
async fn main() -> Result<()> {
    let args = args()?;
    raise_open_files();
    let baseline = match &args.baseline {
        Some(path) if path.exists() => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let json: serde_json::Value = serde_json::from_str(&text)?;
            // Numbers at another scale or seed measure another corpus.
            let same = json["about"]["scale"].as_f64() == Some(args.scale)
                && json["about"]["seed"].as_u64() == Some(args.seed);
            if !same {
                eprintln!("{}: another scale or seed, so not compared", path.display());
            }
            same.then(|| report::baseline(&json))
        }
        _ => None,
    };
    let folder = args.docs.as_deref().map(corpus::folder).transpose()?;
    let mut bench = scenarios::Bench::new(
        args.server.clone(),
        args.metrics.clone(),
        args.seed,
        args.scale,
        args.memory_limit,
    )?;
    let mut results = report::Results::default();
    eprintln!("seeding: the corpus, scale {}", args.scale);
    bench.seed(&mut results, folder).await?;
    let bench = Arc::new(bench);
    for run in 1..=args.runs {
        for scenario in args.scenarios.iter().filter(|s| *s != "soak") {
            eprintln!("run {run}/{}: {scenario}", args.runs);
            bench.run(scenario, &mut results).await?;
        }
    }
    if args.scenarios.iter().any(|s| s == "soak") {
        eprintln!("soak, once");
        bench.run("soak", &mut results).await?;
    }
    let about = format!(
        "{} at seed {}, scale {}: {} of {}, the median of each; the soak once.",
        args.server,
        args.seed,
        args.scale,
        match args.runs {
            1 => "1 run".to_string(),
            n => format!("{n} runs"),
        },
        args.scenarios.join(", ")
    );
    let markdown = report::markdown(&results, &about, baseline.as_ref());
    println!("{markdown}");
    if let Some(path) = &args.report {
        std::fs::write(path, &markdown)?;
    }
    if let Some(path) = &args.out {
        let about = json!({
            "server": args.server,
            "seed": args.seed,
            "scale": args.scale,
            "runs": args.runs,
            "scenarios": args.scenarios,
        });
        std::fs::write(path, serde_json::to_string_pretty(&results.to_json(about))?)?;
    }
    Ok(())
}
