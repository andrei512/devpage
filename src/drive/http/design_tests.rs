//! The design store behind the server: PNGs and their validators, a design
//! as an entry document, and what `devpage check` makes of designs a page
//! embeds.

use std::path::Path;
use std::sync::Arc;

use devpage_core::designs::testing::*;

use crate::drive::http::{App, router};
use devpage_core::comments::Identity;
use devpage_core::designs::Designs;
use devpage_core::document::Document;
use devpage_core::folder::{Folder, ServedFolder};

/// The served folder `dir`, opened on `entry` in it, as `devpage check`
/// opens one.
fn opened(dir: &Path, entry: &str) -> Folder {
    let served = ServedFolder::new(dir).unwrap();
    Folder::new(Arc::new(Document::load(&dir.join(entry), &served).unwrap()))
}

/// What `devpage check` finds in the served folder `dir` opened on `entry`,
/// its designs read through a store of their own.
async fn checked(dir: &Path, entry: &str) -> Vec<devpage_core::problem::Problem> {
    let designs = Designs::new(&ServedFolder::new(dir).unwrap());
    devpage_core::check::document(&opened(dir, entry), &designs, None).await
}

pub(crate) async fn start(dir: &Path, entry: &str) -> (String, Designs) {
    let document =
        Arc::new(Document::load(&dir.join(entry), &ServedFolder::new(dir).unwrap()).unwrap());
    let designs = Designs::new(&document.served);
    let changes = crate::adapters::changes::Changes::default();
    let router = router(
        App::testing(document, changes)
            .with_designs(designs.clone())
            .with_comments_on_disk(Identity::Local("T".into())),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    (address, designs)
}

#[tokio::test]
async fn a_png_answers_304_from_its_scene_without_painting() {
    let dir = served("png-etag");
    let (address, designs) = start(&dir, "app/screen.design").await;
    let url = format!("{address}/app/screen.design.png");
    let client = reqwest::Client::new();
    let first = client.get(&url).send().await.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(first.headers()["cache-control"], "no-cache");
    let tag = first.headers()["etag"].to_str().unwrap().to_string();
    let again = client
        .get(&url)
        .header("if-none-match", &tag)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 304);
    assert_eq!(again.headers()["etag"], tag.as_str());
    // A change to a design it uses is a new scene, and a new PNG.
    let button = std::fs::read_to_string(dir.join("app/button.design")).unwrap();
    save(
        &designs,
        &dir.join("app/button.design"),
        &button.replace("#2563eb", "#16a34a"),
    );
    let changed = client
        .get(&url)
        .header("if-none-match", &tag)
        .send()
        .await
        .unwrap();
    assert_eq!(changed.status(), 200);
    assert_ne!(changed.headers()["etag"], tag.as_str());
}

#[tokio::test]
async fn a_png_or_the_problems() {
    let dir = served("png");
    let (address, _) = start(&dir, "app/screen.design").await;
    let response = reqwest::get(format!("{address}/app/screen.design.png"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "image/png");
    let png = response.bytes().await.unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 200);

    std::fs::write(
        dir.join("app/broken.design"),
        "import Button from \"./button.design\"\n<Frame>\n  <Button lable=\"x\" />\n  <Text size=\"big\">a</Text>\n</Frame>\n",
    )
    .unwrap();
    let response = reqwest::get(format!("{address}/app/broken.design.png"))
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let text = response.text().await.unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("app/broken.design:3:"), "{text}");
    assert!(lines[1].starts_with("app/broken.design:4:"), "{text}");
}

#[tokio::test]
async fn a_png_in_either_theme() {
    let dir = served("png-theme");
    std::fs::write(dir.join("themes.yml"), THEMES).unwrap();
    std::fs::write(
        dir.join("app/themed.design"),
        "<Frame width={10} height={10} fill={theme.bg} />",
    )
    .unwrap();
    let (address, _) = start(&dir, "app/screen.design").await;
    let get = |query: &str| reqwest::get(format!("{address}/app/themed.design.png{query}"));
    let mut tags = Vec::new();
    for (query, value) in [("", 255), ("?theme=light", 255), ("?theme=dark", 0)] {
        let response = get(query).await.unwrap();
        assert_eq!(response.status(), 200, "{query}");
        tags.push(response.headers()["etag"].to_str().unwrap().to_string());
        let png = response.bytes().await.unwrap();
        let (pixels, _, _) = devpage_design::compare::pixels(&png).unwrap();
        assert_eq!(pixels[..4], [value, value, value, 255], "{query}");
    }
    assert_eq!(tags[0], tags[1]);
    assert_ne!(tags[0], tags[2], "each theme is its own PNG");
    let wrong = get("?theme=blue").await.unwrap();
    assert_eq!(wrong.status(), 400);
    assert_eq!(
        wrong.text().await.unwrap(),
        "theme: expected light or dark, found \"blue\""
    );
}

