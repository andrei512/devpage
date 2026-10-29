//! What the server says about its own work, read from `/metrics`: each
//! timing's total seconds and count, and each total's value. Names are read
//! as the server gives them; a series the server doesn't serve is simply not
//! reported.

use std::collections::BTreeMap;

use anyhow::Result;

#[derive(Default, Clone)]
pub struct Scrape {
    /// Each histogram series — `name{labels}` — as (seconds, count).
    pub timings: BTreeMap<String, (f64, f64)>,
    /// Every other series: counters and gauges.
    pub totals: BTreeMap<String, f64>,
}

/// The stage histogram's family; its series are keyed by the stage alone.
const STAGES: &str = "devpage_stage_seconds";

pub async fn scrape(client: &reqwest::Client, url: &str) -> Result<Scrape> {
    let text = client.get(url).send().await?.text().await?;
    Ok(parse(&text))
}

pub fn parse(text: &str) -> Scrape {
    let mut out = Scrape::default();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let Some((series, value)) = line.rsplit_once(' ') else {
            continue;
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        let (name, labels) = match series.split_once('{') {
            Some((name, rest)) => (name, format!("{{{rest}")),
            None => (series, String::new()),
        };
        if name.ends_with("_bucket") {
            continue;
        }
        let timing = |family: &str| {
            if family == STAGES {
                labels.split('"').nth(1).unwrap_or("").to_string()
            } else {
                format!("{family}{labels}")
            }
        };
        if let Some(family) = name.strip_suffix("_sum") {
            out.timings.entry(timing(family)).or_default().0 = value;
        } else if let Some(family) = name.strip_suffix("_count") {
            out.timings.entry(timing(family)).or_default().1 = value;
        } else {
            out.totals.insert(format!("{name}{labels}"), value);
        }
    }
    out
}

/// Whether a timing is one of the stages, rather than a request series.
pub fn is_stage(key: &str) -> bool {
    !key.contains('{')
}

/// What each timing took between two scrapes, as (times, seconds).
pub fn between(before: &Scrape, after: &Scrape) -> BTreeMap<String, (f64, f64)> {
    after
        .timings
        .iter()
        .map(|(key, (seconds, count))| {
            let (seconds0, count0) = before.timings.get(key).copied().unwrap_or_default();
            (
                key.clone(),
                ((count - count0).max(0.0), (seconds - seconds0).max(0.0)),
            )
        })
        .filter(|(_, (times, _))| *times > 0.0)
        .collect()
}

/// The first total whose name holds `word`, as the server reports it.
pub fn total(scrape: &Scrape, word: &str) -> Option<f64> {
    scrape
        .totals
        .iter()
        .find(|(name, _)| name.contains(word))
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scrape_reads_stages_requests_and_totals() {
        let text = "# TYPE devpage_stage_seconds histogram\n\
                    devpage_stage_seconds_bucket{stage=\"check\",le=\"0.005\"} 2\n\
                    devpage_stage_seconds_sum{stage=\"check\"} 0.5\n\
                    devpage_stage_seconds_count{stage=\"check\"} 4\n\
                    devpage_request_seconds_sum{route=\"/d/{id}/{*rest}\",status=\"200\"} 1.5\n\
                    devpage_request_seconds_count{route=\"/d/{id}/{*rest}\",status=\"200\"} 3\n\
                    devpage_threads 18\n\
                    devpage_resident_memory_bytes 1048576\n";
        let scrape = parse(text);
        assert_eq!(scrape.timings["check"], (0.5, 4.0));
        assert!(is_stage("check"));
        let request = "devpage_request_seconds{route=\"/d/{id}/{*rest}\",status=\"200\"}";
        assert_eq!(scrape.timings[request], (1.5, 3.0));
        assert!(!is_stage(request));
        assert_eq!(total(&scrape, "threads"), Some(18.0));
        assert_eq!(total(&scrape, "resident"), Some(1048576.0));
        assert_eq!(total(&scrape, "nothing"), None);
    }

    #[test]
    fn between_two_scrapes_is_what_happened_in_between() {
        let before = parse(
            "devpage_stage_seconds_sum{stage=\"nav\"} 1\ndevpage_stage_seconds_count{stage=\"nav\"} 10\n",
        );
        let after = parse(
            "devpage_stage_seconds_sum{stage=\"nav\"} 3\ndevpage_stage_seconds_count{stage=\"nav\"} 30\n",
        );
        assert_eq!(between(&before, &after)["nav"], (20.0, 2.0));
        assert!(between(&after, &after).is_empty());
    }
}
