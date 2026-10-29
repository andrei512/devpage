//! What a request for a path in the served folder is answered with: a page,
//! a redirect, a file's bytes, a design's or a slide's PNG, or why there is
//! none. Decided here from the documents alone, so every route is tested
//! without a socket; the HTTP server only turns an [`Answer`] into a response
//! and draws the PNGs it names.

use std::path::PathBuf;

use devpage_design::Theme;

use crate::check::Preview;
use crate::document::{self, Document, Kind};
use crate::folder::{Folder, Owner};
use crate::render;
use crate::widgets::{Modules, Widgets};

/// What a path answers with.
#[derive(Debug, PartialEq)]
pub enum Answer {
    /// A rendered page.
    Html(String),
    /// Elsewhere: an address, mount prefix included.
    Redirect(String),
    /// The canvas page of the design at `design`, a path in the served
    /// folder, in `theme` when the address names one.
    Canvas {
        design: String,
        theme: Option<Theme>,
    },
    /// A file of the served folder, sent as it is.
    File(PathBuf),
    /// The PNG of the design at `design`, a path in the served folder.
    DesignPng { design: String, theme: Theme },
    /// Slide `n` of the deck whose page is at `page` — the deck's own
    /// address, without the mount prefix, as Chrome asks the server for it.
    SlidePng { page: String, n: usize },
    /// 404, and why.
    NotFound(String),
    /// 400, and why.
    BadRequest(String),
    /// 500, and why.
    Failed(String),
}

/// What decides an answer: the served folder with its entry document, its
/// widget modules, the problems each page lists and whether comments are on.
/// Every address it answers with is under the served folder's mount, which
/// the documents carry.
pub struct Resolver<'a> {
    pub folder: &'a Folder,
    pub modules: &'a Modules,
    pub preview: &'a Preview,
    pub comments: bool,
}

