//! Link checking: every link, image and diagram target a page holds must
//! reach a file in the served folder — and a heading on it, when it names one —
//! every external address must answer, and every diagram must draw.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::document::{Document, Kind};
use crate::render;

use crate::external::{External, Link};
use crate::html;
use crate::problem::Problem;
use crate::widgets::Widgets;

/// The pages a document is made of.
fn pages(document: &Document) -> Vec<PathBuf> {
    match &document.kind {
        Kind::Site { root } => vec![document.base.join(root)],
        Kind::Doc { chapters, .. } => chapters.clone(),
        Kind::Slides { slides, .. } => slides.iter().map(|slide| slide.file.clone()).collect(),
        Kind::Design => Vec::new(),
    }
}

/// Checks the document and every page and document reachable from it by
/// links inside the served folder.
/// External links are checked when `checker` is given.
pub async fn document(document: &Document, checker: Option<&External>) -> Vec<Problem> {
    let mut external = Vec::new();
    let mut problems = internal(document, &mut external, None);
    if let Kind::Design = document.kind {
        let address = document.name(&document.path());
        problems.extend(
            crate::designs::problems(&document.served, &address)
                .into_iter()
                // A design names its place by line and column.
                .map(|problem| Problem {
                    file: format!("{}:{}:{}", problem.file, problem.pos.line, problem.pos.col),
                    line: None,
                    message: problem.message,
                    target: None,
                }),
        );
    }
    if let Some(checker) = checker {
        problems.extend(checker.check(external).await);
    }
    problems
}

/// Every file the check follows from `document`, each once and in path
/// order: the document file, its pages, chapters or slides and every page
/// and document they reach, the images and files their links name, a design's
/// imports, images, themes and fonts, the logo, and the project widgets with
/// every module they import — what the document is made of, and nothing else
/// beside it. External addresses are not followed.
pub fn files(document: &Document) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    internal(document, &mut Vec::new(), Some(&mut files));
    files.into_iter().collect()
}

/// A file the walk followed, kept when the caller asked for them: only
/// regular files inside the served folder.
fn follow(document: &Document, files: &mut Option<&mut BTreeSet<PathBuf>>, file: &Path) {
    let Some(files) = files.as_deref_mut() else {
        return;
    };
    if let Some(real) = document.served.file(file)
        && files.insert(real.clone())
        && real.extension().is_some_and(|ext| ext == "design")
    {
        let address = document.name(&real);
        files.extend(crate::designs::files(&document.served, &address));
    }
}

fn internal(
    document: &Document,
    external: &mut Vec<Link>,
    mut files: Option<&mut BTreeSet<PathBuf>>,
) -> Vec<Problem> {
    follow(document, &mut files, &document.path());
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut checked = Vec::new();
    // Every page of a deck the check reaches is a slide, however else it is
    // reached: its notes are judged once the whole walk is done.
    let mut slides = HashSet::new();
    let deck_pages = |linked: &Document, slides: &mut HashSet<PathBuf>| {
        let found = pages(linked);
        if matches!(linked.kind, Kind::Slides { .. }) {
            slides.extend(found.iter().cloned());
        }
        found
    };
    let mut queue: VecDeque<PathBuf> = deck_pages(document, &mut slides).into();
    // A page is checked as its nearest site's page, whichever document the
    // check started from — as the preview shows it.
    let index = crate::index::Index::build(&document.served);
    let mut widget_folders = HashSet::new();
    let mut navbars = HashSet::new();
    let mut logos = HashSet::new();
    while let Some(page) = queue.pop_front() {
        if !seen.insert(page.clone()) {
            continue;
        }
        checked.push(page.clone());
        follow(document, &mut files, &page);
        let owner = match index.site_for(&page) {
            Some(site) => Document::load(&site, &document.served).ok(),
            // In no site, a page is shown as a doc of one chapter — unless it
            // is one of the doc's or deck's own files the check started from.
            None if matches!(document.kind, Kind::Site { .. }) => {
                Document::load(&page, &document.served).ok()
            }
            None => None,
        };
        let owner = owner.as_ref().unwrap_or(document);
        follow(document, &mut files, &owner.path());
        let widgets = Widgets::of(owner, index.modules());
        if let Some(folder) = &owner.widgets
            && widget_folders.insert(folder.clone())
        {
            problems.extend(widget_problems(owner, &widgets));
            if files.is_some() {
                for widget in widget_files(folder) {
                    let snapshot = index.modules().snapshot(&document.served, &widget);
                    for (module, _) in snapshot.closure(&widget) {
                        follow(document, &mut files, &module);
                    }
                }
            }
        }
        if let Some(logo) = &owner.logo {
            follow(document, &mut files, &owner.base.join(logo));
        }
        // A document's logo is checked once.
        if logos.insert(owner.path())
            && let Some(problem) = crate::navbar::logo_problem(owner)
        {
            problems.push(problem);
        }
        // A site's navbar is checked once, and what it links to is checked too.
        if !owner.navbar.is_empty() && navbars.insert(owner.path()) {
            problems.extend(crate::navbar::problems(owner));
            let (reached_files, web) = crate::navbar::reached(owner);
            let file = owner.name(&owner.path());
            external.extend(web.into_iter().map(|address| (file.clone(), None, address)));
            for target in reached_files {
                follow(document, &mut files, &target);
                if target.extension().is_some_and(|ext| ext == "md") {
                    queue.push_back(target);
                } else if let Ok(linked) = Document::load(&target, &document.served) {
                    queue.extend(deck_pages(&linked, &mut slides));
                }
            }
        }
        let (found, reached) = page_links_with(owner, &widgets, &page, external);
        problems.extend(found);
        for target in reached {
            follow(document, &mut files, &target);
            if target.extension().is_some_and(|ext| ext == "md") {
                queue.push_back(target);
            } else if target
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
            {
                // An invalid one is a problem on the page linking to it.
                if let Ok(linked) = Document::load(&target, &document.served) {
                    queue.extend(deck_pages(&linked, &mut slides));
                }
            }
        }
    }
    for page in &checked {
        problems.extend(notes_problems(document, page, slides.contains(page)));
    }
    problems
}

