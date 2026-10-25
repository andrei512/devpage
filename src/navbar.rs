//! A site's navbar — its title, then the links, dropdowns and icons its
//! `navbar` field lists — and the light/dark selector every page carries.

use std::path::{Path, PathBuf};

use crate::check::Problem;
use crate::document::{Document, NavItem, NavLink};
use crate::{icons, nav};

/// Where a navbar link goes: an address on the web as written, or a file in
/// the served folder by its address. `file` is set for the latter, whether or
/// not it exists.
struct Target {
    href: String,
    file: Option<PathBuf>,
}

fn target(document: &Document, written: &str) -> Target {
    if written.contains("://") || written.starts_with("mailto:") {
        return Target {
            href: written.to_string(),
            file: None,
        };
    }
    let (path, fragment) = written.split_once('#').unwrap_or((written, ""));
    let file = normalize(&document.base.join(path));
    let mut href = document.url(&file);
    if !fragment.is_empty() {
        href.push('#');
        href.push_str(fragment);
    }
    Target {
        href,
        file: Some(file),
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The site name an icon-only link to the web is known by: `github.com`.
fn host(href: &str) -> String {
    href.split("://")
        .nth(1)
        .unwrap_or(href)
        .split('/')
        .next()
        .unwrap_or(href)
        .trim_start_matches("www.")
        .to_string()
}

/// Every link the navbar holds, dropdowns opened.
fn links(document: &Document) -> impl Iterator<Item = &NavLink> {
    document.navbar.iter().flat_map(|item| match item {
        NavItem::Link(link) => std::slice::from_ref(link).iter(),
        NavItem::Menu { items, .. } => items.iter(),
    })
}

/// The navbar's problems: a link to a file that isn't in the served folder,
/// and an icon devpage doesn't have. They belong to the document file.
pub fn problems(document: &Document) -> Vec<Problem> {
    let file = document.url(&document.path())[1..].to_string();
    let mut problems = Vec::new();
    let mut icon = |name: &Option<String>| {
        if let Some(name) = name
            && !icons::exists(name)
        {
            problems.push(Problem {
                file: file.clone(),
                line: None,
                message: format!("navbar: no icon named {name}"),
                target: None,
            });
        }
    };
    for item in &document.navbar {
        if let NavItem::Menu { icon: name, .. } = item {
            icon(name);
        }
    }
    for link in links(document) {
        icon(&link.icon);
    }
    for link in links(document) {
        if let Some(path) = target(document, &link.target).file {
            let inside = path
                .canonicalize()
                .is_ok_and(|real| real.is_file() && real.starts_with(&document.served));
            if !inside {
                problems.push(Problem {
                    file: file.clone(),
                    line: None,
                    message: format!("navbar: {} — no such file", link.target),
                    target: Some(link.target.clone()),
                });
            }
        }
    }
    problems
}

/// Where the navbar leads, for the link checker: the files it names that
/// exist, and its addresses on the web.
pub fn reached(document: &Document) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut web = Vec::new();
    for link in links(document) {
        match target(document, &link.target).file {
            Some(path) => {
                if let Ok(real) = path.canonicalize()
                    && real.is_file()
                    && real.starts_with(&document.served)
                {
                    files.push(real);
                }
            }
            None if !link.target.starts_with("mailto:") => web.push(link.target.clone()),
            None => {}
        }
    }
    (files, web)
}

/// The address of the navbar link that holds the page shown: the link to the
/// page itself, or else to its nearest ancestor in the sidebar tree, the root
/// page holding them all.
fn active(document: &Document, current: &str) -> Option<String> {
    let targets: Vec<String> = links(document)
        .map(|link| target(document, &link.target).href)
        .collect();
    if targets.iter().any(|href| href == current) {
        return Some(current.to_string());
    }
    fn path(
        document: &Document,
        groups: &[nav::Group],
        current: &str,
        trail: &mut Vec<String>,
    ) -> bool {
        for entry in groups.iter().flat_map(|group| &group.entries) {
            trail.push(document.url(&entry.file));
            if trail.last().is_some_and(|url| url == current)
                || path(document, &entry.children, current, trail)
            {
                return true;
            }
            trail.pop();
        }
        false
    }
    let mut trail = vec![document.entry()];
    let found = trail[0] == current || path(document, &nav::tree(document), current, &mut trail);
    if !found {
        return None;
    }
    trail.into_iter().rev().find(|url| targets.contains(url))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn icon(name: &Option<String>) -> String {
    name.as_deref().and_then(icons::svg).unwrap_or_default()
}

/// One link, in the bar or in a dropdown.
fn link(document: &Document, link: &NavLink, active: Option<&str>, class: &str) -> String {
    let to = target(document, &link.target);
    let current = active == Some(to.href.as_str());
    let external = to.file.is_none();
    let (label, extra) = match &link.label {
        Some(label) => (format!("<span>{}</span>", escape(label)), String::new()),
        None => {
            let name = if external {
                host(&to.href)
            } else {
                link.target.clone()
            };
            (
                String::new(),
                format!(" title=\"{0}\" aria-label=\"{0}\"", escape(&name)),
            )
        }
    };
    format!(
        "<a class=\"{class}{only}{current}\" href=\"{href}\"{extra}{aria}{rel}>{icon}{label}</a>",
        only = if link.label.is_none() {
            " icon-only"
        } else {
            ""
        },
        current = if current { " current" } else { "" },
        href = escape(&to.href),
        aria = if current {
            " aria-current=\"page\""
        } else {
            ""
        },
        rel = if external { " rel=\"noopener\"" } else { "" },
        icon = icon(&link.icon),
    )
}

/// The bar across the top of a site's pages, when its document file sets
/// `navbar`; `current` is the page's address.
pub fn render(document: &Document, current: &str) -> String {
    if document.navbar.is_empty() {
        return String::new();
    }
    let active = active(document, current);
    let active = active.as_deref();
    let mut items = String::new();
    for item in &document.navbar {
        match item {
            NavItem::Link(one) => items.push_str(&link(document, one, active, "navbar-link")),
            NavItem::Menu {
                label,
                icon: name,
                items: menu,
            } => {
                let holds_current = menu
                    .iter()
                    .any(|one| active == Some(target(document, &one.target).href.as_str()));
                let entries: String = menu
                    .iter()
                    .map(|one| link(document, one, active, "navbar-entry"))
                    .collect();
                items.push_str(&format!(
                    "<div class=\"navbar-menu{current}\"><button type=\"button\" class=\"navbar-link\" aria-haspopup=\"true\" aria-expanded=\"false\">{icon}<span>{label}</span>{chevron}</button><div class=\"navbar-dropdown\" role=\"menu\">{entries}</div></div>",
                    current = if holds_current { " current" } else { "" },
                    icon = icon(name),
                    label = escape(label.as_deref().unwrap_or("")),
                    chevron = icons::svg("chevron-down").unwrap_or_default(),
                ));
            }
        }
    }
    format!(
        "<header id=\"navbar\"><div class=\"navbar-inner\"><a class=\"navbar-title{root}\" href=\"{home}\">{title}</a><nav class=\"navbar-links\" aria-label=\"{title}\">{items}</nav><div class=\"navbar-end\">{selector}<button type=\"button\" class=\"navbar-toggle\" aria-expanded=\"false\" aria-label=\"Menu\">{menu}</button></div></div></header>",
        root = if current == document.entry() {
            " current"
        } else {
            ""
        },
        home = escape(&document.entry()),
        title = escape(&document.title),
        selector = selector(document),
        menu = icons::svg("menu").unwrap_or_default(),
    )
}

/// The Auto / Light / Dark selector, when the document shows one.
pub fn selector(document: &Document) -> String {
    if !document.theme_selector {
        return String::new();
    }
    let button = |choice: &str, label: &str, name: &str| {
        format!(
            "<button type=\"button\" role=\"radio\" aria-checked=\"false\" data-theme-choice=\"{choice}\" title=\"{label}\" aria-label=\"{label}\">{}</button>",
            icons::svg(name).unwrap_or_default()
        )
    };
    format!(
        "<div class=\"theme-selector\" role=\"radiogroup\" aria-label=\"Theme\">{}{}{}</div>",
        button("auto", "Auto", "monitor"),
        button("light", "Light", "sun"),
        button("dark", "Dark", "moon")
    )
}

/// The selector for a page with no navbar to hold it: the top-right corner.
pub fn corner(document: &Document) -> String {
    if !document.theme_selector {
        return String::new();
    }
    format!("<div id=\"theme-corner\">{}</div>", selector(document))
}

/// Sets the theme before the page paints, so it never flashes the wrong one:
/// the reader's choice if the page offers one and they made it, else the
/// system's setting.
pub fn head(document: &Document) -> String {
    let stored = if document.theme_selector {
        r#"try{c=localStorage.getItem("devpage-theme")}catch(e){}"#
    } else {
        ""
    };
    format!(
        r#"<script>(function(){{var c;{stored}var d=document.documentElement;d.dataset.themeChoice=c==="light"||c==="dark"?c:"auto";d.dataset.theme=d.dataset.themeChoice==="auto"?(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"):d.dataset.themeChoice}})()</script>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(dir: &Path, yaml: &str) -> Document {
        std::fs::write(dir.join("devpage.yml"), yaml).unwrap();
        Document::load(&dir.join("devpage.yml"), dir).unwrap()
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("devpage-navbar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("guide")).unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Home\n\n## Topics\n\n- [Guide](guide/README.md)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("guide/README.md"),
            "# Guide\n\n## Topics\n\n- [Deep](deep.md)\n",
        )
        .unwrap();
        std::fs::write(dir.join("guide/deep.md"), "# Deep\n").unwrap();
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [guide/deep.md]\n",
        )
        .unwrap();
        dir.canonicalize().unwrap()
    }

    const NAVBAR: &str = "kind: site\ntitle: Garden\nroot: README.md\nnavbar:\n  Home (house): README.md\n  Guide: guide/README.md\n  More (layout-grid):\n    Talk (presentation): talk.yml\n    Site: https://example.com/garden\n  (github): https://github.com/someone/garden\n";

    #[test]
    fn renders_links_dropdowns_icons_and_icon_only_links_in_order() {
        let dir = folder("render");
        let document = site(&dir, NAVBAR);
        let html = render(&document, "/guide/deep.md");
        let order: Vec<usize> = ["Home", "Guide", "More", "Talk", "github.com"]
            .iter()
            .map(|text| {
                html.find(text)
                    .unwrap_or_else(|| panic!("{text} in {html}"))
            })
            .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{html}");
        assert!(html.contains("href=\"/README.md\""), "{html}");
        assert!(html.contains("href=\"/talk.yml\""), "{html}");
        assert!(html.contains("aria-haspopup=\"true\""), "{html}");
        assert!(html.contains("class=\"navbar-link icon-only\" href=\"https://github.com/someone/garden\" title=\"github.com\""), "{html}");
        // house, layout-grid, chevron, presentation, github, menu, and the selector's three.
        assert_eq!(html.matches("<svg class=\"icon\"").count(), 9, "{html}");
    }

    #[test]
    fn highlights_the_link_holding_the_page_shown() {
        let dir = folder("current");
        let document = site(&dir, NAVBAR);
        // A page deep under the guide lights the guide, not home.
        let deep = render(&document, "/guide/deep.md");
        assert!(
            deep.contains("class=\"navbar-link current\" href=\"/guide/README.md\""),
            "{deep}"
        );
        assert!(
            !deep.contains("class=\"navbar-link current\" href=\"/README.md\""),
            "{deep}"
        );
        // The root page lights home.
        let home = render(&document, "/README.md");
        assert!(
            home.contains("class=\"navbar-link current\" href=\"/README.md\""),
            "{home}"
        );
        // A deck in a dropdown lights its dropdown.
        let talk = render(&document, "/talk.yml");
        assert!(talk.contains("class=\"navbar-menu current\""), "{talk}");
    }

    #[test]
    fn missing_files_and_unknown_icons_are_problems() {
        let dir = folder("problems");
        let document = site(
            &dir,
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  Gone (no-such-icon): gone.md\n  Out: ../../outside.md\n  Web: https://example.com\n",
        );
        let found: Vec<String> = problems(&document).iter().map(|p| p.to_string()).collect();
        assert_eq!(
            found,
            [
                "devpage.yml: navbar: no icon named no-such-icon",
                "devpage.yml: navbar: gone.md — no such file",
                "devpage.yml: navbar: ../../outside.md — no such file",
            ],
            "{found:?}"
        );
    }

    #[test]
    fn no_navbar_renders_nothing_and_the_selector_can_be_turned_off() {
        let dir = folder("off");
        let plain = site(&dir, "kind: site\ntitle: T\nroot: README.md\n");
        assert_eq!(render(&plain, "/README.md"), "");
        assert!(corner(&plain).contains("data-theme-choice=\"dark\""));
        assert!(head(&plain).contains("localStorage"));
        let off = site(
            &dir,
            "kind: site\ntitle: T\nroot: README.md\ntheme-selector: false\n",
        );
        assert_eq!(corner(&off), "");
        assert!(
            !head(&off).contains("localStorage"),
            "the system decides when there is no selector"
        );
    }
}
