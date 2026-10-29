//! The report: every number of every scenario, as the median of its runs and
//! their spread, beside its target and a baseline's; and from `/metrics`, the
//! stages that took the time. The numbers are also written as JSON, which
//! the next run reads as its baseline.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq)]
pub enum Unit {
    Ms,
    PerSecond,
    Bytes,
    Count,
    Ratio,
}

impl Unit {
    fn name(self) -> &'static str {
        match self {
            Unit::Ms => "ms",
            Unit::PerSecond => "/s",
            Unit::Bytes => "bytes",
            Unit::Count => "count",
            Unit::Ratio => "ratio",
        }
    }

    fn show(self, value: f64) -> String {
        match self {
            Unit::Ms => format!("{value:.1} ms"),
            Unit::PerSecond => format!("{value:.0}/s"),
            Unit::Bytes => format!("{:.0} MB", value / 1e6),
            Unit::Count => format!("{value:.0}"),
            Unit::Ratio => format!("{:.2}%", value * 100.0),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Target {
    AtMost(f64),
    AtLeast(f64),
}

/// One number of one scenario, measured once per run.
pub struct Metric {
    pub unit: Unit,
    pub target: Option<Target>,
    pub runs: Vec<f64>,
}

impl Metric {
    pub fn median(&self) -> Option<f64> {
        let mut runs = self.runs.clone();
        runs.sort_by(f64::total_cmp);
        runs.get(runs.len() / 2).copied()
    }

    /// How far apart its runs came: the noise a change has to beat.
    pub fn spread(&self) -> f64 {
        let max = self.runs.iter().copied().fold(f64::MIN, f64::max);
        let min = self.runs.iter().copied().fold(f64::MAX, f64::min);
        if self.runs.is_empty() { 0.0 } else { max - min }
    }

    /// Met, missed — only by more than its spread — or at the edge.
    pub fn verdict(&self) -> &'static str {
        let (Some(target), Some(median)) = (self.target, self.median()) else {
            return if self.runs.is_empty() {
                "not reported"
            } else {
                ""
            };
        };
        let spread = self.spread();
        match target {
            Target::AtMost(t) if median <= t => "met",
            Target::AtMost(t) if median - spread > t => "missed",
            Target::AtLeast(t) if median >= t => "met",
            Target::AtLeast(t) if median + spread < t => "missed",
            _ => "at the edge",
        }
    }
}

/// Everything a run measured.
#[derive(Default)]
pub struct Results {
    pub metrics: BTreeMap<String, Metric>,
    /// Each scenario's timings from `/metrics`, summed over its runs:
    /// (times, seconds).
    pub timings: BTreeMap<String, BTreeMap<String, (f64, f64)>>,
    /// The server's totals at the end of the run.
    pub totals: BTreeMap<String, f64>,
}

impl Results {
    /// Files `value` as one run of `key` — `<scenario>.<number>`.
    pub fn record(&mut self, key: &str, unit: Unit, target: Option<Target>, value: f64) {
        self.metrics
            .entry(key.to_string())
            .or_insert_with(|| Metric {
                unit,
                target,
                runs: Vec::new(),
            })
            .runs
            .push(value);
    }

    /// A number the server didn't report: its row says so.
    pub fn missing(&mut self, key: &str, unit: Unit, target: Option<Target>) {
        self.metrics
            .entry(key.to_string())
            .or_insert_with(|| Metric {
                unit,
                target,
                runs: Vec::new(),
            });
    }

    pub fn timed(&mut self, scenario: &str, between: BTreeMap<String, (f64, f64)>) {
        let into = self.timings.entry(scenario.to_string()).or_default();
        for (key, (times, seconds)) in between {
            let entry = into.entry(key).or_default();
            entry.0 += times;
            entry.1 += seconds;
        }
    }

    pub fn to_json(&self, about: Value) -> Value {
        let metrics: serde_json::Map<String, Value> = self
            .metrics
            .iter()
            .map(|(key, m)| {
                let target = match m.target {
                    Some(Target::AtMost(t)) => json!({ "at_most": t }),
                    Some(Target::AtLeast(t)) => json!({ "at_least": t }),
                    None => Value::Null,
                };
                (
                    key.clone(),
                    json!({
                        "unit": m.unit.name(),
                        "target": target,
                        "runs": m.runs,
                        "median": m.median(),
                        "spread": m.spread(),
                        "verdict": m.verdict(),
                    }),
                )
            })
            .collect();
        json!({
            "about": about,
            "metrics": metrics,
            "timings": self.timings,
            "totals": self.totals,
        })
    }
}

/// A baseline's numbers: each metric's median and spread.
pub type Baseline = BTreeMap<String, (f64, f64)>;

pub fn baseline(json: &Value) -> Baseline {
    json["metrics"]
        .as_object()
        .map(|metrics| {
            metrics
                .iter()
                .filter_map(|(key, m)| {
                    Some((key.clone(), (m["median"].as_f64()?, m["spread"].as_f64()?)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// How a number moved from the baseline's: only by more than the noise of
/// either run counts.
fn moved(metric: &Metric, base: Option<&(f64, f64)>) -> String {
    let (Some(now), Some((then, then_spread))) = (metric.median(), base) else {
        return "—".to_string();
    };
    let noise = metric.spread().max(*then_spread);
    if (now - then).abs() <= noise || *then == 0.0 {
        return "same".to_string();
    }
    format!("{:+.0}%", (now - then) / then * 100.0)
}

fn target(metric: &Metric) -> String {
    match metric.target {
        Some(Target::AtMost(t)) => format!("≤ {}", metric.unit.show(t)),
        Some(Target::AtLeast(t)) => format!("≥ {}", metric.unit.show(t)),
        None => "—".to_string(),
    }
}

pub fn markdown(results: &Results, about: &str, base: Option<&Baseline>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Server benchmark\n\n{about}\n");
    let missed: Vec<&String> = results
        .metrics
        .iter()
        .filter(|(_, m)| m.verdict() == "missed")
        .map(|(key, _)| key)
        .collect();
    let _ = writeln!(
        out,
        "**{}**\n",
        if missed.is_empty() {
            "Every target met.".to_string()
        } else {
            format!(
                "{} missed: {}.",
                missed.len(),
                missed
                    .iter()
                    .map(|k| format!("`{k}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    let mut scenario = "";
    for (key, metric) in &results.metrics {
        let (this, number) = key.split_once('.').unwrap_or((key, ""));
        if this != scenario {
            if !scenario.is_empty() {
                stages(&mut out, results, scenario);
            }
            scenario = this;
            let _ = writeln!(out, "### {this}\n");
            let _ = writeln!(
                out,
                "| | median | spread | runs | target | | main | change |"
            );
            let _ = writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- |");
        }
        let main = base
            .and_then(|b| b.get(key))
            .map(|(m, _)| metric.unit.show(*m))
            .unwrap_or_else(|| "—".to_string());
        let _ = writeln!(
            out,
            "| {number} | {} | {} | {} | {} | {} | {main} | {} |",
            metric
                .median()
                .map(|m| metric.unit.show(m))
                .unwrap_or_else(|| "—".to_string()),
            metric.unit.show(metric.spread()),
            metric.runs.len(),
            target(metric),
            metric.verdict(),
            moved(metric, base.and_then(|b| b.get(key))),
        );
    }
    if !scenario.is_empty() {
        stages(&mut out, results, scenario);
    }
    if !results.totals.is_empty() {
        let _ = writeln!(out, "### The server's totals at the end\n");
        let _ = writeln!(out, "| | |\n| --- | --- |");
        for (name, value) in &results.totals {
            let _ = writeln!(out, "| `{name}` | {value} |");
        }
    }
    out
}

/// A scenario's stages, the most time first, then its other timings —
/// requests by route and status.
fn stages(out: &mut String, results: &Results, scenario: &str) {
    let Some(timings) = results.timings.get(scenario) else {
        let _ = writeln!(out, "\nNo stage timings reported.\n");
        return;
    };
    let mut rows: Vec<(&String, &(f64, f64))> = timings.iter().collect();
    rows.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    let (stages, others): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|(key, _)| crate::metrics::is_stage(key));
    for (title, rows) in [("stage", stages), ("other timing", others)] {
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n| {title} | times | mean | total |");
        let _ = writeln!(out, "| --- | --- | --- | --- |");
        for (key, (times, seconds)) in rows.iter().take(12) {
            let _ = writeln!(
                out,
                "| `{key}` | {times:.0} | {:.2} ms | {seconds:.2} s |",
                seconds / times.max(1.0) * 1000.0
            );
        }
    }
    let _ = writeln!(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(target: Option<Target>, runs: &[f64]) -> Metric {
        Metric {
            unit: Unit::Ms,
            target,
            runs: runs.to_vec(),
        }
    }

    #[test]
    fn a_target_is_missed_only_by_more_than_the_spread() {
        assert_eq!(
            metric(Some(Target::AtMost(100.0)), &[80.0, 90.0, 95.0]).verdict(),
            "met"
        );
        assert_eq!(
            metric(Some(Target::AtMost(100.0)), &[90.0, 105.0, 120.0]).verdict(),
            "at the edge"
        );
        assert_eq!(
            metric(Some(Target::AtMost(100.0)), &[300.0, 310.0, 320.0]).verdict(),
            "missed"
        );
        assert_eq!(
            metric(Some(Target::AtLeast(500.0)), &[400.0]).verdict(),
            "missed"
        );
        assert_eq!(
            metric(Some(Target::AtMost(1.0)), &[]).verdict(),
            "not reported"
        );
    }

    #[test]
    fn a_number_moved_only_beyond_the_noise() {
        let now = metric(None, &[100.0, 104.0, 110.0]);
        assert_eq!(moved(&now, Some(&(102.0, 6.0))), "same");
        assert_eq!(moved(&now, Some(&(50.0, 2.0))), "+108%");
        assert_eq!(moved(&now, None), "—");
    }

    #[test]
    fn a_runs_json_is_the_next_runs_baseline() {
        let mut results = Results::default();
        results.record(
            "views.page p99",
            Unit::Ms,
            Some(Target::AtMost(100.0)),
            40.0,
        );
        results.record(
            "views.page p99",
            Unit::Ms,
            Some(Target::AtMost(100.0)),
            60.0,
        );
        let json = results.to_json(json!({ "seed": 1 }));
        let base = baseline(&json);
        assert_eq!(base["views.page p99"], (60.0, 20.0));
        let text = markdown(&results, "a run", Some(&base));
        assert!(
            text.contains(
                "| page p99 | 60.0 ms | 20.0 ms | 2 | ≤ 100.0 ms | met | 60.0 ms | same |"
            ),
            "{text}"
        );
    }
}