/// A page's `<Notes>` that are out of place: any on a page that isn't a
/// slide, and every one after the first on a slide.
fn notes_problems(document: &Document, page: &Path, slide: bool) -> Vec<Problem> {
    let source = std::fs::read_to_string(page).unwrap_or_default();
    let file = document.name(page);
    // A slide's first `<Notes>` is its notes; a page has none.
    crate::markup::notes(&source)
        .iter()
        .skip(usize::from(slide))
        .map(|note| Problem {
            file: file.clone(),
            line: Some(note.line),
            message: if slide {
                "a second <Notes> on one slide".to_string()
            } else {
                "<Notes> is only for a slide".to_string()
            },
            target: None,
        })
        .collect()
}

/// One page's problems and reached files; its external links are collected
/// in `external` as (file, line, address).
fn page_links_with(
    document: &Document,
    widgets: &Widgets,
    page: &Path,
    external: &mut Vec<Link>,
) -> (Vec<Problem>, Vec<PathBuf>) {
    let source = std::fs::read_to_string(page).unwrap_or_default();
    let mut unknown = Vec::new();
    let rendered = render::markdown(&crate::widgets::expand(
        &source,
        widgets.names(),
        &mut unknown,
    ));
    let file = document.name(page);
    let mut problems = Vec::new();
    for found in unknown {
        let (line, message) = found.split_once(": ").unwrap_or(("", &found));
        problems.push(Problem {
            file: file.clone(),
            line: line.parse().ok(),
            message: message.to_string(),
            target: None,
        });
    }
    let mut reached = Vec::new();
    let mut targets = html::values(&rendered, "href");
    targets.extend(html::values(&rendered, "src"));
    // A design embedded with `![…](x.design)`, read from the Markdown itself,
    // whatever HTML the page renders it as.
    for target in embedded_designs(&source) {
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    // A diagram's `click … href "…"` lines are links in its drawing, found
    // with the rest; one that doesn't draw is a problem of its own.
    problems.extend(diagram_problems(&source, &file));
    for target in targets {
        let line = source
            .find(&target)
            .map(|at| source[..at].lines().count().max(1));
        let (path, fragment) = match html::Link::classify(&target) {
            html::Link::File { path, fragment } => (path, fragment),
            html::Link::Web(_) => {
                external.push((file.clone(), line, target));
                continue;
            }
            html::Link::Elsewhere => continue, // mailto: and other schemes are not checked
        };
        let resolved = if path.is_empty() {
            Some(page.to_path_buf())
        } else {
            document
                .served
                .inside(&page.parent().unwrap_or(Path::new(".")).join(path))
        };
        // A design's PNG address is the design drawn, when no real file of
        // that name is there: the link is as good as the design.
        let resolved = resolved.or_else(|| {
            let design = path
                .strip_suffix(".png")
                .filter(|p| p.ends_with(".design"))?;
            document
                .served
                .file(&page.parent().unwrap_or(Path::new(".")).join(design))
        });
        let Some(resolved) = resolved else {
            problems.push(Problem {
                file: file.clone(),
                line,
                message: format!("broken link {target} — no such file"),
                target: Some(target.clone()),
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
                    target: Some(target.clone()),
                });
            }
        }
        if resolved.extension().is_some_and(|ext| ext == "design") {
            let address = document.name(&resolved);
            for problem in crate::designs::problems(&document.served, &address) {
                problems.push(Problem {
                    file: file.clone(),
                    line,
                    message: format!("broken design {target} — {problem}"),
                    target: Some(target.clone()),
                });
            }
        }
        if crate::document::kind_of(&resolved).is_some() {
            match Document::load(&resolved, &document.served) {
                Err(error) => {
                    problems.push(Problem {
                        file: file.clone(),
                        line,
                        message: format!(
                            "broken link {target} — {}",
                            relative(&format!("{error:#}"), document.served.path())
                        ),
                        target: Some(target.clone()),
                    });
                    continue;
                }
                // `#n` on a deck must name one of its slides.
                Ok(Document {
                    kind: Kind::Slides { slides, .. },
                    ..
                }) if !fragment.is_empty()
                    && !fragment
                        .parse::<usize>()
                        .is_ok_and(|n| (1..=slides.len()).contains(&n)) =>
                {
                    problems.push(Problem {
                        file: file.clone(),
                        line,
                        message: format!(
                            "broken link {target} — the deck has {} slide{}",
                            slides.len(),
                            if slides.len() == 1 { "" } else { "s" }
                        ),
                        target: Some(target.clone()),
                    });
                }
                // A heading in a doc is named by the id the doc renders it with.
                Ok(
                    linked @ Document {
                        kind: Kind::Doc { .. },
                        ..
                    },
                ) if !fragment.is_empty() => {
                    let Kind::Doc { chapters, pdf } = &linked.kind else {
                        unreachable!()
                    };
                    let linked_widgets = Widgets::of(&linked, widgets.cache());
                    let rendered =
                        render::doc(&linked, &linked_widgets, chapters, pdf, true, true, &[]);
                    if !rendered.contains(&format!("id=\"{fragment}\"")) {
                        problems.push(Problem {
                            file: file.clone(),
                            line,
                            message: format!("broken link {target} — no heading #{fragment}"),
                            target: Some(target.clone()),
                        });
                    }
                }
                Ok(_) => {}
            }
        }
        reached.push(resolved);
    }
    // In a site, a Topics entry outside the site's folder is left out of the sidebar.
    if matches!(document.kind, Kind::Site { .. }) {
        for topic in crate::nav::topics(&source) {
            if let crate::nav::Target::Problem(message) =
                crate::nav::resolve(document, page, &topic.href)
                && message.ends_with("outside the site's folder")
            {
                problems.push(Problem {
                    file: file.clone(),
                    line: source
                        .find(&topic.href)
                        .map(|at| source[..at].lines().count().max(1)),
                    message,
                    target: Some(topic.href.clone()),
                });
            }
        }
    }
    problems.extend(crate::comments::yaml::problems(document, page));
    (problems, reached)
}

