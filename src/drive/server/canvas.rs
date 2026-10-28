//! The canvas page: a design alone in the window, drawn by the design
//! script.

use devpage_core::served::ServedFolder;
use devpage_design::Theme;

/// The canvas page for the design at `path` in `served`: the design fills
/// the window, drawn by the design script — in `theme` when the address names
/// one, set before the page paints and never remembered.
pub fn page(served: &ServedFolder, path: &str, theme: Option<Theme>) -> String {
    let path = path.trim_start_matches('/');
    let file = path.rsplit('/').next().unwrap_or(path);
    let name = file.strip_suffix(".design").unwrap_or(file);
    format!(
        r#"<!doctype html>
{html}<head><meta charset="utf-8"><title>{name}</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="stylesheet" href="{stylesheet}">
{theme}{chosen}
</head><body class="design-page">
<canvas class="design" src="{mount}/{path}" role="img" aria-label="{name}"></canvas>
<div id="devpage-corner">{toggle}</div>
<script type="module" src="{design_script}"></script>
</body></html>"#,
        stylesheet = crate::addresses::address(crate::addresses::Owned::Asset("style.css")),
        design_script = crate::addresses::address(crate::addresses::Owned::Asset("design.js")),
        html = crate::render::html_open(served),
        mount = crate::render::escape(served.mount()),
        name = crate::render::escape(name),
        path = crate::render::escape(path),
        theme = crate::navbar::theme_script(true),
        chosen = match theme {
            Some(Theme::Dark) =>
                r#"<script>document.documentElement.dataset.theme="dark"</script>"#,
            Some(Theme::Light) =>
                r#"<script>document.documentElement.dataset.theme="light"</script>"#,
            None => "",
        },
        toggle = crate::navbar::theme_toggle(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::design_tests::start;
    use devpage_core::designs::testing::served;

    #[test]
    fn the_canvas_page_is_titled_without_the_extension() {
        let page = super::page(&ServedFolder::default(), "/app/checkout.design", None);
        assert!(page.contains("<title>checkout</title>"), "{page}");
        assert!(page.contains(r#"src="/app/checkout.design""#), "{page}");
        assert!(page.contains(r#"<html lang="en">"#), "{page}");
    }

    #[test]
    fn a_mounted_canvas_page_carries_its_mount() {
        let served = ServedFolder::default().mounted("/d/x/v/1");
        let page = super::page(&served, "/app/checkout.design", None);
        assert!(
            page.contains(r#"<html lang="en" data-mount="/d/x/v/1">"#),
            "{page}"
        );
        assert!(
            page.contains(r#"src="/d/x/v/1/app/checkout.design""#),
            "{page}"
        );
    }

    #[tokio::test]
    async fn the_canvas_page_shows_the_theme_its_address_names() {
        let dir = served("page-theme");
        let (address, _) = start(&dir, "app/screen.design").await;
        let page = |query: &str| {
            let url = format!("{address}/app/screen.design{query}");
            async move { reqwest::get(url).await.unwrap().text().await.unwrap() }
        };
        let chosen = r#"<script>document.documentElement.dataset.theme="dark"</script>"#;
        assert!(page("?theme=dark").await.contains(chosen));
        assert!(!page("").await.contains("dataset.theme=\"dark\""));
        assert!(!page("?theme=blue").await.contains("dataset.theme=\"blue\""));
    }
}
