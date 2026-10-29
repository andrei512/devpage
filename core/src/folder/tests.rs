//! The folder's rules, tested without a watcher: what the document index
//! holds, which site owns a page, what can matter when a path changes, and
//! what open pages hear. The watcher's sift and refresh tests, ported here so
//! the rules are pinned on every platform.

use super::*;

/// A served folder `dir` with an outer site, an inner site, a deck, pages, a
/// YAML file that isn't devpage's, an ignored `target/` holding a document
/// file, and a nested `.gitignore`.
fn indexed(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-folder-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("guides/deep")).unwrap();
    std::fs::create_dir_all(dir.join("target/doc")).unwrap();
    std::fs::write(dir.join("README.md"), "# Home").unwrap();
    std::fs::write(
        dir.join("site.yml"),
        "kind: site\ntitle: Outer\nroot: README.md\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("guides/inner.yml"),
        "kind: site\ntitle: Inner\nroot: a.md\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("guides/talk.yml"),
        "kind: slides\ntitle: T\nslides: [a.md]\n",
    )
    .unwrap();
    std::fs::write(dir.join("guides/a.md"), "# A").unwrap();
    std::fs::write(dir.join("guides/deep/b.md"), "# B").unwrap();
    std::fs::write(dir.join("deploy.yaml"), "kind: Deployment\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
    std::fs::write(
        dir.join("target/doc/devpage.yml"),
        "kind: site\ntitle: X\nroot: a\n",
    )
    .unwrap();
    std::fs::write(dir.join("guides/.gitignore"), "*.tmp\n").unwrap();
    dir.canonicalize().unwrap()
}

/// The folder at `dir`, opened on `entry` in it.
fn open(dir: &Path, entry: &str) -> Folder {
    let served = ServedFolder::new(dir).unwrap();
    Folder::new(Arc::new(Document::load(&dir.join(entry), &served).unwrap()))
}

#[test]
fn indexes_document_files_outside_what_gitignore_excludes() {
    let dir = indexed("build");
    let folder = open(&dir, "site.yml");
    assert_eq!(folder.kind(&dir.join("site.yml")), Some(KindName::Site));
    assert_eq!(
        folder.kind(&dir.join("guides/talk.yml")),
        Some(KindName::Slides)
    );
    assert_eq!(
        folder.kind(&dir.join("deploy.yaml")),
        None,
        "not devpage's kind"
    );
    assert_eq!(
        folder.kind(&dir.join("target/doc/devpage.yml")),
        None,
        "ignored"
    );
}

#[test]
fn a_page_is_owned_by_the_nearest_site_above_it() {
    let dir = indexed("nearest");
    let folder = open(&dir, "site.yml");
    let owner = folder.owner(&dir.join("guides/deep/b.md")).unwrap();
    assert_eq!(owner.file, dir.join("guides/inner.yml"));
    assert_eq!(owner.document.unwrap().title, "Inner");
    let owner = folder.owner(&dir.join("README.md")).unwrap();
    assert_eq!(owner.file, dir.join("site.yml"));
}

#[test]
fn an_owner_is_read_as_its_file_says_now() {
    let dir = indexed("owner-now");
    let folder = open(&dir, "site.yml");
    std::fs::write(
        dir.join("guides/inner.yml"),
        "kind: site\ntitle: Renamed\nroot: a.md\n",
    )
    .unwrap();
    let owner = folder.owner(&dir.join("guides/a.md")).unwrap();
    assert_eq!(owner.document.unwrap().title, "Renamed", "nothing cached");
    std::fs::write(dir.join("guides/inner.yml"), "kind: site\ntitle: Broken\n").unwrap();
    let owner = folder.owner(&dir.join("guides/a.md")).unwrap();
    assert_eq!(owner.file, dir.join("guides/inner.yml"));
    assert!(
        owner.document.is_err(),
        "an invalid site is still the owner"
    );
}

#[test]
fn nested_gitignores_and_git_folders_are_out_of_sight() {
    let dir = indexed("ignored");
    let folder = open(&dir, "site.yml");
    assert!(folder.ignored(&dir.join("target/doc/x.md"), false));
    assert!(
        folder.ignored(&dir.join("guides/notes.tmp"), false),
        "nested .gitignore"
    );
    assert!(
        !folder.ignored(&dir.join("notes.tmp"), false),
        "only below guides/"
    );
    assert!(folder.ignored(&dir.join(".git/HEAD"), false));
    assert!(folder.ignored(&dir.join(".devpage/comments/a.md/x.yml"), false));
    assert!(!folder.ignored(&dir.join("guides/a.md"), false));
}