/// Each Mermaid fence of a page's Markdown that doesn't draw, at the fence's
/// line. Rendering the page drew them already, so they come from the cache.
fn diagram_problems(source: &str, file: &str) -> Vec<Problem> {
    use comrak::nodes::NodeValue;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, source, &render::options());
    let fences: Vec<(usize, String)> = root
        .descendants()
        .filter_map(|node| {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::CodeBlock(block)
                    if block.fenced && block.info.split_whitespace().next() == Some("mermaid") =>
                {
                    Some((data.sourcepos.start.line, block.literal.clone()))
                }
                _ => None,
            }
        })
        .collect();
    let sources: Vec<String> = fences.iter().map(|(_, s)| s.clone()).collect();
    fences
        .iter()
        .zip(crate::diagrams::draw_all(&sources))
        .filter_map(|((line, _), drawn)| {
            let why = drawn.err()?;
            Some(Problem {
                file: file.to_string(),
                line: Some(*line),
                message: format!("broken diagram — {why}"),
                target: None,
            })
        })
        .collect()
}

/// The `.design` files a page's Markdown embeds or links to, read from its
/// syntax tree — never from code, where `![…](x.design)` is only an example.
fn embedded_designs(source: &str) -> Vec<String> {
    use comrak::nodes::NodeValue;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, source, &comrak::Options::default());
    root.descendants()
        .filter_map(|node| match &node.data.borrow().value {
            NodeValue::Image(link) | NodeValue::Link(link) => Some(link.url.clone()),
            _ => None,
        })
        .filter(|url| {
            url.split('#')
                .next()
                .is_some_and(|path| path.ends_with(".design"))
        })
        .collect()
}

