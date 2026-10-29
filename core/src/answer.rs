//! What a request is answered with: a page, a redirect, a file's bytes, a
//! design's or a slide's PNG, a widget's module, or why there is none.
//! Decided here from the documents alone, so every route is tested without
//! a socket; the HTTP server only turns an [`Answer`] into a response — its
//! headers, the file's bytes read, the PNGs it names drawn.

use std::path::PathBuf;

use devpage_design::Theme;

use crate::check::Preview;
use crate::designs::Designs;
use crate::document::{self, Document, Kind};
use crate::folder::{Folder, Owner};
use crate::page;
use crate::widgets::{self, Compiled, Modules, Widgets};

/// What a request is answered with.
#[derive(Debug, PartialEq)]
pub enum Answer {
    /// A rendered page: a document, a page of one, or a design's canvas
    /// page.
    Html(String),
    /// Elsewhere: an address, mount prefix included.
    Redirect(String),
    /// A file of the served folder, sent as it is.
    File(PathBuf),
    /// The PNG of the design at `design`, a path in the served folder.
    DesignPng { design: String, theme: Theme },
    /// Slide `n` of the deck whose page is at `page` — the deck's own
    /// address, without the mount prefix, as Chrome asks the server for it;
    /// `deck` is its file.
    SlidePng {
        page: String,
        n: usize,
        deck: PathBuf,
    },
    /// A widget's module, compiled; `immutable` when asked for at its current
    /// address, since what answers that never changes.
    Module { js: String, immutable: bool },
    /// A module that throws why a widget's module can't be had — it, or a
    /// helper it imports, doesn't compile — so the page shows it in the
    /// widget's place.
    ModuleError(String),
    /// 404, and why.
    NotFound(String),
    /// 400, and why.
    BadRequest(String),
    /// 500, and why.
    Failed(String),
}

/// What decides an answer: the served folder with its entry document, its
/// widget modules, its design store, the problems each page lists and
/// whether comments are on.
/// Every address it answers with is under the served folder's mount, which
/// the documents carry.
pub struct Answers<'a> {
    folder: &'a Folder,
    modules: &'a Modules,
    designs: &'a Designs,
    preview: &'a Preview,
    comments: bool,
}

impl<'a> Answers<'a> {
    /// Answers from `folder`, its widget `modules`, its design store
    /// `designs` and the `preview` that finds each page's problems, with
    /// comments shown or not.
    pub fn new(
        folder: &'a Folder,
        modules: &'a Modules,
        designs: &'a Designs,
        preview: &'a Preview,
        comments: bool,
    ) -> Self {
        Self {
            folder,
            modules,
            designs,
            preview,
            comments,
        }
    }

    /// `/` leads to the entry document as its file says now — or, while the
    /// file is invalid, shows why.
    pub fn home(&self) -> Answer {
        match self.folder.entry_error() {
            Some(error) => page::invalid(&self.folder.entry(), self.folder.entry_file(), &error)
                .map_or_else(
                    || Answer::NotFound("/ is not part of the served folder".to_string()),
                    Answer::Html,
                ),
            None => self.redirect(self.folder.entry().entry()),
        }
    }