impl Resolver<'_> {
    /// `/` leads to the entry document as its file says now — or, while the
    /// file is invalid, shows why.
    pub fn home(&self) -> Answer {
        match self.folder.entry_error() {
            Some(error) => Answer::Html(render::invalid(
                &self.folder.entry(),
                self.folder.entry_file(),
                &error,
            )),
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
            return Answer::Canvas {
                design: path.to_string(),
                theme: theme.and_then(Result::ok),
            };
        }
        let served = &entry.served;
        if document::kind_of(&file).is_some() {
            return match Document::load(&file, served) {
                Ok(document) => self.show(&document, print),
                Err(error) => Answer::Html(render::invalid(&entry, &file, &format!("{error:#}"))),
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
                let widgets = Widgets::of(&site, self.modules);
                let found = self
                    .preview
                    .problems(&site, &widgets, std::slice::from_ref(&file));
                Answer::Html(render::page(
                    &site,
                    &widgets,
                    &file,
                    &source,
                    self.comments,
                    &found,
                ))
            }
            Some(Owner {
                file: site,
                document: Err(error),
            }) if !print => Answer::Html(render::invalid(&entry, &site, &format!("{error:#}"))),
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
            },
            None => Answer::NotFound(format!(
                "{deck} has {count} slide{}",
                if count == 1 { "" } else { "s" }
            )),
        }
    }

    /// A document as its kind: a site opens on its root page. Printed, a
    /// page lists no problems, so none are looked for.
    fn show(&self, document: &Document, print: bool) -> Answer {
        let widgets = Widgets::of(document, self.modules);
        let found = |pages: &[PathBuf]| {
            if print {
                Vec::new()
            } else {
                self.preview.problems(document, &widgets, pages)
            }
        };
        match &document.kind {
            Kind::Site { .. } | Kind::Design => self.redirect(document.entry()),
            Kind::Doc { chapters, pdf } => Answer::Html(render::doc(
                document,
                &widgets,
                chapters,
                pdf,
                print,
                self.comments,
                &found(chapters),
            )),
            Kind::Slides { slides, .. } => {
                let files: Vec<PathBuf> = slides.iter().map(|slide| slide.file.clone()).collect();
                Answer::Html(render::slides(
                    document,
                    &widgets,
                    slides,
                    print,
                    self.comments,
                    &found(&files),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A served folder with a site, a deck, a loose page, an invalid doc and
    /// a YAML file that isn't a document — and a secret beside it, outside.
    fn served(name: &str) -> PathBuf {
        let top =
            std::env::temp_dir().join(format!("devpage-resolve-{name}-{}", std::process::id()));
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

    /// The documents of `dir` with `entry` as the entry document: what a
    /// resolver is made from.
    struct Served {
        folder: Folder,
        modules: Modules,
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
                preview: crate::check::testing::preview(),
            }
        }

        fn at(&self) -> Resolver<'_> {
            Resolver {
                folder: &self.folder,
                modules: &self.modules,
                preview: &self.preview,
                comments: true,
            }
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
        let resolve = served.at();

        assert_eq!(resolve.home(), Answer::Redirect("/docs/README.md".into()));
        let page = html(resolve.path("docs/guide/step.md", None));
        assert!(
            page.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "a page of its nearest site"
        );
        assert!(page.contains("Step · Docs"), "{page}");

        let deck = html(resolve.path("notes/talk.yml", None));
        assert!(deck.contains("Talk"), "a document file opens as its kind");
        assert!(!deck.contains("<nav id=\"devpage-nav\" data-sidebar="));

        let loose = html(resolve.path("notes/loose.md", None));
        assert!(
            !loose.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "in no site: a doc of one chapter"
        );
        assert!(loose.contains("Loose"));

        let broken = html(resolve.path("notes/broken.yml", None));
        assert!(broken.contains("not a valid document"), "{broken}");
        assert!(
            broken.contains("chapter gone.md does not exist"),
            "{broken}"
        );

        assert_eq!(
            resolve.path("docs/devpage.yml", None),
            Answer::Redirect("/docs/README.md".into()),
            "a site opens on its root page"
        );
        assert_eq!(
            resolve.path("notes/ci.yml", None),
            Answer::File(dir.join("notes/ci.yml"))
        );
        assert_eq!(
            resolve.path("notes/screen.design", Some("theme=dark")),
            Answer::Canvas {
                design: "notes/screen.design".into(),
                theme: Some(Theme::Dark)
            }
        );
    }

    #[test]
    fn a_page_of_a_site_prints_as_a_doc_of_one_chapter() {
        let dir = served("print");
        let served = Served::new(&dir, "docs");
        let resolve = served.at();
        let page = html(resolve.path("docs/guide/step.md", Some("print")));
        assert!(
            page.contains("<body class=\"doc print\" data-build="),
            "{page}"
        );
        assert!(page.contains("@page { size: A4;"), "{page}");
        assert!(
            !page.contains("<nav id=\"devpage-nav\" data-sidebar="),
            "{page}"
        );
        let on_screen = html(resolve.path("docs/guide/step.md", Some("x=1&y")));
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
        let resolve = served.at();
        assert!(
            html(resolve.path("other/page.md", None))
                .contains("<a class=\"document-title\" href=\"/other/index.md\">Other</a>")
        );
        assert!(
            html(resolve.path("docs/guide/step.md", None))
                .contains("<a class=\"document-title\" href=\"/docs/README.md\">Docs</a>")
        );
    }

    #[test]
    fn nothing_outside_the_served_folder_is_answered() {
        let dir = served("outside");
        std::os::unix::fs::symlink(dir.join("../secret.md"), dir.join("docs/escape.md")).unwrap();
        let served = Served::new(&dir, "docs");
        let resolve = served.at();
        for path in [
            "../secret.md",
            "docs/escape.md",
            "docs/missing.md",
            "../secret.md.design.png",
            "missing.design.png",
            "../secret.md/1.png",
        ] {
            assert!(
                matches!(resolve.path(path, None), Answer::NotFound(_)),
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
        let resolve = served.at();
        assert!(html(resolve.path("docs/guide/step.md", None)).contains("Step"));
        assert!(matches!(
            resolve.path("notes/ci.yml", None),
            Answer::File(_)
        ));
    }

    #[test]
    fn a_slide_png_names_a_slide_of_a_deck_or_why_there_is_none() {
        let dir = served("slide-png");
        let served = Served::new(&dir, "docs");
        let resolve = served.at();
        assert_eq!(
            resolve.path("notes/talk.yml/1.png", None),
            Answer::SlidePng {
                page: "/notes/talk.yml".into(),
                n: 1
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
            match resolve.path(path, None) {
                Answer::NotFound(why) => assert!(why.contains(reason), "{path}: {why}"),
                other => panic!("{path}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_design_png_takes_its_theme_from_the_address() {
        let dir = served("design-png");
        let served = Served::new(&dir, "docs");
        let resolve = served.at();
        let png = |query| resolve.path("notes/screen.design.png", query);
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
        let resolve = Served::mounted(&dir, "docs", "/d/6f1c/v/3");
        let resolve = resolve.at();
        assert_eq!(
            resolve.home(),
            Answer::Redirect("/d/6f1c/v/3/docs/README.md".into())
        );
        assert_eq!(
            resolve.path("docs/devpage.yml", None),
            Answer::Redirect("/d/6f1c/v/3/docs/README.md".into())
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
        let resolve = served.at();
        let page = html(resolve.path("drafts/plan.yml", None));
        assert!(page.contains("First draft"), "{page}");

        // The watcher never hears of a gitignored file, and nothing has to:
        // every request reads the document as its files say now.
        std::fs::write(
            dir.join("drafts/plan.yml"),
            "kind: doc\ntitle: Renamed plan\nchapters: [a.md]\n",
        )
        .unwrap();
        std::fs::write(dir.join("drafts/a.md"), "# Second draft\n").unwrap();
        let page = html(resolve.path("drafts/plan.yml", None));
        assert!(page.contains("Renamed plan"), "{page}");
        assert!(page.contains("Second draft"), "{page}");
        assert!(!page.contains("First draft"), "{page}");
    }
}
