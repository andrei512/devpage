//! Link checking: every link, image and diagram target a page holds must
//! reach a file in the served folder — and a heading on it, when it names one —
//! every external address must answer, and every diagram must draw.

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::designs::Designs;
use crate::document::{Document, Kind};
use crate::folder::Folder;
use crate::page::{self, Rendered};

use crate::external::{External, Link};
use crate::html;
use crate::problem::Problem;
use crate::widgets::{Modules, Widgets};

/// The pages a document is made of.
fn pages(document: &Document) -> Vec<PathBuf> {
    match &document.kind {
        Kind::Site { root } => vec![document.base.join(root)],
        Kind::Doc { chapters, .. } => chapters.clone(),
        Kind::Slides { slides, .. } => slides.iter().map(|slide| slide.file.clone()).collect(),
        Kind::Design => Vec::new(),
    }
}

/// Checks the folder's entry document and every page and document reachable
/// from it by links inside the served folder, each design it reaches as
/// `designs` — the folder's design store — holds it.
/// External links are checked when `checker` is given.
pub async fn document(
    folder: &Folder,
    designs: &Designs,
    checker: Option<&External>,
) -> Vec<Problem> {
    let document = &*folder.entry();
    let mut external = Vec::new();
    let mut problems = internal(folder, designs, document, &mut external, None);
    if let Kind::Design = document.kind {
        let address = document.part_name(&document.path());
        problems.extend(
            designs
                .problems(&address)
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

/// Every file the check follows from the folder's entry document, each once
/// and in path order: the document file, its pages, chapters or slides and every page
/// and document they reach, the images and files their links name, a design's
/// imports, images, themes and fonts, the logo, and the project widgets with
/// every module they import — what the document is made of, and nothing else
/// beside it. External addresses are not followed. A design's files are the
/// ones `designs`, the folder's design store, resolved it from.
pub fn files(folder: &Folder, designs: &Designs) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    internal(
        folder,
        designs,
        &folder.entry(),
        &mut Vec::new(),
        Some(&mut files),
    );
    files.into_iter().collect()
}

/// A file the walk followed, kept when the caller asked for them: only
/// regular files inside the served folder, and every file a design is made
/// of, as `designs` read them.
fn follow(
    document: &Document,
    designs: &Designs,
    files: &mut Option<&mut BTreeSet<PathBuf>>,
    file: &Path,
) {
    let Some(files) = files.as_deref_mut() else {
        return;
    };
    if let Some(real) = document.served.file(file)
        && files.insert(real.clone())
        && real.extension().is_some_and(|ext| ext == "design")
        && let Some(address) = document.name(&real)
    {
        files.extend(designs.files(&address));
    }
}

fn internal(
    folder: &Folder,
    designs: &Designs,
    document: &Document,
    external: &mut Vec<Link>,
    mut files: Option<&mut BTreeSet<PathBuf>>,
) -> Vec<Problem> {
    follow(document, designs, &mut files, &document.path());
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
    // check started from — as the preview shows it. Widgets are read and
    // compiled once per check.
    let modules = Modules::default();
    let mut widget_folders = HashSet::new();
    let mut navbars = HashSet::new();
    let mut logos = HashSet::new();
    while let Some(page) = queue.pop_front() {
        if !seen.insert(page.clone()) {
            continue;
        }
        follow(document, designs, &mut files, &page);
        let owner = match folder.owner(&page) {
            Some(owner) => owner.document.ok(),
            // In no site, a page is shown as a doc of one chapter — unless it
            // is one of the doc's or deck's own files the check started from.
            None if matches!(document.kind, Kind::Site { .. }) => {
                Document::load(&page, &document.served).ok()
            }
            None => None,
        };
        let owner = owner.as_ref().unwrap_or(document);
        follow(document, designs, &mut files, &owner.path());
        let widgets = Widgets::of(owner, &modules);
        if let Some(folder) = &owner.widgets
            && widget_folders.insert(folder.clone())
        {
            problems.extend(widget_problems(owner, &widgets));
            if files.is_some() {
                for widget in widget_files(folder) {
                    let snapshot = modules.snapshot(&document.served, &widget);
                    for (module, _) in snapshot.closure(&widget) {
                        follow(document, designs, &mut files, &module);
                    }
                }
            }
        }
        if let Some(logo) = &owner.logo {
            follow(document, designs, &mut files, &owner.base.join(logo));
        }
        // A document's logo is checked once.
        if logos.insert(owner.path())
            && let Some(problem) = page::navbar::logo_problem(owner)
        {
            problems.push(problem);
        }
        // A site's navbar is checked once, and what it links to is checked too.
        if !owner.navbar.is_empty() && navbars.insert(owner.path()) {
            problems.extend(page::navbar::problems(owner));
            let (reached_files, web) = page::navbar::reached(owner);
            let file = owner.part_name(&owner.path());
            external.extend(web.into_iter().map(|address| (file.clone(), None, address)));
            for target in reached_files {
                follow(document, designs, &mut files, &target);
                if target.extension().is_some_and(|ext| ext == "md") {
                    queue.push_back(target);
                } else if let Ok(linked) = Document::load(&target, &document.served) {
                    queue.extend(deck_pages(&linked, &mut slides));
                }
            }
        }
        let rendered = page::read(&widgets, &page);
        let (found, reached) = page_links_with(owner, designs, &rendered, external);
        problems.extend(found);
        checked.push(rendered);
        for target in reached {
            follow(document, designs, &mut files, &target);
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
    for rendered in &checked {
        problems.extend(notes_problems(
            document,
            rendered,
            slides.contains(rendered.file()),
        ));
    }
    problems
}

/// A page's `<Notes>` that are out of place: any on a page that isn't a
/// slide, and every one after the first on a slide.
fn notes_problems(document: &Document, rendered: &Rendered, slide: bool) -> Vec<Problem> {
    // A page outside the served folder is no page of the check: the link to
    // it is the problem.
    let Some(file) = document.name(rendered.file()) else {
        return Vec::new();
    };
    // A slide's first `<Notes>` is its notes; a page has none.
    crate::markup::notes(rendered.source())
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

/// One page's problems and reached files, judged from its `rendered`
/// content — the content its page shows; its external links are collected
/// in `external` as (file, line, address).
fn page_links_with(
    document: &Document,
    designs: &Designs,
    rendered: &Rendered,
    external: &mut Vec<Link>,
) -> (Vec<Problem>, Vec<PathBuf>) {
    let page = rendered.file();
    let source = rendered.source();
    // A page outside the served folder is no page of the check: the link to
    // it is the problem.
    let Some(file) = document.name(page) else {
        return (Vec::new(), Vec::new());
    };
    let mut problems = Vec::new();
    for found in rendered.tag_problems() {
        let (line, message) = found.split_once(": ").unwrap_or(("", found));
        problems.push(Problem {
            file: file.clone(),
            line: line.parse().ok(),
            message: message.to_string(),
            target: None,
        });
    }
    let mut reached = Vec::new();
    // A diagram's `click … href "…"` lines are links in its drawing, found
    // with the rest; one that doesn't draw is a problem of its own.
    problems.extend(diagram_problems(source, &file));
    for link in rendered.links() {
        let (target, line) = (link.target.clone(), link.line);
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
        // A heading on another page is named by an id that page renders.
        let no_heading = || Problem {
            file: file.clone(),
            line,
            message: format!("broken link {target} — no heading #{fragment}"),
            target: Some(target.clone()),
        };
        if !fragment.is_empty()
            && resolved.extension().is_some_and(|ext| ext == "md")
            && fragment.parse::<usize>().is_err()
            && !page::ids(&resolved, &document.served).contains(fragment)
        {
            problems.push(no_heading());
        }
        if resolved.extension().is_some_and(|ext| ext == "design")
            && let Some(address) = document.name(&resolved)
        {
            for problem in designs.problems(&address) {
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
                Ok(Document {
                    kind: Kind::Doc { .. },
                    ..
                }) if !fragment.is_empty()
                    && !page::ids(&resolved, &document.served).contains(fragment) =>
                {
                    problems.push(no_heading());
                }
                Ok(_) => {}
            }
        }
        reached.push(resolved);
    }
    // In a site, a Topics entry outside the site's folder is left out of the sidebar.
    if matches!(document.kind, Kind::Site { .. }) {
        for topic in crate::nav::topics(source) {
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
    let root = comrak::parse_document(&arena, source, &page::markdown::options());
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
        // A widget file outside the served folder is never read.
        let Some(name) = document.name(&file) else {
            continue;
        };
        if let Err(error) = widgets.compile(&file) {
            problems.push(Problem {
                file: name,
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
/// One per devpage process, shared by every page.
pub struct Preview {
    external: Arc<External>,
    report: Arc<dyn Report>,
    /// Every problem reported so far: each is reported once.
    reported: Mutex<HashSet<Problem>>,
}

impl Preview {
    pub fn new(external: Arc<External>, report: Arc<dyn Report>) -> Self {
        Preview {
            external,
            report,
            reported: Mutex::default(),
        }
    }

    /// The problems on the given pages — each page's content as its page
    /// shows it, rendered once for both — each design they reach as `designs`
    /// — the served folder's design store — holds it: each is reported the
    /// first time it is found, and returned for the page to list. External
    /// addresses are checked in the background: a definite error shows from
    /// the next render, which its answer triggers; an address that can't be
    /// reached is retried, and shows only after three failures in a row,
    /// until it answers again.
    pub fn problems(
        &self,
        document: &Document,
        widgets: &Widgets,
        designs: &Designs,
        pages: &[Rendered],
    ) -> Vec<Problem> {
        let mut external = Vec::new();
        let mut problems = widget_problems(document, widgets);
        problems.extend(page::navbar::problems(document));
        problems.extend(page::navbar::logo_problem(document));
        problems.extend(
            pages
                .iter()
                .flat_map(|page| page_links_with(document, designs, page, &mut external).0),
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
    use crate::folder::ServedFolder;

    /// A folder opened on `document`, as `devpage check` opens one.
    pub(super) fn opened(document: &Document) -> Folder {
        Folder::new(Arc::new(
            Document::load(&document.path(), &document.served).unwrap(),
        ))
    }

    /// `document`'s widget set, read through a cache of its own.
    fn widgets_of(document: &Document) -> Widgets {
        Widgets::of(document, &crate::widgets::Modules::default())
    }

    /// The content of each of `files`, as `document` shows it: what the
    /// preview is given.
    pub(super) fn rendered(document: &Document, files: &[PathBuf]) -> Vec<Rendered> {
        let widgets = widgets_of(document);
        files
            .iter()
            .map(|file| page::read(&widgets, file))
            .collect()
    }

    /// A design store of its own for `document`'s served folder, as
    /// `devpage check` makes one.
    pub(super) fn designs_of(document: &Document) -> Designs {
        Designs::new(&document.served)
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
        let found: Vec<String> = internal(
            &opened(&doc),
            &designs_of(&doc),
            &doc,
            &mut Vec::new(),
            None,
        )
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
            internal(
                &opened(&doc),
                &designs_of(&doc),
                &doc,
                &mut Vec::new(),
                None,
            ),
            testing::preview().problems(
                &doc,
                &widgets_of(&doc),
                &designs_of(&doc),
                &rendered(&doc, &[doc.base.join("README.md")]),
            ),
        ] {
            let found: Vec<String> = found.iter().map(ToString::to_string).collect();
            assert_eq!(found.len(), 1, "{found:#?}");
            assert!(found[0].starts_with(expected), "{found:#?}");
            assert!(found[0].contains("got 'NEWLINE'"), "{found:#?}");
        }
    }

    /// A design's problems and files come from the store the check is
    /// given: a design document checked and then zipped is resolved once,
    /// there.
    #[tokio::test]
    async fn a_design_is_checked_and_followed_through_the_store() {
        let dir = crate::designs::testing::served("check-store");
        let served = ServedFolder::new(&dir).unwrap();
        let folder = Folder::new(Arc::new(
            Document::load(&dir.join("app/screen.design"), &served).unwrap(),
        ));
        let designs = Designs::new(&served);
        assert!(document(&folder, &designs, None).await.is_empty());
        assert_eq!(
            files(&folder, &designs),
            [dir.join("app/button.design"), dir.join("app/screen.design")]
        );
        assert_eq!(designs.resolutions(), 1, "resolved once, in the store");
    }

    /// A page's problems name a design it embeds as the server's store holds
    /// it: checking the page keeps the design there, and a save the store is
    /// told of shows on the page.
    #[test]
    fn a_page_shows_an_embedded_designs_problems_as_the_store_holds_them() {
        let dir = crate::designs::testing::served("preview-store");
        let screen = dir.join("app/screen.design");
        let broken = crate::designs::testing::SCREEN.replace("<Button label", "<Button lable");
        std::fs::write(&screen, &broken).unwrap();
        std::fs::write(
            dir.join("page.md"),
            "# Page\n\n![Screen](app/screen.design)\n",
        )
        .unwrap();
        let doc = Document::load(&dir.join("page.md"), &ServedFolder::new(&dir).unwrap()).unwrap();
        let designs = Designs::new(&doc.served);
        let preview = testing::preview();
        let pages = rendered(&doc, &[dir.join("page.md")]);
        let found = preview.problems(&doc, &widgets_of(&doc), &designs, &pages);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0]
                .to_string()
                .starts_with("page.md:3: broken design app/screen.design — app/screen.design:5:"),
            "{found:#?}"
        );
        assert!(designs.resolve("app/screen.design").is_err());
        assert_eq!(designs.resolutions(), 1, "the page's check kept it");
        crate::designs::testing::save(&designs, &screen, crate::designs::testing::SCREEN);
        let found = preview.problems(&doc, &widgets_of(&doc), &designs, &pages);
        assert!(found.is_empty(), "{found:#?}");
    }
}

#[cfg(test)]
mod external {
    use super::tests::{designs_of, opened, rendered};
    use super::*;
    use crate::folder::ServedFolder;
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
        let found: Vec<String> = document(&opened(&site), &designs_of(&site), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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

    /// A link into another page — a Markdown file or a doc — must land on an
    /// id that page renders: `devpage check` and the preview say so alike.
    #[tokio::test]
    async fn a_broken_cross_page_anchor_is_a_problem_in_check_and_the_preview() {
        let doc = folder(
            "cross-anchor",
            "# Home\n\n[ok](other.md#there) [gone](other.md#nowhere)\n\n[ok](d/doc.yml#c1--real) [gone](d/doc.yml#nowhere)\n",
        );
        std::fs::write(doc.base.join("other.md"), "# Other\n\n## There\n").unwrap();
        let d = doc.base.join("d");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("c1.md"), "# One\n\n## Real\n").unwrap();
        std::fs::write(d.join("c2.md"), "# Two\n").unwrap();
        std::fs::write(
            d.join("doc.yml"),
            "kind: doc\ntitle: D\nchapters: [c1.md, c2.md]\n",
        )
        .unwrap();
        let expected = [
            "README.md:3: broken link other.md#nowhere — no heading #nowhere",
            "README.md:5: broken link d/doc.yml#nowhere — no heading #nowhere",
        ];
        let checked: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
            .await
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(checked, expected, "{checked:#?}");
        let previewed: Vec<String> = preview(&doc, &[doc.base.join("README.md")])
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(previewed, expected, "{previewed:#?}");
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let mut found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
            &designs_of(doc),
            &rendered(doc, pages),
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
        let pages = rendered(&doc, &[doc.base.join("README.md")]);
        assert_eq!(
            preview
                .problems(&doc, &widgets_of(&doc), &designs_of(&doc), &pages)
                .len(),
            1
        );
        assert_eq!(
            preview
                .problems(&doc, &widgets_of(&doc), &designs_of(&doc), &pages)
                .len(),
            1
        );
        assert_eq!(
            *recorder.0.lock().unwrap(),
            ["README.md:3: broken link gone.md — no such file"]
        );
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
        let found: Vec<String> = document(&opened(&doc), &designs_of(&doc), None)
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
    use super::tests::{designs_of, opened};
    use super::*;
    use crate::folder::ServedFolder;

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
            let problems = internal(
                &opened(&doc),
                &designs_of(&doc),
                &doc,
                &mut Vec::new(),
                None,
            );
            assert!(problems.is_empty(), "{example}: {problems:#?}");
        }
    }

    /// devpage's own docs, from their root page: every internal link and
    /// anchor must hold. External links aren't checked, so no network.
    #[test]
    fn devpage_docs_have_no_broken_links() {
        let root = repository();
        let doc = Document::load(&root.join("docs"), &ServedFolder::new(&root).unwrap()).unwrap();
        let problems: Vec<String> = internal(
            &opened(&doc),
            &designs_of(&doc),
            &doc,
            &mut Vec::new(),
            None,
        )
        .iter()
        .map(ToString::to_string)
        .collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