    /// `path` — relative to the served folder, already decoded — asked with
    /// `query`. A document file opens as its kind; a Markdown file becomes a
    /// page of its nearest site, or a doc of one chapter; `<design>.png` and
    /// `<deck>/<n>.png` name PNGs, unless a real file of that name is there;
    /// anything else in the served folder is sent as it is.
    pub fn path(&self, path: &str, query: Option<&str>) -> Answer {
        let pairs = || query.into_iter().flat_map(|query| query.split('&'));
        let print = pairs().any(|pair| pair == "print");
        // A design's theme, as `?theme=` names it.
        let theme = pairs()
            .find_map(|pair| pair.strip_prefix("theme="))
            .map(|name| Theme::named(name).ok_or(name));
        let entry = self.folder.entry();
        let file_at = |path: &str| entry.served.file_at(path);
        let outside = || Answer::NotFound(format!("{path} is not part of the served folder"));
        let real = file_at(path);

        if real.is_none()
            && let Some(design) = path.strip_suffix(".png").filter(|p| p.ends_with(".design"))
        {
            // A design that isn't a file in the served folder is like any
            // other path that isn't: 404, never its problems.
            if file_at(design).is_none() {
                return outside();
            }
            return match theme {
                None => Answer::DesignPng {
                    design: design.to_string(),
                    theme: Theme::Light,
                },
                Some(Ok(theme)) => Answer::DesignPng {
                    design: design.to_string(),
                    theme,
                },
                Some(Err(other)) => {
                    Answer::BadRequest(format!("theme: expected light or dark, found \"{other}\""))
                }
            };
        }
        if real.is_none()
            && let Some((deck, n)) = path.strip_suffix(".png").and_then(|p| p.rsplit_once('/'))
            && let Some(file) = file_at(deck).filter(|file| {
                file.extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
            })
        {
            return self.slide(deck, &file, n);
        }
        let Some(file) = real else {
            return outside();
        };
        if path.ends_with(".design") {
            // An address that names a theme shows it; any other `theme` is
            // passed over.
            return Answer::Html(page::canvas(
                self.folder.served(),
                path,
                theme.and_then(Result::ok),
            ));
        }
        let served = &entry.served;
        if document::kind_of(&file).is_some() {
            return match Document::load(&file, served) {
                Ok(document) => self.show(&document, print),
                Err(error) => page::invalid(&entry, &file, &format!("{error:#}"))
                    .map_or_else(outside, Answer::Html),
            };
        }
        if file.extension().is_none_or(|ext| ext != "md") {
            return Answer::File(file);
        }
        let source = match std::fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => return Answer::Failed(error.to_string()),
        };
        // A site is never printed: in print mode a Markdown file is a doc of
        // one chapter, whatever site holds it — what `devpage pdf` prints. A
        // page of a site whose file is invalid shows why, until it is fixed.
        match self.folder.owner(&file) {
            Some(Owner {
                document: Ok(site), ..
            }) if !print => {
                // Rendered once: the preview judges the content the page shows.
                let widgets = Widgets::of(&site, self.modules);
                let rendered = page::content(&widgets, &file, source);
                let found = self.preview.problems(
                    &site,
                    &widgets,
                    self.designs,
                    std::slice::from_ref(&rendered),
                );
                page::page(&site, &widgets, &rendered, self.comments, &found)
                    .map_or_else(outside, Answer::Html)
            }
            Some(Owner {
                file: site,
                document: Err(error),
            }) if !print => page::invalid(&entry, &site, &format!("{error:#}"))
                .map_or_else(outside, Answer::Html),
            owner => match Document::load(&file, served) {
                Ok(mut doc) => {
                    // A page keeps its site's widgets when printed on its own.
                    if let Some(Owner {
                        document: Ok(site), ..
                    }) = owner
                    {
                        doc.widgets = site.widgets;
                    }
                    self.show(&doc, print)
                }
                Err(error) => Answer::Failed(format!("{error:#}")),
            },
        }
    }

    /// `/__devpage/system-widgets/<name>`: a system widget, compiled from
    /// the TSX built into the binary.
    pub fn system_widget(&self, name: &str, query: Option<&str>) -> Answer {
        module(
            widgets::system_module(self.modules, name, version(query)),
            || format!("{name} is not a system widget"),
        )
    }

    /// `/__devpage/widgets/<path>`: a project widget or a helper it imports,
    /// compiled from the served folder.
    pub fn widget(&self, path: &str, query: Option<&str>) -> Answer {
        module(
            widgets::project_module(self.folder, self.modules, path, version(query)),
            || format!("{path} is not part of the served folder"),
        )
    }

    /// `<deck>/<n>.png`: slide `n` of the deck at `file` — or why there is
    /// none: the file isn't a deck, or has no slide `n`. One address per
    /// slide: `2`, never `02`.
    fn slide(&self, deck: &str, file: &std::path::Path, n: &str) -> Answer {
        let document = match Document::load(file, self.folder.served()) {
            Ok(document) if matches!(document.kind, Kind::Slides { .. }) => document,
            _ => return Answer::NotFound(format!("{deck} is not a deck")),
        };
        let Kind::Slides { slides, .. } = &document.kind else {
            unreachable!()
        };
        let count = slides.len();
        match n
            .parse::<usize>()
            .ok()
            .filter(|i| (1..=count).contains(i) && i.to_string() == n)
        {
            Some(n) => Answer::SlidePng {
                page: document.entry(),
                n,
                deck: document.path(),
            },
            None => Answer::NotFound(format!(
                "{deck} has {count} slide{}",
                if count == 1 { "" } else { "s" }
            )),
        }
    }

    /// A document as its kind: a site opens on its root page. Printed, a
    /// page lists no problems, so none are looked for.
    /// Each page's content is rendered once, for the preview to judge and
    /// the frame to show.
    fn show(&self, document: &Document, print: bool) -> Answer {
        let widgets = Widgets::of(document, self.modules);
        let found = |pages: &[page::Rendered]| {
            if print {
                Vec::new()
            } else {
                self.preview
                    .problems(document, &widgets, self.designs, pages)
            }
        };
        match &document.kind {
            Kind::Site { .. } | Kind::Design => self.redirect(document.entry()),
            Kind::Doc { chapters, pdf } => {
                let chapters: Vec<_> = chapters
                    .iter()
                    .map(|chapter| page::read(&widgets, chapter))
                    .collect();
                Answer::Html(page::doc(
                    document,
                    &widgets,
                    &chapters,
                    pdf,
                    print,
                    self.comments,
                    &found(&chapters),
                ))
            }
            Kind::Slides { slides, .. } => {
                let slides: Vec<_> = slides
                    .iter()
                    .map(|slide| page::read(&widgets, &slide.file))
                    .collect();
                Answer::Html(page::slides(
                    document,
                    &widgets,
                    &slides,
                    print,
                    self.comments,
                    &found(&slides),
                ))
            }
        }
    }

    /// A redirect to `address`, an address of the served folder's — under
    /// its mount already.
    fn redirect(&self, address: String) -> Answer {
        Answer::Redirect(address)
    }
}