/// The problems of a document's widgets folder, once per document rather than
/// once per page: a file with a system widget's name, and every widget or
/// helper that doesn't compile.
/// The project widget files directly in `folder`, sorted.
fn widget_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| crate::widgets::is_widget_file(path, folder))
        .collect();
    files.sort();
    files
}

fn widget_problems(document: &Document, widgets: &Widgets) -> Vec<Problem> {
    let mut problems: Vec<Problem> = widgets
        .problems
        .iter()
        .map(|found| {
            let (widget_file, message) = found.split_once(" — ").unwrap_or(("", found));
            Problem {
                file: widget_file.to_string(),
                line: None,
                message: message.to_string(),
                target: None,
            }
        })
        .collect();
    let Some(folder) = &document.widgets else {
        return problems;
    };
    for file in widget_files(folder) {
        if let Err(error) = widgets.compile(&file) {
            problems.push(Problem {
                file: document.name(&file),
                line: None,
                message: format!("does not compile: {error}"),
                target: None,
            });
        }
    }
    problems
}

/// An error's text with the served folder's absolute path taken out, so
/// files read as their addresses do.
fn relative(text: &str, served: &Path) -> String {
    text.replace(&format!("{}/", served.display()), "")
}

/// Where the preview tells someone about a problem it has just found: the
/// terminal in `devpage`, a recorder in tests.
pub trait Report: Send + Sync {
    fn problem(&self, problem: &Problem);
}

/// The preview's checks: what each page shows above itself as it is served.
/// One per devpage process, shared by every page — or, in server mode, one
/// per version.
pub struct Preview {
    external: Arc<External>,
    report: Arc<dyn Report>,
    /// Every problem reported so far: each is reported once.
    reported: Mutex<HashSet<Problem>>,
    /// The problems found for each document and its pages, when the files
    /// checked never change: kept, so each is checked once.
    kept: Option<Mutex<Kept>>,
}

/// A document's file and the pages checked, and the problems found on them.
type Kept = HashMap<(PathBuf, Vec<PathBuf>), Vec<Problem>>;

impl Preview {
    /// A preview of files that change: every view checks its page again.
    pub fn new(external: Arc<External>, report: Arc<dyn Report>) -> Self {
        Preview {
            external,
            report,
            reported: Mutex::default(),
            kept: None,
        }
    }

    /// A preview of files that never change — a published version's: each
    /// page is checked on its first view, and every later view gets the same
    /// problems without checking. Its external addresses are best left
    /// unasked: an answer arriving after a page's first view never shows.
    pub fn once(external: Arc<External>, report: Arc<dyn Report>) -> Self {
        Preview {
            kept: Some(Mutex::default()),
            ..Preview::new(external, report)
        }
    }

    /// The problems on the given pages: each is reported the first time it is
    /// found, and returned for the page to list. External
    /// addresses are checked in the background: a definite error shows from
    /// the next render, which its answer triggers; an address that can't be
    /// reached is retried, and shows only after three failures in a row,
    /// until it answers again.
    pub fn problems(
        &self,
        document: &Document,
        widgets: &Widgets,
        pages: &[PathBuf],
    ) -> Vec<Problem> {
        let Some(kept) = &self.kept else {
            return self.check(document, widgets, pages);
        };
        let key = (document.path(), pages.to_vec());
        let lock = || kept.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(found) = lock().get(&key) {
            return found.clone();
        }
        // Checked without the lock, so a slow page holds up no other; two
        // first views at once both check, and find the same.
        let found = self.check(document, widgets, pages);
        lock().entry(key).or_insert(found).clone()
    }