#[tokio::test]
async fn no_png_of_a_design_outside_the_served_folder() {
    let dir = served("png-outside");
    let outside = dir.join("../png-outside-secret.design");
    std::fs::write(&outside, "<Frame width={10} height={10} />").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("app/escape.design")).unwrap();
    let (address, _) = start(&dir, "app/screen.design").await;
    for path in [
        "/../png-outside-secret.design.png",
        "/%2E%2E/png-outside-secret.design.png",
        "/app/escape.design.png",
        "/app/missing.design.png",
    ] {
        let response = reqwest::get(format!("{address}{path}")).await.unwrap();
        assert_eq!(response.status(), 404, "{path}");
        let body = response.text().await.unwrap();
        assert!(
            body.ends_with("is not part of the served folder"),
            "{path}: {body}"
        );
    }
}

#[tokio::test]
async fn a_real_file_named_like_a_designs_png_wins() {
    let dir = served("png-real");
    std::fs::write(dir.join("app/screen.design.png"), b"not a drawing").unwrap();
    let (address, _) = start(&dir, "app/screen.design").await;
    let real = reqwest::get(format!("{address}/app/screen.design.png"))
        .await
        .unwrap();
    assert_eq!(real.status(), 200);
    assert_eq!(real.bytes().await.unwrap().as_ref(), b"not a drawing");
    // Without one, the address is the design's drawing.
    let drawn = reqwest::get(format!("{address}/app/other.design.png"))
        .await
        .unwrap();
    assert_eq!(drawn.headers()["content-type"], "image/png");
}

#[tokio::test]
async fn a_designs_png_is_a_link_check_accepts() {
    let dir = served("png-link");
    std::fs::write(
        dir.join("app/broken.design"),
        SCREEN.replace("<Button label", "<Button lable"),
    )
    .unwrap();
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![Screen](app/screen.design.png)\n\n![Broken](app/broken.design.png)\n\n![Nothing](app/nothing.design.png)\n",
    )
    .unwrap();
    let problems: Vec<String> = checked(&dir, "page.md")
        .await
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0]
            .starts_with("page.md:5: broken design app/broken.design.png — app/broken.design:5:"),
        "{problems:?}"
    );
    assert_eq!(
        problems[1],
        "page.md:7: broken link app/nothing.design.png — no such file"
    );
}

#[tokio::test]
async fn a_design_is_an_entry_document() {
    let dir = served("entry");
    let (address, _) = start(&dir, "app/screen.design").await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let home = client.get(format!("{address}/")).send().await.unwrap();
    assert_eq!(home.status(), 307);
    assert_eq!(home.headers()["location"], "/app/screen.design");
    let page = client
        .get(format!("{address}/app/screen.design"))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.text().await.unwrap().contains("<canvas"));
}

#[tokio::test]
async fn check_reports_a_broken_embedded_design_where_it_is_wrong() {
    let dir = served("check");
    std::fs::write(
        dir.join("app/screen.design"),
        SCREEN.replace("<Button label", "<Button lable"),
    )
    .unwrap();
    std::fs::write(
        dir.join("page.md"),
        "# Page\n\n![Screen](app/screen.design)\n\nEmbed with `![Name](name.design)`:\n\n```markdown\n![Gone](gone.design)\n```\n",
    )
    .unwrap();
    let problems = checked(&dir, "page.md").await;
    assert_eq!(problems.len(), 1, "{problems:?}");
    let shown = problems[0].to_string();
    assert!(
        shown.starts_with("page.md:3: broken design app/screen.design — app/screen.design:5:"),
        "{shown}"
    );
    assert!(shown.contains("lable"), "{shown}");

    let problems = checked(&dir, "app/screen.design").await;
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].to_string().starts_with("app/screen.design:5:"),
        "{problems:?}"
    );
}
