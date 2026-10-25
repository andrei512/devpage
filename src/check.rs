//! Link checking: every link, image and diagram target a page holds must
//! reach a file in the repository — and a heading on it, when it names one.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::document::{Document, Kind};
use crate::render;

/// One broken thing, where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Problem {
    /// The file it is in, from the top of the repository.
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// The pages a document is made of.
fn pages(document: &Document) -> Vec<PathBuf> {
    match &document.kind {
        Kind::Site { root } => vec![document.base.join(root)],
        Kind::Doc { chapters, .. } => chapters.clone(),
        Kind::Slides { slides } => slides.clone(),
    }
}

/// Checks the document and every page and document reachable from it by
/// links inside the repository.
pub async fn document(document: &Document, check_external: bool) -> Vec<Problem> {
    let mut external = Vec::new();
    let mut problems = internal(document, &mut external);
    if check_external {
        problems.extend(external_links(external).await);
    }
    problems
}

/// External addresses must answer below 400, following redirects, in 10 s.
async fn external_links(links: Vec<(String, Option<usize>, String)>) -> Vec<Problem> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("devpage/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("an HTTP client");
    let checks = links.into_iter().map(|(file, line, address)| {
        let client = client.clone();
        async move {
            let message = match client.get(&address).send().await {
                Ok(response) if response.status().as_u16() < 400 => return None,
                Ok(response) => format!("{address} — {}", response.status()),
                Err(error) => format!("{address} — {error}"),
            };
            Some(Problem {
                file,
                line,
                message,
            })
        }
    });
    futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect()
}

fn internal(
    document: &Document,
    external: &mut Vec<(String, Option<usize>, String)>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<PathBuf> = pages(document).into();
    while let Some(page) = queue.pop_front() {
        if !seen.insert(page.clone()) {
            continue;
        }
        let (found, reached) = page_links_with(document, &page, external);
        problems.extend(found);
        for target in reached {
            if target.extension().is_some_and(|ext| ext == "md") {
                queue.push_back(target);
            } else if target
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
            {
                match Document::load(&target) {
                    Ok(linked) => queue.extend(pages(&linked)),
                    Err(error) => problems.push(Problem {
                        file: document.url(&target)[1..].to_string(),
                        line: None,
                        message: format!("{error:#}"),
                    }),
                }
            }
        }
    }
    problems
}

/// One page's problems, and the files its working links reach.
pub fn page_links(document: &Document, page: &Path) -> (Vec<Problem>, Vec<PathBuf>) {
    let mut external = Vec::new();
    page_links_with(document, page, &mut external)
}

/// One page's problems and reached files; its external links are collected
/// in `external` as (file, line, address).
fn page_links_with(
    document: &Document,
    page: &Path,
    external: &mut Vec<(String, Option<usize>, String)>,
) -> (Vec<Problem>, Vec<PathBuf>) {
    let source = std::fs::read_to_string(page).unwrap_or_default();
    let html = render::markdown(&source);
    let file = document.url(page)[1..].to_string();
    let mut problems = Vec::new();
    let mut reached = Vec::new();
    let mut targets = attributes(&html, "href");
    targets.extend(attributes(&html, "src"));
    // A diagram's `click … href "…"` lines, from real Mermaid blocks only.
    for block in html.split("<pre lang=\"mermaid\"><code>").skip(1) {
        let block = block
            .split("</code>")
            .next()
            .unwrap_or_default()
            .replace("&quot;", "\"");
        targets.extend(block.lines().filter_map(|line| {
            let rest = line.trim().strip_prefix("click ")?;
            let href = rest.split("href \"").nth(1)?.split('"').next()?;
            Some(href.to_string())
        }));
    }
    for target in targets {
        let line = source
            .find(&target)
            .map(|at| source[..at].lines().count().max(1));
        if target.starts_with("http://") || target.starts_with("https://") {
            external.push((file.clone(), line, target));
            continue;
        }
        if target.contains(':') {
            continue; // mailto: and other schemes are not checked
        }
        let (path, fragment) = target.split_once('#').unwrap_or((&target, ""));
        let resolved = if path.is_empty() {
            Some(page.to_path_buf())
        } else {
            page.parent()
                .unwrap_or(Path::new("."))
                .join(path)
                .canonicalize()
                .ok()
                .filter(|resolved| resolved.starts_with(&document.repo))
        };
        let Some(resolved) = resolved else {
            problems.push(Problem {
                file: file.clone(),
                line,
                message: format!("broken link {target} — no such file in the repository"),
            });
            continue;
        };
        if !fragment.is_empty()
            && resolved.extension().is_some_and(|ext| ext == "md")
            && fragment.parse::<usize>().is_err()
        {
            let target_html =
                render::markdown(&std::fs::read_to_string(&resolved).unwrap_or_default());
            if !target_html.contains(&format!("id=\"{fragment}\"")) {
                problems.push(Problem {
                    file: file.clone(),
                    line,
                    message: format!("broken link {target} — no heading #{fragment}"),
                });
            }
        }
        reached.push(resolved);
    }
    (problems, reached)
}

/// The problems on the given pages, for the preview: each is printed on the
/// terminal the first time it is found, and returned for the page to list.
pub fn preview(document: &Document, pages: &[PathBuf]) -> Vec<Problem> {
    static REPORTED: Mutex<Option<HashSet<Problem>>> = Mutex::new(None);
    let problems: Vec<Problem> = pages
        .iter()
        .flat_map(|page| page_links(document, page).0)
        .collect();
    let mut reported = REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let reported = reported.get_or_insert_with(HashSet::new);
    for problem in &problems {
        if reported.insert(problem.clone()) {
            eprintln!("{problem}");
        }
    }
    problems
}

/// The values of every `name="…"` attribute in rendered HTML.
fn attributes(html: &str, name: &str) -> Vec<String> {
    let pattern = format!(" {name}=\"");
    html.split(&pattern)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|value| value.replace("&amp;", "&"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_missing_files_headings_and_climbs_out_of_the_repository() {
        let dir = std::env::temp_dir().join(format!("devpage-check-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Home\n\n[ok](b.md#two) [gone](nope.md)\n\n[bad](b.md#three) [out](../../x.md)\n\n```mermaid\nflowchart LR\n  a\n  click a href \"missing.md\"\n```\n",
        )
        .unwrap();
        std::fs::write(dir.join("b.md"), "# B\n\n## Two\n\n![logo](logo.png)\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        let doc = Document::load(&dir).unwrap();
        let found: Vec<String> = internal(&doc, &mut Vec::new())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 5, "{found:#?}");
        assert!(
            found.contains(
                &"README.md:3: broken link nope.md — no such file in the repository".to_string()
            ),
            "{found:#?}"
        );
        assert!(found.iter().any(|p| p.contains("b.md#three — no heading")));
        assert!(found.iter().any(|p| p.contains("../../x.md")));
        assert!(found.iter().any(|p| p.contains("missing.md")));
        assert!(
            found
                .iter()
                .any(|p| p.starts_with("b.md:5: broken link logo.png")),
            "{found:#?}"
        );
    }
}