    /// The problems on the given pages, checked now.
    fn check(&self, document: &Document, widgets: &Widgets, pages: &[PathBuf]) -> Vec<Problem> {
        let _check = tracing::info_span!("check").entered();
        let mut external = Vec::new();
        let mut problems = widget_problems(document, widgets);
        problems.extend(crate::navbar::problems(document));
        problems.extend(crate::navbar::logo_problem(document));
        problems.extend(
            pages
                .iter()
                .flat_map(|page| page_links_with(document, widgets, page, &mut external).0),
        );
        // The pages of a deck are its slides; any other page is not one.
        let slide = matches!(document.kind, Kind::Slides { .. });
        problems.extend(
            pages
                .iter()
                .flat_map(|page| notes_problems(document, page, slide)),
        );
        problems.extend(self.external.preview(external));
        let mut reported = self
            .reported
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for problem in &problems {
            if reported.insert(problem.clone()) {
                self.report.problem(problem);
            }
        }
        problems
    }
}

/// Previews for tests that serve pages.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::{Preview, Problem, Report};
    use crate::external::testing::offline;
    use std::sync::Arc;

    /// Tells no one.
    struct Silent;

    impl Report for Silent {
        fn problem(&self, _: &Problem) {}
    }

    /// A preview that asks no external address, announces on nothing and
    /// reports to no one.
    pub fn preview() -> Arc<Preview> {
        Arc::new(Preview::new(offline(), Arc::new(Silent)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::served::ServedFolder;

    /// `document`'s widget set, read through a cache of its own.
    fn widgets_of(document: &Document) -> Widgets {
        Widgets::of(document, &crate::widgets::Modules::default())
    }

    #[test]
    fn finds_missing_files_headings_and_climbs_out_of_the_served_folder() {
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
        let doc = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        let found: Vec<String> = internal(&doc, &mut Vec::new(), None)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 5, "{found:#?}");
        assert!(
            found.contains(&"README.md:3: broken link nope.md — no such file".to_string()),
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

    #[test]
    fn a_diagram_that_does_not_parse_is_a_problem_at_its_fence() {
        let dir =
            std::env::temp_dir().join(format!("devpage-check-diagram-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let page = "---\ntitle: x\n---\n# Home\n\n```mermaid\nflowchart LR\n  a --> b\n```\n\n````markdown\n```mermaid\nnot a diagram\n```\n````\n\n```mermaid\nsequenceDiagram\n  A->>B: one; two\n```\n";
        std::fs::write(dir.join("README.md"), page).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        let doc = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        let expected = "README.md:17: broken diagram — Parse error on line 2:\n  A->>B: one; two\n";
        // In `devpage check`, and in the page's problems.
        for found in [
            internal(&doc, &mut Vec::new(), None),
            testing::preview().problems(&doc, &widgets_of(&doc), &[doc.base.join("README.md")]),
        ] {
            let found: Vec<String> = found.iter().map(ToString::to_string).collect();
            assert_eq!(found.len(), 1, "{found:#?}");
            assert!(found[0].starts_with(expected), "{found:#?}");
            assert!(found[0].contains("got 'NEWLINE'"), "{found:#?}");
        }
    }
}

#[cfg(test)]
mod external {
    use super::*;
    use crate::served::ServedFolder;
    use crate::widgets::Modules;

    /// `document`'s widget set, read through a cache of its own.
    fn widgets_of(document: &Document) -> Widgets {
        Widgets::of(document, &Modules::default())
    }
    use crate::external::testing::offline;

    fn folder(name: &str, page: &str) -> Document {
        let dir = std::env::temp_dir().join(format!("devpage-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), page).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_page_in_no_site_is_checked_as_a_doc() {
        let top = std::env::temp_dir().join(format!("devpage-no-site-{}", std::process::id()));
        let docs = top.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(docs.join("README.md"), "# Docs\n\n[repo](../README.md)\n").unwrap();
        std::fs::write(
            top.join("README.md"),
            "# Repo\n\n## Topics\n\n- [Contributing](CONTRIBUTING.md) — how.\n",
        )
        .unwrap();
        std::fs::write(top.join("CONTRIBUTING.md"), "# Contributing").unwrap();
        let site = Document::load(&docs, &ServedFolder::new(&top).unwrap()).unwrap();
        let found: Vec<String> = document(&site, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(found.is_empty(), "{found:#?}");
    }

    #[tokio::test]
    async fn topics_are_checked_against_the_pages_own_site() {
        let doc = folder("sibling-topics", "# Home\n\n[other](o2/devpage.yml)\n");
        let o2 = doc.base.join("o2");
        std::fs::create_dir_all(o2.join("x")).unwrap();
        std::fs::write(o2.join("x/p.md"), "# P").unwrap();
        std::fs::write(doc.base.join("outside.md"), "# Outside").unwrap();
        std::fs::write(
            o2.join("devpage.yml"),
            "kind: site\ntitle: O\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(
            o2.join("README.md"),
            "# O\n\n## Topics\n\n- [P](x/p.md) — inside.\n- [Out](../outside.md) — not.\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0].starts_with("o2/README.md:6: Topics entry ../outside.md"),
            "{found:#?}"
        );
    }

    #[tokio::test]
    async fn a_heading_in_a_doc_is_checked_against_the_ids_it_renders() {
        let doc = folder(
            "doc-fragment",
            "# Home\n\n[real](d/devpage.yml#c1--real) [chapter](d/devpage.yml#c2) [none](d/devpage.yml#nosuch) [bare](d/devpage.yml#real)\n",
        );
        let d = doc.base.join("d");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("c1.md"), "# One\n\n## Real\n").unwrap();
        std::fs::write(d.join("c2.md"), "# Two\n").unwrap();
        std::fs::write(
            d.join("devpage.yml"),
            "kind: doc\ntitle: D\nchapters: [c1.md, c2.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                "README.md:3: broken link d/devpage.yml#nosuch — no heading #nosuch".to_string(),
                "README.md:3: broken link d/devpage.yml#real — no heading #real".to_string(),
            ],
            "{found:#?}"
        );
    }

    #[tokio::test]
    async fn a_slide_number_must_be_one_the_deck_has() {
        let doc = folder(
            "deck-number",
            "# Home\n\n[two](deck/devpage.yml#2) [nine](deck/devpage.yml#9) [word](deck/devpage.yml#intro)\n",
        );
        std::fs::create_dir_all(doc.base.join("deck")).unwrap();
        std::fs::write(doc.base.join("deck/a.md"), "# A").unwrap();
        std::fs::write(doc.base.join("deck/b.md"), "# B").unwrap();
        std::fs::write(
            doc.base.join("deck/devpage.yml"),
            "kind: slides\ntitle: D\nslides: [a.md, b.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                "README.md:3: broken link deck/devpage.yml#9 — the deck has 2 slides".to_string(),
                "README.md:3: broken link deck/devpage.yml#intro — the deck has 2 slides"
                    .to_string(),
            ],
            "{found:#?}"
        );
    }

    #[tokio::test]
    async fn notes_belong_on_a_slide_once() {
        let doc = folder(
            "notes",
            "# Home\n\n[deck](deck/devpage.yml) [page](page.md) [slide](deck/a.md)\n",
        );
        std::fs::create_dir_all(doc.base.join("deck")).unwrap();
        std::fs::write(
            doc.base.join("deck/a.md"),
            "# A\n\n<Notes>one</Notes>\n\n<Notes>two</Notes>\n",
        )
        .unwrap();
        std::fs::write(doc.base.join("deck/b.md"), "# B\n\n<Notes>fine</Notes>\n").unwrap();
        std::fs::write(
            doc.base.join("deck/devpage.yml"),
            "kind: slides\ntitle: D\nslides: [a.md, b.md]\n",
        )
        .unwrap();
        std::fs::write(doc.base.join("page.md"), "# Page\n\n<Notes>lost</Notes>\n").unwrap();
        let mut found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![
                "deck/a.md:5: a second <Notes> on one slide".to_string(),
                "page.md:3: <Notes> is only for a slide".to_string(),
            ],
            "{found:#?}"
        );
        // The preview: a deck's slides, and a page of the site.
        let deck = Document::load(
            &doc.base.join("deck/devpage.yml"),
            &ServedFolder::new(&doc.base).unwrap(),
        )
        .unwrap();
        let shown: Vec<String> = preview(&deck, &pages(&deck))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(shown, vec!["deck/a.md:5: a second <Notes> on one slide"]);
        let shown: Vec<String> = preview(&doc, &[doc.base.join("page.md")])
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(shown, vec!["page.md:3: <Notes> is only for a slide"]);
    }

    #[tokio::test]
    async fn unclosed_notes_are_a_problem() {
        let doc = folder("unclosed-notes", "# Home\n\n[deck](deck/devpage.yml)\n");
        std::fs::create_dir_all(doc.base.join("deck")).unwrap();
        std::fs::write(
            doc.base.join("deck/a.md"),
            "# A\n\n<Notes>\nSay this.\n</Note>\n",
        )
        .unwrap();
        std::fs::write(
            doc.base.join("deck/devpage.yml"),
            "kind: slides\ntitle: D\nslides: [a.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        let expected = vec!["deck/a.md:3: a <Notes> without its </Notes>".to_string()];
        assert_eq!(found, expected, "{found:#?}");
        let deck = Document::load(
            &doc.base.join("deck/devpage.yml"),
            &ServedFolder::new(&doc.base).unwrap(),
        )
        .unwrap();
        let shown: Vec<String> = preview(&deck, &pages(&deck))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(shown, expected);
    }

    #[tokio::test]
    async fn a_broken_linked_document_is_named_by_its_address() {
        let doc = folder("linked-relative", "# Home\n\n[plan](plan/devpage.yml)\n");
        std::fs::create_dir_all(doc.base.join("plan")).unwrap();
        std::fs::write(
            doc.base.join("plan/devpage.yml"),
            "kind: doc\ntitle: P\nchapters: [missing.md]\n",
        )
        .unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            !found[0].contains(&doc.served.path().display().to_string()),
            "{found:#?}"
        );
        assert!(found[0].contains("in plan/devpage.yml"), "{found:#?}");
    }

    /// The problems a fresh preview finds at once.
    fn preview(doc: &Document, pages: &[PathBuf]) -> Vec<Problem> {
        Preview::new(offline(), Arc::new(Recorder::default())).problems(
            doc,
            &widgets_of(doc),
            pages,
        )
    }

    /// A reporter that keeps what it is told, for tests.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Report for Recorder {
        fn problem(&self, problem: &Problem) {
            self.0.lock().unwrap().push(problem.to_string());
        }
    }

    #[test]
    fn the_preview_reports_each_problem_once_to_its_reporter() {
        let doc = folder("preview-report", "# Home\n\n[gone](gone.md)\n");
        let recorder = Arc::new(Recorder::default());
        let preview = Preview::new(offline(), recorder.clone());
        let pages = [doc.base.join("README.md")];
        assert_eq!(preview.problems(&doc, &widgets_of(&doc), &pages).len(), 1);
        assert_eq!(preview.problems(&doc, &widgets_of(&doc), &pages).len(), 1);
        assert_eq!(
            *recorder.0.lock().unwrap(),
            ["README.md:3: broken link gone.md — no such file"]
        );
    }

    #[test]
    fn a_preview_of_files_that_never_change_checks_each_page_once() {
        let doc = folder(
            "preview-once",
            "# Home\n\n[gone](gone.md) and [heading](README.md#nope)\n",
        );
        let preview = Preview::once(offline(), Arc::new(Recorder::default()));
        let pages = [doc.base.join("README.md")];
        let first = preview.problems(&doc, &widgets_of(&doc), &pages);
        assert_eq!(first.len(), 2, "{first:#?}");
        // Fixed on disk, which a version never is: the kept answer shows it
        // was not checked again.
        std::fs::write(doc.base.join("README.md"), "# Home\n").unwrap();
        for _ in 0..9 {
            assert_eq!(preview.problems(&doc, &widgets_of(&doc), &pages), first);
        }
        // Another page is checked on its own.
        std::fs::write(doc.base.join("two.md"), "# Two\n\n[gone](gone.md)\n").unwrap();
        let two = [doc.base.join("two.md")];
        assert_eq!(preview.problems(&doc, &widgets_of(&doc), &two).len(), 1);
    }

    #[test]
    fn a_preview_of_files_that_change_checks_on_every_view() {
        let doc = folder("preview-every", "# Home\n\n[gone](gone.md)\n");
        let preview = Preview::new(offline(), Arc::new(Recorder::default()));
        let pages = [doc.base.join("README.md")];
        assert_eq!(preview.problems(&doc, &widgets_of(&doc), &pages).len(), 1);
        std::fs::write(doc.base.join("README.md"), "# Home\n").unwrap();
        assert!(preview.problems(&doc, &widgets_of(&doc), &pages).is_empty());
    }

    #[tokio::test]
    async fn check_reports_the_navbar_once_and_follows_its_links() {
        let doc = folder("navbar-check", "# Home\n\n[Two](two.md)\n");
        std::fs::write(doc.base.join("two.md"), "# Two\n").unwrap();
        // Reached only through the navbar, with a broken link of its own.
        std::fs::write(doc.base.join("only.md"), "# Only\n\n[gone](gone.md)\n").unwrap();
        std::fs::write(
            doc.base.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  Only (no-such-icon): only.md\n  Missing: missing.md\n",
        )
        .unwrap();
        let doc = Document::load(&doc.base, &doc.served).unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        let count = |text: &str| found.iter().filter(|p| p.contains(text)).count();
        assert_eq!(count("navbar: no icon named no-such-icon"), 1, "{found:#?}");
        assert_eq!(count("navbar: missing.md — no such file"), 1, "{found:#?}");
        assert_eq!(count("only.md:3: broken link gone.md"), 1, "{found:#?}");
    }

    #[tokio::test]
    async fn check_reports_a_missing_logo_once() {
        let doc = folder("logo-check", "# Home\n\n[Two](two.md)\n");
        std::fs::write(doc.base.join("two.md"), "# Two\n").unwrap();
        std::fs::write(
            doc.base.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nlogo: gone.svg\n",
        )
        .unwrap();
        let doc = Document::load(&doc.base, &doc.served).unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        let count = found
            .iter()
            .filter(|p| p.contains("logo: gone.svg — no such file"))
            .count();
        assert_eq!(count, 1, "{found:#?}");
    }

    #[tokio::test]
    async fn widget_problems_are_reported_by_check() {
        let doc = folder(
            "widget-problems",
            "# Home\n\n<Nope />\n\n<Callout>fine</Callout>\n\n[Two](two.md)\n",
        );
        std::fs::write(
            doc.base.join("two.md"),
            "# Two\n\n<Callout>also</Callout>\n",
        )
        .unwrap();
        std::fs::create_dir_all(doc.base.join("widgets")).unwrap();
        std::fs::write(
            doc.base.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(
            doc.base.join("widgets/Callout.tsx"),
            "export default () => null;",
        )
        .unwrap();
        std::fs::write(doc.base.join("widgets/Broken.tsx"), "export default (").unwrap();
        // A helper that doesn't compile is a problem too.
        std::fs::write(doc.base.join("widgets/fmt.ts"), "export const x = ;").unwrap();
        let doc = Document::load(&doc.base, &doc.served).unwrap();
        let found: Vec<String> = document(&doc, None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            found
                .iter()
                .any(|p| p == "README.md:3: <Nope> is not a widget"),
            "{found:#?}"
        );
        assert!(
            found
                .iter()
                .any(|p| p.contains("widgets/Callout.tsx") && p.contains("system widget's name")),
            "{found:#?}"
        );
        assert!(
            found
                .iter()
                .any(|p| p.contains("widgets/Broken.tsx") && p.contains("does not compile")),
            "{found:#?}"
        );
        assert!(
            found
                .iter()
                .any(|p| p.contains("widgets/fmt.ts: does not compile: line 1:")),
            "{found:#?}"
        );
        assert!(
            !found.iter().any(|p| p.contains("<Callout> is not")),
            "{found:#?}"
        );
        // Two pages, but the widgets folder's problems are reported once.
        for once in ["Callout.tsx", "Broken.tsx", "fmt.ts"] {
            assert_eq!(
                found.iter().filter(|p| p.contains(once)).count(),
                1,
                "{once}: {found:#?}"
            );
        }
    }
}

#[cfg(test)]
mod documents {
    use super::*;
    use crate::served::ServedFolder;

    /// The top of the repository the core crate sits in.
    fn repository() -> PathBuf {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn the_fixture_doc_the_catalog_and_the_talk_have_no_broken_links() {
        for example in [
            "tests/fixtures/sync-design",
            "docs/talks/devpage",
            "docs/features",
        ] {
            let root = repository();
            let doc =
                Document::load(&root.join(example), &ServedFolder::new(&root).unwrap()).unwrap();
            let problems = internal(&doc, &mut Vec::new(), None);
            assert!(problems.is_empty(), "{example}: {problems:#?}");
        }
    }

    /// devpage's own docs, from their root page: every internal link and
    /// anchor must hold. External links aren't checked, so no network.
    #[test]
    fn devpage_docs_have_no_broken_links() {
        let root = repository();
        let doc = Document::load(&root.join("docs"), &ServedFolder::new(&root).unwrap()).unwrap();
        let problems: Vec<String> = internal(&doc, &mut Vec::new(), None)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
