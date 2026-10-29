//! The report: each number beside its target, and from the server's
//! `/metrics` the time each stage took during the run.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::time::Duration;

use anyhow::Result;

/// Each stage's total seconds and count, as `/metrics` gives them.
pub type Scrape = BTreeMap<String, (f64, f64)>;

pub async fn scrape(client: &reqwest::Client, metrics: &str) -> Result<Scrape> {
    let text = client.get(metrics).send().await?.text().await?;
    Ok(parse(&text))
}

fn parse(text: &str) -> Scrape {
    let mut out = Scrape::new();
    for line in text.lines() {
        for (suffix, sum) in [("_sum{", true), ("_count{", false)] {
            let Some(rest) = line
                .strip_prefix("devpage_stage_seconds")
                .and_then(|r| r.strip_prefix(&suffix[..suffix.len() - 1]))
            else {
                continue;
            };
            let Some(stage) = rest.split('"').nth(1) else {
                continue;
            };
            let value: f64 = rest
                .rsplit(' ')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let entry = out.entry(stage.to_string()).or_default();
            if sum {
                entry.0 = value;
            } else {
                entry.1 = value;
            }
        }
    }
    out
}

/// What each stage took between two scrapes: (stage, times, mean, total).
pub fn stages(before: &Scrape, after: &Scrape) -> Vec<(String, f64, Duration, Duration)> {
    after
        .iter()
        .map(|(stage, (sum, count))| {
            let (sum0, count0) = before.get(stage).copied().unwrap_or_default();
            let times = count - count0;
            let total = Duration::from_secs_f64((sum - sum0).max(0.0));
            let mean = if times > 0.0 {
                total.div_f64(times)
            } else {
                Duration::ZERO
            };
            (stage.clone(), times, mean, total)
        })
        .collect()
}

pub struct Latencies {
    pub sorted: Vec<Duration>,
    pub errors: usize,
    pub wall: Duration,
}

impl Latencies {
    pub fn of(results: Vec<(Duration, bool)>, wall: Duration) -> Self {
        let errors = results.iter().filter(|(_, ok)| !ok).count();
        let mut sorted: Vec<Duration> = results.into_iter().map(|(took, _)| took).collect();
        sorted.sort();
        Latencies {
            sorted,
            errors,
            wall,
        }
    }

    pub fn at(&self, quantile: f64) -> Duration {
        if self.sorted.is_empty() {
            return Duration::ZERO;
        }
        let at =
            ((self.sorted.len() as f64 * quantile).ceil() as usize).clamp(1, self.sorted.len());
        self.sorted[at - 1]
    }
}

pub struct Run {
    pub documents: usize,
    pub seeding: Duration,
    pub publishes: Vec<Duration>,
    pub views: Latencies,
    pub rate: u64,
    pub seconds: u64,
    pub target: Duration,
    pub stages: Vec<(String, f64, Duration, Duration)>,
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

pub fn render(run: &Run) -> String {
    let mut out = String::new();
    let publishes = Latencies::of(
        run.publishes.iter().map(|d| (*d, true)).collect(),
        run.seeding,
    );
    let views = &run.views;
    let p99 = views.at(0.99);
    let _ = writeln!(out, "## Server benchmark\n");
    let _ = writeln!(
        out,
        "{} documents published in {:.1} s: {:.1} a second, p50 {}, p99 {}\n",
        run.documents,
        run.seeding.as_secs_f64(),
        run.documents as f64 / run.seeding.as_secs_f64(),
        ms(publishes.at(0.5)),
        ms(publishes.at(0.99)),
    );
    let _ = writeln!(
        out,
        "| views | asked | answered/s | p50 | p99 | max | errors | target p99 | |"
    );
    let _ = writeln!(
        out,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    let _ = writeln!(
        out,
        "| open, {} s | {}/s | {:.0} | {} | {} | {} | {} | {} | {} |\n",
        run.seconds,
        run.rate,
        views.sorted.len() as f64 / views.wall.as_secs_f64(),
        ms(views.at(0.5)),
        ms(p99),
        ms(views.at(1.0)),
        views.errors,
        ms(run.target),
        if p99 <= run.target && views.errors == 0 {
            "met"
        } else {
            "missed"
        },
    );
    let _ = writeln!(out, "| stage | times | mean | total |");
    let _ = writeln!(out, "| --- | --- | --- | --- |");
    for (stage, times, mean, total) in &run.stages {
        let _ = writeln!(
            out,
            "| {stage} | {times:.0} | {} | {:.2} s |",
            ms(*mean),
            total.as_secs_f64()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrape_reads_each_stages_sum_and_count() {
        let text = "devpage_stage_seconds_bucket{stage=\"check\",le=\"0.005\"} 2\n\
                    devpage_stage_seconds_sum{stage=\"check\"} 0.5\n\
                    devpage_stage_seconds_count{stage=\"check\"} 4\n";
        assert_eq!(parse(text)["check"], (0.5, 4.0));
    }

    #[test]
    fn a_quantile_is_the_slowest_of_its_share() {
        let l = Latencies::of(
            (1..=100)
                .map(|i| (Duration::from_millis(i), true))
                .collect(),
            Duration::from_secs(1),
        );
        assert_eq!(l.at(0.5), Duration::from_millis(50));
        assert_eq!(l.at(0.99), Duration::from_millis(99));
    }
}
