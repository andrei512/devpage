//! Shared fixtures for tests that mount widgets on a served page.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::Page;
use devpage_core::document::Document;
use devpage_core::folder::ServedFolder;

use crate::adapters::chrome::testing::{self, Answered, eval};
use crate::drive::http;

/// A served folder for the widget tests: `docs/` is a site with the given
/// README, and `files` are written beside it.
pub(super) fn widget_folder(name: &str, readme: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(
        dir.join("docs/devpage.yml"),
        "kind: site\ntitle: W\nroot: README.md\nwidgets: widgets\n",
    )
    .unwrap();
    std::fs::write(dir.join("docs/README.md"), readme).unwrap();
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

/// Serves `dir` with `docs` as the entry and opens `path` in headless
/// Chrome, once every widget has mounted.
pub(super) async fn open(dir: &Path, path: &str) -> (testing::Browser, Page) {
    let (browser, page, _) = open_logged(dir, path).await;
    (browser, page)
}

/// [`open`], also keeping every request the server answers.
pub(super) async fn open_logged(dir: &Path, path: &str) -> (testing::Browser, Page, Answered) {
    let document =
        Arc::new(Document::load(&dir.join("docs"), &ServedFolder::new(dir).unwrap()).unwrap());
    let changes = crate::adapters::changes::Changes::default();
    let (router, answered) = testing::answering(http::router(
        http::App::testing(document, changes)
            .with_comments_on_disk(devpage_core::comments::Identity::Local("T".into())),
    ));
    let address = testing::serve(router).await;
    let browser = testing::browser().await;
    let page = testing::open(&browser, &format!("{address}{path}")).await;
    (browser, page, answered)
}

/// `script` evaluated in `page`, as a string.
pub(super) async fn text(page: &Page, script: &str) -> String {
    eval(page, script).await
}

/// What the server answered since `from`, less the event stream.
pub(super) fn since(answered: &Answered, from: usize) -> Vec<(String, u16)> {
    answered.lock().unwrap()[from..]
        .iter()
        .filter(|(path, _)| path != "/__devpage/events" && path != "/favicon.ico")
        .cloned()
        .collect()
}

/// Writes `text` to the widget file `path`, its time moved on past the
/// last write's, so the module cache — which keys a file by its time and
/// length — reads it again however soon after that write it comes.
pub(super) fn rewrite(path: &Path, text: &str) {
    let last = std::fs::metadata(path).unwrap().modified().unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(last + Duration::from_secs(1))
        .unwrap();
}