/// The `v` a module's address was asked with, if any.
fn version(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| pair.strip_prefix("v="))
}

/// A module request's answer: the module, a module that throws why it
/// can't be had, or — when there is none — 404 with `missing`.
fn module(requested: Option<Result<Compiled, String>>, missing: impl FnOnce() -> String) -> Answer {
    match requested {
        Some(Ok(Compiled { js, current })) => Answer::Module {
            js,
            immutable: current,
        },
        Some(Err(why)) => Answer::ModuleError(format!(
            "throw new Error({});",
            serde_json::to_string(&why).unwrap_or_default()
        )),
        None => Answer::NotFound(missing()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A served folder with a site, a deck, a loose page, an invalid doc and
    /// a YAML file that isn't a document — and a secret beside it, outside.
    fn served(name: &str) -> PathBuf {
        let top =
            std::env::temp_dir().join(format!("devpage-answer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let dir = top.join("served");
        std::fs::create_dir_all(dir.join("docs/guide")).unwrap();
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        std::fs::write(top.join("secret.md"), "# Secret\n").unwrap();
        std::fs::write(dir.join("docs/README.md"), "# Home\n").unwrap();
        std::fs::write(dir.join("docs/guide/step.md"), "# Step\n").unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes/one.md"), "# One\n").unwrap();
        std::fs::write(dir.join("notes/loose.md"), "# Loose\n").unwrap();
        std::fs::write(
            dir.join("notes/broken.yml"),
            "kind: doc\ntitle: B\nchapters: [gone.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes/ci.yml"), "on: push\n").unwrap();
        std::fs::write(dir.join("notes/screen.design"), "").unwrap();
        dir.canonicalize().unwrap()
    }

    /// The documents of `dir` with `entry` as the entry document: what
    /// answers are made from.
    struct Served {
        folder: Folder,
        modules: Modules,
        designs: Designs,
        preview: std::sync::Arc<Preview>,
    }

    impl Served {
        fn new(dir: &std::path::Path, entry: &str) -> Self {
            Self::mounted(dir, entry, "")
        }

        /// The same, its served folder under `mount`.
        fn mounted(dir: &std::path::Path, entry: &str, mount: &str) -> Self {
            let served = crate::folder::ServedFolder::new(dir)
                .unwrap()
                .mounted(mount);
            Self {
                folder: Folder::new(std::sync::Arc::new(
                    Document::load(&dir.join(entry), &served).unwrap(),
                )),
                modules: Modules::default(),
                designs: Designs::new(&served),
                preview: crate::check::testing::preview(),
            }
        }

        fn at(&self) -> Answers<'_> {
            Answers::new(
                &self.folder,
                &self.modules,
                &self.designs,
                &self.preview,
                true,
            )
        }
    }

    fn html(answer: Answer) -> String {
        match answer {
            Answer::Html(html) => html,
            other => panic!("not a page: {other:?}"),
        }
    }

    #[test]
    fn answers_a_folder_without_git_by_what_each_file_is() {
        let dir = served("kinds");
        assert!(!dir.join(".git").exists());
        let served = Served::new(&dir, "docs");
        let answers = served.at();

        assert_eq!(answers.home(), Answer::Redirect("/docs/README.md".into()));
        let page = html(answers.path("docs/guide/step.md", None));
        assert!(
            page.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "a page of its nearest site"
        );
        assert!(page.contains("Step · Docs"), "{page}");

        let deck = html(answers.path("notes/talk.yml", None));
        assert!(deck.contains("Talk"), "a document file opens as its kind");
        assert!(!deck.contains("<nav id=\"devpage-nav\" data-sidebar="));

        let loose = html(answers.path("notes/loose.md", None));
        assert!(
            !loose.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "in no site: a doc of one chapter"
        );
        assert!(loose.contains("Loose"));

        let broken = html(answers.path("notes/broken.yml", None));
        assert!(broken.contains("not a valid document"), "{broken}");
        assert!(
            broken.contains("chapter gone.md does not exist"),
            "{broken}"
        );

        assert_eq!(
            answers.path("docs/devpage.yml", None),
            Answer::Redirect("/docs/README.md".into()),
            "a site opens on its root page"
        );
        assert_eq!(
            answers.path("notes/ci.yml", None),
            Answer::File(dir.join("notes/ci.yml"))
        );
        let canvas = html(answers.path("notes/screen.design", None));
        assert!(
            canvas.contains(r#"<canvas class="design" src="/notes/screen.design""#),
            "a design opens on its canvas page: {canvas}"
        );
    }

    #[test]
    fn a_page_of_a_site_prints_as_a_doc_of_one_chapter() {
        let dir = served("print");
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        let page = html(answers.path("docs/guide/step.md", Some("print")));
        assert!(
            page.contains("<body class=\"doc print\" data-build="),
            "{page}"
        );
        assert!(page.contains("@page { size: A4;"), "{page}");
        assert!(
            !page.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "{page}"
        );
        let on_screen = html(answers.path("docs/guide/step.md", Some("x=1&y")));
        assert!(
            on_screen.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "still a site page on screen"
        );
    }

    #[test]
    fn a_sites_title_links_to_its_own_root_page() {
        let dir = served("titles");
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("other/index.md"), "# Index\n").unwrap();
        std::fs::write(dir.join("other/page.md"), "# Page\n").unwrap();
        std::fs::write(
            dir.join("other/devpage.yml"),
            "kind: site\ntitle: Other\nroot: index.md\n",
        )
        .unwrap();
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        assert!(
            html(answers.path("other/page.md", None))
                .contains("<a class=\"document-title\" href=\"/other/index.md\">Other</a>")
        );
        assert!(
            html(answers.path("docs/guide/step.md", None))
                .contains("<a class=\"document-title\" href=\"/docs/README.md\">Docs</a>")
        );
    }

    #[test]
    fn nothing_outside_the_served_folder_is_answered() {
        let dir = served("outside");
        std::os::unix::fs::symlink(dir.join("../secret.md"), dir.join("docs/escape.md")).unwrap();
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        for path in [
            "../secret.md",
            "docs/escape.md",
            "docs/missing.md",
            "../secret.md.design.png",
            "missing.design.png",
            "../secret.md/1.png",
        ] {
            assert!(
                matches!(answers.path(path, None), Answer::NotFound(_)),
                "{path}"
            );
        }
    }

    #[test]
    fn answers_a_folder_reached_through_a_symbolic_link() {
        let dir = served("symlinked");
        let link = dir.parent().unwrap().join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        let link = link.canonicalize().unwrap();
        let served = Served::new(&link, "docs");
        let answers = served.at();
        assert!(html(answers.path("docs/guide/step.md", None)).contains("Step"));
        assert!(matches!(
            answers.path("notes/ci.yml", None),
            Answer::File(_)
        ));
    }

    #[test]
    fn a_slide_png_names_a_slide_of_a_deck_or_why_there_is_none() {
        let dir = served("slide-png");
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        assert_eq!(
            answers.path("notes/talk.yml/1.png", None),
            Answer::SlidePng {
                page: "/notes/talk.yml".into(),
                n: 1,
                deck: dir.join("notes/talk.yml"),
            }
        );
        for (path, reason) in [
            ("notes/talk.yml/2.png", "notes/talk.yml has 1 slide"),
            ("notes/talk.yml/0.png", "notes/talk.yml has 1 slide"),
            ("notes/talk.yml/01.png", "notes/talk.yml has 1 slide"),
            ("notes/talk.yml/one.png", "notes/talk.yml has 1 slide"),
            ("docs/devpage.yml/1.png", "docs/devpage.yml is not a deck"),
            ("notes/ci.yml/1.png", "notes/ci.yml is not a deck"),
            ("notes/broken.yml/1.png", "notes/broken.yml is not a deck"),
            ("notes/gone.yml/1.png", "is not part of the served folder"),
        ] {
            match answers.path(path, None) {
                Answer::NotFound(why) => assert!(why.contains(reason), "{path}: {why}"),
                other => panic!("{path}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_design_png_takes_its_theme_from_the_address() {
        let dir = served("design-png");
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        let png = |query| answers.path("notes/screen.design.png", query);
        assert_eq!(
            png(None),
            Answer::DesignPng {
                design: "notes/screen.design".into(),
                theme: Theme::Light
            }
        );
        assert_eq!(
            png(Some("theme=dark")),
            Answer::DesignPng {
                design: "notes/screen.design".into(),
                theme: Theme::Dark
            }
        );
        assert_eq!(
            png(Some("theme=blue")),
            Answer::BadRequest("theme: expected light or dark, found \"blue\"".into())
        );
        // A real file of that name is sent as it is.
        std::fs::write(dir.join("notes/screen.design.png"), "png").unwrap();
        assert!(matches!(png(None), Answer::File(_)));
    }

    #[test]
    fn every_address_it_answers_with_carries_the_mount_prefix() {
        let dir = served("mount");
        let served = Served::mounted(&dir, "docs", "/d/6f1c/v/3");
        let answers = served.at();
        assert_eq!(
            answers.home(),
            Answer::Redirect("/d/6f1c/v/3/docs/README.md".into())
        );
        assert_eq!(
            answers.path("docs/devpage.yml", None),
            Answer::Redirect("/d/6f1c/v/3/docs/README.md".into())
        );
        let canvas = html(answers.path("notes/screen.design", None));
        assert!(
            canvas.contains(r#"src="/d/6f1c/v/3/notes/screen.design""#),
            "{canvas}"
        );
        // Unmounted, the same answers are today's addresses.
        assert_eq!(
            Served::new(&dir, "docs").at().home(),
            Answer::Redirect("/docs/README.md".into())
        );
    }

    #[test]
    fn an_edited_gitignored_served_doc_is_served_fresh() {
        let dir = served("ignored-fresh");
        std::fs::write(dir.join(".gitignore"), "/drafts/\n").unwrap();
        std::fs::create_dir_all(dir.join("drafts")).unwrap();
        std::fs::write(
            dir.join("drafts/plan.yml"),
            "kind: doc\ntitle: Plan\nchapters: [a.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("drafts/a.md"), "# First draft\n").unwrap();
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        let page = html(answers.path("drafts/plan.yml", None));
        assert!(page.contains("First draft"), "{page}");

        // The watcher never hears of a gitignored file, and nothing has to:
        // every request reads the document as its files say now.
        std::fs::write(
            dir.join("drafts/plan.yml"),
            "kind: doc\ntitle: Renamed plan\nchapters: [a.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("drafts/a.md"), "# Second draft\n").unwrap();
        let page = html(answers.path("drafts/plan.yml", None));
        assert!(page.contains("Renamed plan"), "{page}");
        assert!(page.contains("Second draft"), "{page}");
        assert!(!page.contains("First draft"), "{page}");
    }

    #[test]
    fn a_design_opens_on_its_canvas_page_in_the_theme_its_address_names() {
        let dir = served("canvas-theme");
        let served = Served::new(&dir, "docs");
        let answers = served.at();
        let page = |query| html(answers.path("notes/screen.design", query));
        let chosen = r#"<script>document.documentElement.dataset.theme="dark"</script>"#;
        assert!(page(Some("theme=dark")).contains(chosen));
        assert!(!page(None).contains("dataset.theme=\"dark\""));
        assert!(!page(Some("theme=blue")).contains("dataset.theme=\"blue\""));
        assert!(page(None).contains("<title>screen</title>"));
    }

    /// The module `answer` is, and whether it is immutable.
    fn module(answer: Answer) -> (String, bool) {
        match answer {
            Answer::Module { js, immutable } => (js, immutable),
            other => panic!("not a module: {other:?}"),
        }
    }

    /// The module that throws, that `answer` is.
    fn thrown(answer: Answer) -> String {
        match answer {
            Answer::ModuleError(js) => js,
            other => panic!("not a module that throws: {other:?}"),
        }
    }

    #[test]
    fn widgets_are_answered_compiled_for_the_document_they_belong_to() {
        let dir = served("widget-modules");
        std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
        std::fs::create_dir_all(dir.join("notes/parts")).unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/Card.tsx"),
            "import { label } from \"./label\";\nexport default () => <Callout>{label}</Callout>;",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/label.ts"),
            "export const label = 'x';",
        )
        .unwrap();
        std::fs::write(dir.join("docs/widgets/Broken.tsx"), "export default (").unwrap();
        // Another document's widgets compose with that document's names.
        std::fs::write(
            dir.join("notes/talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\nwidgets: parts\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/parts/Pill.tsx"),
            "export default () => <i>p</i>;",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes/parts/Row.tsx"),
            "export default () => <Pill />;",
        )
        .unwrap();
        let served = Served::new(&dir, "docs");
        let answers = served.at();

        // A system widget: immutable at its current address only.
        let tabs = widgets::system_address("Tabs", "");
        let (path, query) = tabs.split_once('?').unwrap();
        let name = path.rsplit('/').next().unwrap();
        let (system, immutable) = module(answers.system_widget(name, Some(query)));
        assert!(system.contains("export const __devpageUses"), "{system}");
        assert!(immutable);
        assert_eq!(module(answers.system_widget(name, None)), (system, false));
        assert_eq!(
            answers.system_widget("Nope.tsx", None),
            Answer::NotFound("Nope.tsx is not a system widget".into())
        );

        // The address a page names a project widget by is the one it is
        // immutable at; the helper it imports is named by its own address.
        let page = html(answers.path("docs/README.md", None));
        let card = page
            .split("\"Card\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_string();
        let asked = |address: &str| {
            let (path, query) = address.split_once('?').unwrap_or((address, ""));
            let path = path.strip_prefix("/__devpage/widgets/").unwrap();
            answers.widget(path, Some(query).filter(|q| !q.is_empty()))
        };
        let (compiled, immutable) = module(asked(&card));
        assert!(immutable, "{card}");
        let label = Modules::default()
            .snapshot(served.folder.served(), &dir.join("docs/widgets/label.ts"))
            .address(
                &dir.join("docs/widgets/label.ts"),
                Widgets::of(&served.folder.entry(), &Modules::default()).names(),
            );
        assert!(compiled.contains(&format!("\"{label}\"")), "{compiled}");
        assert!(
            compiled.contains("__devpageUses = [\"Callout\"]"),
            "{compiled}"
        );
        let (helper, immutable) = module(asked(&label));
        assert!(immutable && helper.contains("label"), "{helper}");
        // Any other version answers with the current module, never immutable.
        for stale in [
            "/__devpage/widgets/docs/widgets/Card.tsx?v=0123456789abcdef",
            "/__devpage/widgets/docs/widgets/Card.tsx",
        ] {
            assert_eq!(module(asked(stale)), (compiled.clone(), false), "{stale}");
        }
        // An import without an extension finds the `.ts` beside it.
        let (helper, _) = module(answers.widget("docs/widgets/label", None));
        assert!(helper.contains("label"), "{helper}");

        let broken = thrown(answers.widget("docs/widgets/Broken.tsx", None));
        assert!(
            broken.starts_with("throw new Error(\"docs/widgets/Broken.tsx does not compile:"),
            "{broken}"
        );
        let (row, _) = module(answers.widget("notes/parts/Row.tsx", None));
        assert!(row.contains("__devpageUses = [\"Pill\"]"), "{row}");

        for outside in ["../secret.md", "docs/widgets/Gone.tsx"] {
            assert_eq!(
                answers.widget(outside, None),
                Answer::NotFound(format!("{outside} is not part of the served folder"))
            );
        }
    }

    #[test]
    fn a_widget_whose_helper_does_not_compile_throws_naming_the_helper() {
        let dir = served("widget-broken-helper");
        std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
        std::fs::write(
            dir.join("docs/devpage.yml"),
            "kind: site\ntitle: Docs\nroot: README.md\nwidgets: widgets\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/widgets/Card.tsx"),
            "import { frame } from \"./frame\";\nexport default () => <i>{frame}</i>;",
        )
        .unwrap();
        // Reached through a second helper: the error is transitive.
        std::fs::write(
            dir.join("docs/widgets/frame.ts"),
            "export { inner as frame } from \"./inner\";",
        )
        .unwrap();
        std::fs::write(dir.join("docs/widgets/inner.ts"), "export const inner = (").unwrap();
        let served = Served::new(&dir, "docs");
        let thrown = thrown(served.at().widget("docs/widgets/Card.tsx", None));
        assert!(
            thrown.starts_with("throw new Error(\"docs/widgets/inner.ts does not compile:"),
            "{thrown}"
        );
    }
}