#[test]
fn changes_keep_it_current() {
    let dir = indexed("changes");
    let folder = open(&dir, "site.yml");
    let deck = dir.join("guides/deep/deck.yml");
    std::fs::write(&deck, "kind: slides\ntitle: D\nslides: [b.md]\n").unwrap();
    folder.changed(std::slice::from_ref(&deck));
    assert_eq!(folder.kind(&deck), Some(KindName::Slides));
    let site = dir.join("guides/deep/site.yml");
    std::fs::write(&site, "kind: site\ntitle: S\nroot: b.md\n").unwrap();
    assert!(folder.changed(std::slice::from_ref(&site)).reshaped);
    assert_eq!(
        folder.owner(&dir.join("guides/deep/b.md")).unwrap().file,
        site
    );
    std::fs::remove_file(&deck).unwrap();
    assert!(folder.changed(std::slice::from_ref(&deck)).reshaped);
    assert_eq!(folder.kind(&deck), None);
    std::fs::write(dir.join(".gitignore"), "/target/\n/guides/deep/\n").unwrap();
    assert!(folder.changed(&[dir.join(".gitignore")]).reshaped);
    assert!(folder.ignored(&dir.join("guides/deep/b.md"), false));
    assert_eq!(
        folder.owner(&dir.join("guides/deep/b.md")).unwrap().file,
        dir.join("guides/inner.yml")
    );
}

#[test]
fn widgets_belong_to_the_document_naming_their_folder() {
    let dir = indexed("widgets");
    std::fs::create_dir_all(dir.join("w")).unwrap();
    std::fs::create_dir_all(dir.join("guides/w")).unwrap();
    std::fs::write(
        dir.join("site.yml"),
        "kind: site\ntitle: Outer\nroot: README.md\nwidgets: w\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("guides/talk.yml"),
        "kind: slides\ntitle: T\nslides: [a.md]\nwidgets: w\n",
    )
    .unwrap();
    let folder = open(&dir, "site.yml");
    assert_eq!(folder.widgets_owner(&dir.join("w")).unwrap().title, "Outer");
    assert_eq!(
        folder.widgets_owner(&dir.join("guides/w")).unwrap().title,
        "T"
    );
    assert!(folder.widgets_owner(&dir.join("guides")).is_none());
}

/// A served folder with a site in `docs/` whose document file is `site`, and
/// `docs/build/` ignored.
fn served(name: &str, site: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-folder-w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/build")).unwrap();
    std::fs::write(dir.join("docs/README.md"), "# Home").unwrap();
    std::fs::write(dir.join("docs/devpage.yml"), site).unwrap();
    std::fs::write(dir.join("docs/.gitignore"), "build/\n").unwrap();
    dir.canonicalize().unwrap()
}

fn refresh() -> Vec<Change> {
    vec![Change::Refresh]
}

