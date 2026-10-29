//! The canvas page: a design alone in the window, drawn by the design
//! script.

use devpage_design::Theme;

use crate::addresses::{Owned, address};
use crate::folder::ServedFolder;

/// The canvas page for the design at `path` in `served`: the design fills
/// the window, drawn by the design script — in `theme` when the address names
/// one, set before the page paints and never remembered.
pub fn canvas(served: &ServedFolder, path: &str, theme: Option<Theme>) -> String {
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
        stylesheet = address(Owned::Asset("style.css")),
        design_script = address(Owned::Asset("design.js")),
        html = super::frame::html_open(served),
        mount = crate::html::escape(served.mount()),
        name = crate::html::escape(name),
        path = crate::html::escape(path),
        theme = super::navbar::theme_script(true),
        chosen = match theme {
            Some(Theme::Dark) =>
                r#"<script>document.documentElement.dataset.theme="dark"</script>"#,
            Some(Theme::Light) =>
                r#"<script>document.documentElement.dataset.theme="light"</script>"#,
            None => "",
        },
        toggle = super::navbar::theme_toggle(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_page_is_titled_without_the_extension() {
        let page = canvas(&ServedFolder::default(), "/app/checkout.design", None);
        assert!(page.contains("<title>checkout</title>"), "{page}");
        assert!(page.contains(r#"src="/app/checkout.design""#), "{page}");
        assert!(page.contains(r#"<html lang="en">"#), "{page}");
    }

    #[test]
    fn a_mounted_canvas_page_carries_its_mount() {
        let served = ServedFolder::default().mounted("/d/x/v/1");
        let page = canvas(&served, "/app/checkout.design", None);
        assert!(
            page.contains(r#"<html lang="en" data-mount="/d/x/v/1">"#),
            "{page}"
        );
        assert!(
            page.contains(r#"src="/d/x/v/1/app/checkout.design""#),
            "{page}"
        );
    }
}
