//! The server's metrics: how long each stage of its work took, as
//! histograms, served in the Prometheus text format on the metrics port.
//! Every label is a stage from the closed set below — never a document, a
//! version or a path asked for.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

/// The stages a request can spend its time in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for the store's lock, and the query.
    Store,
    /// A version's router made: its document loaded, its index built.
    Router,
    /// The page's problems: its links, and the pages they reach.
    Check,
    /// The site's navigation, around the page.
    Nav,
    /// The page in its frame — its navigation among it.
    Render,
}

const STAGES: [Stage; 5] = [
    Stage::Store,
    Stage::Router,
    Stage::Check,
    Stage::Nav,
    Stage::Render,
];

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Store => "store",
            Stage::Router => "router",
            Stage::Check => "check",
            Stage::Nav => "nav",
            Stage::Render => "render",
        }
    }

    /// The stage a core span of this name times, if it times one.
    fn of_span(name: &str) -> Option<Stage> {
        STAGES
            .into_iter()
            .find(|stage| stage.name() == name && *stage != Stage::Store)
    }
}

/// Upper bounds of the buckets, in seconds.
const BOUNDS: [f64; 14] = [
    0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
struct Histogram {
    buckets: [AtomicU64; BOUNDS.len()],
    count: AtomicU64,
    micros: AtomicU64,
}

impl Histogram {
    fn observe(&self, took: Duration) {
        let seconds = took.as_secs_f64();
        if let Some(at) = BOUNDS.iter().position(|bound| seconds <= *bound) {
            self.buckets[at].fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.micros.fetch_add(
            u64::try_from(took.as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
}

static HISTOGRAMS: LazyLock<[Histogram; STAGES.len()]> = LazyLock::new(Default::default);

fn histogram(stage: Stage) -> &'static Histogram {
    let at = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
    &HISTOGRAMS[at]
}

/// Counts `took` against `stage`.
pub fn observe(stage: Stage, took: Duration) {
    histogram(stage).observe(took);
}

/// Times `work` as `stage`.
pub fn time<T>(stage: Stage, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let done = work();
    observe(stage, started.elapsed());
    done
}

/// Every histogram, in the Prometheus text format.
pub fn text() -> String {
    let mut out = String::from(
        "# HELP devpage_stage_seconds Time spent in each stage of the server's work.\n\
         # TYPE devpage_stage_seconds histogram\n",
    );
    for stage in STAGES {
        let histogram = histogram(stage);
        let name = stage.name();
        let mut cumulative = 0;
        for (bound, bucket) in BOUNDS.iter().zip(&histogram.buckets) {
            cumulative += bucket.load(Ordering::Relaxed);
            let _ = writeln!(
                out,
                "devpage_stage_seconds_bucket{{stage=\"{name}\",le=\"{bound}\"}} {cumulative}"
            );
        }
        let count = histogram.count.load(Ordering::Relaxed);
        let seconds = histogram.micros.load(Ordering::Relaxed) as f64 / 1e6;
        let _ = writeln!(
            out,
            "devpage_stage_seconds_bucket{{stage=\"{name}\",le=\"+Inf\"}} {count}"
        );
        let _ = writeln!(
            out,
            "devpage_stage_seconds_sum{{stage=\"{name}\"}} {seconds}"
        );
        let _ = writeln!(
            out,
            "devpage_stage_seconds_count{{stage=\"{name}\"}} {count}"
        );
    }
    out
}

/// The listener that turns the core's stage spans into histograms: a span is
/// timed from when it is made to when it closes. Every other span and every
/// event is ignored.
#[derive(Default)]
pub struct Stages {
    next: AtomicU64,
    open: Mutex<HashMap<u64, (Stage, Instant)>>,
}

impl Subscriber for Stages {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.enabled(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_span() && Stage::of_span(metadata.name()).is_some()
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(stage) = Stage::of_span(span.metadata().name()) {
            self.open
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id, (stage, Instant::now()));
        }
        Id::from_u64(id)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, _: &Event<'_>) {}

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}

    fn try_close(&self, id: Id) -> bool {
        let closed = self
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id.into_u64());
        if let Some((stage, started)) = closed {
            observe(stage, started.elapsed());
        }
        true
    }
}

/// The metrics port's router: `/metrics`, and nothing else.
pub fn router() -> axum::Router {
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(|| async {
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4",
                )],
                text(),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_core_span_is_counted_against_its_stage() {
        let before = histogram(Stage::Nav).count.load(Ordering::Relaxed);
        tracing::subscriber::with_default(Stages::default(), || {
            let _nav = tracing::info_span!("nav").entered();
            let _other = tracing::info_span!("anything else").entered();
        });
        assert_eq!(
            histogram(Stage::Nav).count.load(Ordering::Relaxed),
            before + 1
        );
    }

    #[test]
    fn the_text_names_only_stages() {
        observe(Stage::Check, Duration::from_millis(3));
        let text = text();
        assert!(text.contains("devpage_stage_seconds_bucket{stage=\"check\",le=\"0.005\"}"));
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let labels = &line[line.find('{').unwrap()..line.find('}').unwrap()];
            let stage = labels.split('"').nth(1).unwrap();
            assert!(STAGES.iter().any(|s| s.name() == stage), "{line}");
        }
    }
}
