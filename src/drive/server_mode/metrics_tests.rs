use axum::Router;
use axum::http::StatusCode;

use super::routers;
use super::tests::{ask, config, create, get, zip_of};

/// A site of two pages, one linking to a heading on the other.
fn linked_site(title: &str, page: &str) -> Vec<u8> {
    zip_of(&[
        (
            "devpage.yml",
            &format!("kind: site\ntitle: {title}\nroot: README.md\n"),
        ),
        (
            "README.md",
            &format!("# {title}\n\nSee [the other](./{page}.md#other).\n"),
        ),
        (&format!("{page}.md"), "# Other\n\n## Other\n\nText.\n"),
    ])
}

async fn scrape(metrics: &Router) -> String {
    let (status, text) = ask(metrics, get("/metrics")).await;
    assert_eq!(status, StatusCode::OK);
    text.as_str().unwrap().to_string()
}

/// The value of the series `name{labels}`, the first whose labels hold every
/// one of `labels`.
fn value(text: &str, name: &str, labels: &[&str]) -> Option<f64> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .find(|line| {
            let (series, _) = line.split_once(' ').unwrap_or((line, ""));
            let bare = series.split('{').next().unwrap_or(series);
            bare == name && labels.iter().all(|label| series.contains(label))
        })
        .and_then(|line| line.rsplit(' ').next()?.parse().ok())
}

#[tokio::test]
async fn the_metrics_port_reports_every_stage_the_requests_and_the_totals() {
    let (app, metrics) = routers(&config("metrics-all")).unwrap();
    let id = create(&app, linked_site("Measured", "other")).await;
    let (status, _) = ask(&app, get(&format!("/d/{id}/README.md"))).await;
    assert_eq!(status, StatusCode::OK);
    let text = scrape(&metrics).await;
    for stage in [
        "store",
        "fetch",
        "unpack",
        "router",
        "check",
        "nav",
        "render",
        "publish-check",
        "upload",
        "download",
    ] {
        let label = format!("stage=\"{stage}\"");
        assert!(
            value(&text, "devpage_stage_seconds_count", &[&label]).is_some(),
            "{stage} is reported"
        );
    }
    // What the publish and the view did, counted against their stages.
    for stage in [
        "store",
        "fetch",
        "unpack",
        "router",
        "check",
        "nav",
        "render",
        "publish-check",
        "upload",
    ] {
        let label = format!("stage=\"{stage}\"");
        let count = value(&text, "devpage_stage_seconds_count", &[&label]).unwrap();
        assert!(count >= 1.0, "{stage} counted the publish or the view");
    }
    assert!(
        value(
            &text,
            "devpage_request_seconds_count",
            &["route=\"/api/documents\"", "status=\"201\""]
        )
        .is_some_and(|n| n >= 1.0),
        "the publish is a request"
    );
    assert!(
        value(
            &text,
            "devpage_request_seconds_count",
            &["route=\"/d/{id}/{*rest}\"", "status=\"200\""]
        )
        .is_some_and(|n| n >= 1.0),
        "the view is a request"
    );
    for total in [
        "devpage_versions_in_memory",
        "devpage_routers",
        "devpage_threads",
        "devpage_open_pages",
        "devpage_cache_bytes",
        "devpage_cache_hits_total",
        "devpage_cache_misses_total",
        "devpage_cache_evictions_total",
        "devpage_resident_bytes",
    ] {
        assert!(value(&text, total, &[]).is_some(), "{total} is reported");
    }
    assert_eq!(value(&text, "devpage_versions_in_memory", &[]), Some(1.0));
    assert!(value(&text, "devpage_cache_bytes", &[]).unwrap() > 0.0);
    assert!(value(&text, "devpage_threads", &[]).unwrap() >= 1.0);
    assert!(value(&text, "devpage_resident_bytes", &[]).unwrap() > 0.0);
}

#[tokio::test]
async fn the_pages_port_has_no_metrics() {
    let (app, _) = routers(&config("metrics-pages-port")).unwrap();
    let (status, _) = ask(&app, get("/metrics")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn no_label_names_a_document_a_version_or_a_page() {
    let (app, metrics) = routers(&config("metrics-labels")).unwrap();
    let mut chosen = Vec::new();
    for (title, page) in [("Alpha", "alpha-secret"), ("Beta", "beta-secret")] {
        let id = create(&app, linked_site(title, page)).await;
        for path in [
            format!("/d/{id}/README.md"),
            format!("/d/{id}/v/1/README.md"),
            format!("/d/{id}/{page}.md"),
            format!("/d/{id}/__devpage/versions"),
        ] {
            ask(&app, get(&path)).await;
        }
        chosen.extend([id, page.to_string(), title.to_string()]);
    }
    let text = scrape(&metrics).await;
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let Some(labels) = line
            .split_once('{')
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(labels, _)| labels)
        else {
            continue;
        };
        for label in labels.split("\",") {
            let (key, value) = label.split_once("=\"").unwrap();
            assert!(
                ["stage", "le", "route", "status"].contains(&key),
                "{key} in {line}"
            );
            for word in &chosen {
                assert!(!value.contains(word.as_str()), "{word} in {line}");
            }
            assert!(!value.contains("/v/1"), "a version in {line}");
        }
    }
}