#[test]
fn build_output_git_and_ignored_folders_never_matter() {
    let dir = served("sift", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::write(dir.join(".gitignore"), "/target/\n").unwrap();
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    let folder = open(&dir, "docs");
    for noise in [
        dir.join("target/debug/devpage"),
        dir.join("target/debug"),
        dir.join(".git/objects/ab"),
        dir.join(".git/index"),
        dir.join("docs/build/out.md"),
        dir.join("docs/build/screen.design"),
        dir.join(".devpage/screen.design"),
    ] {
        assert!(!folder.matters(&noise, false), "{}", noise.display());
    }
    assert!(!folder.matters(&dir.join("docs/build"), true));
    assert_eq!(
        folder.changed(&[dir.join("docs/build/out.md")]).changes,
        [],
        "out of sight refreshes nothing"
    );
}

#[test]
fn comment_threads_and_files_watch_names_still_matter() {
    let dir = served(
        "named",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\", \"build/*.svg\"]\n",
    );
    let folder = open(&dir, "docs");
    let thread = dir.join(".devpage/comments/docs/README.md/20261025T000000Z-abcd.yml");
    let named = dir.join("docs/build/chart.svg");
    let unnamed = dir.join("docs/build/chart.png");
    let page = dir.join("docs/README.md");
    assert!(folder.matters(&thread, false));
    assert!(folder.matters(&named, false));
    assert!(!folder.matters(&unnamed, false));
    assert!(folder.matters(&page, false));
    assert!(
        folder.matters(&dir.parent().unwrap().join("elsewhere"), false),
        "judged once canonical"
    );
    assert_eq!(folder.changed(&[thread]).changes, [], "not the folder's");
    assert_eq!(folder.changed(&[named, page]).changes, refresh());
}

#[test]
fn a_new_gitignore_rule_takes_effect_without_a_restart() {
    let dir = served("rule", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::create_dir_all(dir.join("docs/drafts")).unwrap();
    let folder = open(&dir, "docs");
    let draft = dir.join("docs/drafts/idea.md");
    assert!(folder.matters(&draft, false));
    std::fs::write(dir.join("docs/.gitignore"), "build/\ndrafts/\n").unwrap();
    assert!(folder.changed(&[dir.join("docs/.gitignore")]).reshaped);
    assert!(!folder.matters(&draft, false));
}

#[test]
fn ignored_files_and_files_outside_watch_do_not_refresh() {
    let dir = served(
        "quiet",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
    );
    let folder = open(&dir, "docs");
    std::fs::write(dir.join("docs/build/out.md"), "x").unwrap();
    std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap();
    let quiet = folder.changed(&[dir.join("docs/build/out.md"), dir.join("docs/logo.svg")]);
    assert_eq!(quiet, Changed::default());
    std::fs::write(dir.join("docs/README.md"), "# Changed").unwrap();
    assert_eq!(
        folder.changed(&[dir.join("docs/README.md")]).changes,
        refresh()
    );
}

#[test]
fn the_watch_list_follows_the_entry_file() {
    let dir = served(
        "relist",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
    );
    let folder = open(&dir, "docs");
    let file = dir.join("docs/devpage.yml");
    // Its own file changing refreshes pages, though `watch` names only Markdown.
    std::fs::write(
        &file,
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.svg\"]\n",
    )
    .unwrap();
    let heard = folder.changed(std::slice::from_ref(&file));
    assert_eq!(heard.changes, refresh());
    assert!(heard.reshaped);
    // Now Markdown is outside `watch`, and an SVG is in it.
    assert_eq!(folder.changed(&[dir.join("docs/README.md")]).changes, []);
    std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap();
    assert_eq!(
        folder.changed(&[dir.join("docs/logo.svg")]).changes,
        refresh()
    );
}

#[test]
fn an_invalid_entry_keeps_its_last_valid_document_and_says_why() {
    let dir = served(
        "invalid",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"**/*.md\"]\n",
    );
    let folder = open(&dir, "docs");
    let file = dir.join("docs/devpage.yml");
    std::fs::write(&file, "kind: site\ntitle: T\n").unwrap();
    assert_eq!(
        folder.changed(std::slice::from_ref(&file)).changes,
        refresh()
    );
    assert_eq!(folder.entry().title, "T");
    assert!(folder.entry_error().unwrap().contains("root"));
    // Its last valid `watch` list still judges.
    assert_eq!(
        folder.changed(&[dir.join("docs/README.md")]).changes,
        refresh()
    );
    std::fs::write(dir.join("docs/logo.svg"), "<svg/>").unwrap();
    assert_eq!(folder.changed(&[dir.join("docs/logo.svg")]).changes, []);
    std::fs::write(&file, "kind: site\ntitle: Fixed\nroot: README.md\n").unwrap();
    folder.changed(std::slice::from_ref(&file));
    assert_eq!(folder.entry().title, "Fixed");
    assert_eq!(folder.entry_error(), None);
}

#[test]
fn a_sites_watch_list_narrows_only_its_own_folder() {
    // The entry site has no `watch`; a second site watches only text files.
    let dir = served("other", "kind: site\ntitle: T\nroot: README.md\n");
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::create_dir_all(dir.join("plan")).unwrap();
    std::fs::write(dir.join("other/index.md"), "# Other").unwrap();
    std::fs::write(
        dir.join("other/devpage.yml"),
        "kind: site\ntitle: O\nroot: index.md\nwatch: [\"*.txt\"]\n",
    )
    .unwrap();
    let folder = open(&dir, "docs");
    assert_eq!(folder.changed(&[dir.join("other/index.md")]).changes, []);
    std::fs::write(dir.join("other/notes.txt"), "x").unwrap();
    assert_eq!(
        folder.changed(&[dir.join("other/notes.txt")]).changes,
        refresh()
    );
    // The entry's own pages still refresh, and so does a doc in no site.
    assert_eq!(
        folder.changed(&[dir.join("docs/README.md")]).changes,
        refresh()
    );
    std::fs::write(dir.join("plan/design.md"), "# Plan").unwrap();
    assert_eq!(
        folder.changed(&[dir.join("plan/design.md")]).changes,
        refresh()
    );
}

#[test]
fn the_entrys_watch_list_does_not_quiet_another_folder() {
    let dir = served(
        "entry-watch",
        "kind: site\ntitle: T\nroot: README.md\nwatch: [\"*.svg\"]\n",
    );
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    std::fs::write(dir.join("examples/design.md"), "# Design").unwrap();
    let folder = open(&dir, "docs");
    assert_eq!(
        folder.changed(&[dir.join("examples/design.md")]).changes,
        refresh()
    );
}

#[test]
fn widget_files_refresh_whatever_watch_says() {
    let dir = served(
        "widget-files",
        "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\nwatch: [\"**/*.md\"]\n",
    );
    std::fs::create_dir_all(dir.join("docs/widgets")).unwrap();
    std::fs::create_dir_all(dir.join("docs/other")).unwrap();
    std::fs::write(dir.join("docs/other/app.tsx"), "x").unwrap();
    std::fs::write(dir.join("docs/widgets/format.ts"), "export const f = 1;").unwrap();
    let folder = open(&dir, "docs");
    assert_eq!(
        folder.changed(&[dir.join("docs/other/app.tsx")]).changes,
        [],
        "not a widgets folder"
    );
    assert_eq!(
        folder
            .changed(&[dir.join("docs/widgets/format.ts")])
            .changes,
        refresh()
    );
}
