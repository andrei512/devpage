//! A site's navbar — its title, then the links, dropdowns and icons its
//! `navbar` field lists — and the light/dark selector every page carries.

use std::path::{Path, PathBuf};

use crate::document::{Document, NavItem, NavLink};
use crate::html::{Link, escape};
use crate::problem::Problem;
use crate::{icons, nav};

/// Where a navbar link goes: an address on the web or with another scheme,
/// as written, or a file in the served folder by its address. `file` is set
/// for the latter, whether or not it exists; `web` for an address on the web.
struct Target {
    href: String,
    file: Option<PathBuf>,
    web: bool,
}

fn target(document: &Document, written: &str) -> Target {
    let (path, fragment) = match Link::classify(written) {
        Link::File { path, fragment } => (path, fragment),
        other => {
            return Target {
                href: written.to_string(),
                file: None,
                web: matches!(other, Link::Web(_)),
            };
        }
    };
    let file = normalize(&document.base.join(path));
    // A file outside the served folder has no address: the link keeps its
    // target as written, and the check reports it.
    let href = match document.url(&file) {
        Some(mut href) => {
            if !fragment.is_empty() {
                href.push('#');
                href.push_str(fragment);
            }
            href
        }
        None => written.to_string(),
    };
    Target {
        href,
        file: Some(file),
        web: false,
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
pub(crate) fn problems(document: &Document) -> Vec<Problem> {
    let file = document.part_name(&document.path());
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
            let inside = document.served.file(&path).is_some();
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
pub(crate) fn reached(document: &Document) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut web = Vec::new();
    for link in links(document) {
        let to = target(document, &link.target);
        if let Some(real) = to.file.and_then(|path| document.served.file(&path)) {
            files.push(real);
        } else if to.web {
            web.push(to.href);
        }
    }
    (files, web)
}

/// The address of the navbar link that holds the page shown: the link to the
/// page itself, or else to its nearest ancestor in the site's navigation
/// `tree`, the root page holding them all.
fn active(document: &Document, tree: &[nav::Group], current: &str) -> Option<String> {
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
            // An entry is a file the served folder resolved, so it has an
            // address; one without could hold no page.
            let Some(url) = document.url(&entry.file) else {
                continue;
            };
            trail.push(url);
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
    let found = trail[0] == current || path(document, tree, current, &mut trail);
    if !found {
        return None;
    }
    trail.into_iter().rev().find(|url| targets.contains(url))
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
/// `navbar`; `tree` is the site's navigation tree, `current` the page's
/// address, and `comments` whether comments are on.
pub(super) fn render(
    document: &Document,
    tree: &[nav::Group],
    current: &str,
    comments: bool,
) -> String {
    if document.navbar.is_empty() {
        return String::new();
    }
    let active = active(document, tree, current);
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
        "<header id=\"devpage-navbar\"><div class=\"navbar-inner\"><a class=\"navbar-title{root}\" href=\"{home}\">{logo}{title}</a><nav class=\"navbar-links\" aria-label=\"{title}\">{items}</nav><div class=\"navbar-end\">{selector}<button type=\"button\" class=\"navbar-toggle\" aria-expanded=\"false\" aria-label=\"Menu\">{menu}</button></div></div></header>",
        root = if current == document.entry() {
            " current"
        } else {
            ""
        },
        home = escape(&document.entry()),
        logo = logo(document),
        title = escape(&document.title),
        selector = controls(document, comments),
        menu = icons::svg("menu").unwrap_or_default(),
    )
}

/// The document's logo, to sit before its title; nothing when it has none.
/// It is decoration beside the title it repeats, so it has no alt text.
pub(super) fn logo(document: &Document) -> String {
    match document.logo_url() {
        Some(url) => format!("<img class=\"logo\" src=\"{}\" alt=\"\">", escape(&url)),
        None => String::new(),
    }
}

/// A logo that names no image in the served folder. It belongs to the
/// document file, like a broken navbar link.
pub(crate) fn logo_problem(document: &Document) -> Option<Problem> {
    let written = document.logo.as_ref()?;
    if document.logo_url().is_some() {
        return None;
    }
    Some(Problem {
        file: document.part_name(&document.path()),
        line: None,
        message: format!("logo: {written} — no such file"),
        target: Some(written.clone()),
    })
}

/// The theme selector, when the document shows one: one button that switches
/// between light and dark. It carries both icons; the stylesheet shows the one
/// for the theme a click switches to — the moon in light, the sun in dark.
fn selector(document: &Document) -> String {
    if !document.theme_selector {
        return String::new();
    }
    theme_toggle()
}

/// The light/dark button itself, for a page that shows one.
pub(super) fn theme_toggle() -> String {
    format!(
        "<button type=\"button\" class=\"theme-toggle\" title=\"Switch theme\" aria-label=\"Switch theme\">{}</button>",
        theme_icons()
    )
}

/// The switch's two faces — a moon to switch to dark, a sun to switch to
/// light — which the stylesheet shows one at a time: the page's switch and
/// an embedded design's own.
fn theme_icons() -> String {
    format!(
        "<span class=\"to-dark\">{}</span><span class=\"to-light\">{}</span>",
        icons::svg("moon").unwrap_or_default(),
        icons::svg("sun").unwrap_or_default()
    )
}

/// An embedded design's own theme switch, hidden until the design script
/// finds the design themed; it turns only that embed.
pub(super) fn design_theme_toggle() -> String {
    format!(
        "<button type=\"button\" class=\"design-theme\" hidden title=\"Switch this design's theme\" aria-label=\"Switch this design's theme\">{}</button>",
        theme_icons()
    )
}

/// The comment-mode switch: commenting is opt-in, so the hover icon and the
/// selection's Comment button only appear while it is pressed. The page script
/// remembers the choice; threads show either way. Its icon is the speech
/// bubble the paragraph button uses.
fn comment_mode() -> String {
    "<button type=\"button\" class=\"comment-mode\" title=\"Comment mode\" aria-label=\"Comment mode\" aria-pressed=\"false\"><svg viewBox=\"0 0 20 20\" width=\"17\" height=\"17\" aria-hidden=\"true\"><path d=\"M4 3.5h12A1.5 1.5 0 0 1 17.5 5v8a1.5 1.5 0 0 1-1.5 1.5H9l-3.5 3v-3H4A1.5 1.5 0 0 1 2.5 13V5A1.5 1.5 0 0 1 4 3.5Z\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linejoin=\"round\"/></svg></button>".to_string()
}

/// The page's own controls, in the navbar's end or the corner: the
/// comment-mode switch while comments are on, then the theme selector when
/// the document shows one.
fn controls(document: &Document, comments: bool) -> String {
    let mode = if comments {
        comment_mode()
    } else {
        String::new()
    };
    format!("{mode}{}", selector(document))
}

/// The controls for a page with no navbar to hold them: the top-right corner.
pub(super) fn corner(document: &Document, comments: bool) -> String {
    format!(
        "<div id=\"devpage-corner\">{}</div>",
        controls(document, comments)
    )
}

/// What every page of a document puts in its head: the logo as the browser
/// tab's icon, and a script that sets the theme before the page paints, so it
/// never flashes the wrong one — the theme the reader switched to, if the page
/// offers the switch and they used it, else the system's setting.
pub(super) fn head(document: &Document) -> String {
    let icon = match document.logo_url() {
        Some(url) => format!("<link rel=\"icon\" href=\"{}\">\n", escape(&url)),
        None => String::new(),
    };
    format!("{icon}{}", theme_script(document.theme_selector))
}

/// The script that sets the theme before the page paints: the one the reader
/// switched to, when the page offers the switch and they used it, else the
/// system's setting.
pub(super) fn theme_script(switchable: bool) -> String {
    let stored = if switchable {
        r#"try{c=localStorage.getItem("devpage-theme")}catch(e){}"#
    } else {
        ""
    };
    format!(
        r#"<script>(function(){{var c;{stored}var d=document.documentElement;d.dataset.theme=c==="light"||c==="dark"?c:(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light")}})()</script>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::ServedFolder;

    fn site(dir: &Path, yaml: &str) -> Document {
        std::fs::write(dir.join("devpage.yml"), yaml).unwrap();
        Document::load(&dir.join("devpage.yml"), &ServedFolder::new(dir).unwrap()).unwrap()
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
        let html = render(&document, &nav::tree(&document), "/guide/deep.md", true);
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
        // house, layout-grid, chevron, presentation, github, menu, and the switch's moon and sun.
        assert_eq!(html.matches("<svg class=\"icon\"").count(), 8, "{html}");
    }

    #[test]
    fn highlights_the_link_holding_the_page_shown() {
        let dir = folder("current");
        let document = site(&dir, NAVBAR);
        // A page deep under the guide lights the guide, not home.
        let deep = render(&document, &nav::tree(&document), "/guide/deep.md", true);
        assert!(
            deep.contains("class=\"navbar-link current\" href=\"/guide/README.md\""),
            "{deep}"
        );
        assert!(
            !deep.contains("class=\"navbar-link current\" href=\"/README.md\""),
            "{deep}"
        );
        // The root page lights home.
        let home = render(&document, &nav::tree(&document), "/README.md", true);
        assert!(
            home.contains("class=\"navbar-link current\" href=\"/README.md\""),
            "{home}"
        );
        // A deck in a dropdown lights its dropdown.
        let talk = render(&document, &nav::tree(&document), "/talk.yml", true);
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
    fn a_target_outside_the_served_folder_is_a_problem_and_never_followed() {
        let dir = folder("outside");
        let served = dir.join("served");
        std::fs::create_dir_all(&served).unwrap();
        std::fs::write(served.join("README.md"), "# Home\n").unwrap();
        // The file is there, beside the served folder rather than in it.
        std::fs::write(dir.join("outside.md"), "# Outside\n").unwrap();
        std::fs::write(
            served.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nnavbar:\n  Out: ../outside.md\n  Mail: mailto:a@b.c\n",
        )
        .unwrap();
        let document = Document::load(
            &served.join("devpage.yml"),
            &ServedFolder::new(&served).unwrap(),
        )
        .unwrap();
        let found: Vec<String> = problems(&document).iter().map(|p| p.to_string()).collect();
        assert_eq!(
            found,
            ["devpage.yml: navbar: ../outside.md — no such file"],
            "{found:?}"
        );
        // Not a file to check, and not an address on the web either.
        let (files, web) = reached(&document);
        assert!(files.is_empty(), "{files:?}");
        assert!(web.is_empty(), "{web:?}");
        let html = render(&document, &nav::tree(&document), "/README.md", true);
        let out = html
            .split("<a ")
            .find(|link| link.contains("<span>Out</span>"))
            .unwrap_or_else(|| panic!("{html}"));
        assert!(!out.contains("rel=\"noopener\""), "{out}");
        // Never named: the link keeps the target as written, and no address
        // or path of the file outside reaches the page.
        assert!(out.contains("href=\"../outside.md\""), "{out}");
        assert!(!html.contains(&*dir.to_string_lossy()), "{html}");
    }

    #[test]
    fn no_navbar_renders_nothing_and_the_selector_can_be_turned_off() {
        let dir = folder("off");
        let plain = site(&dir, "kind: site\ntitle: T\nroot: README.md\n");
        assert_eq!(render(&plain, &nav::tree(&plain), "/README.md", true), "");
        assert!(corner(&plain, true).contains("class=\"theme-toggle\""));
        assert!(head(&plain).contains("localStorage"));
        let off = site(
            &dir,
            "kind: site\ntitle: T\nroot: README.md\ntheme-selector: false\n",
        );
        // Commenting is opt-in on every page, so its switch stays when the
        // theme selector is turned off.
        assert!(corner(&plain, true).contains("class=\"comment-mode\""));
        assert!(corner(&off, true).contains("class=\"comment-mode\""));
        assert!(!corner(&off, true).contains("theme-toggle"));
        // With comments off there is no switch, in the corner or the navbar.
        assert!(!corner(&plain, false).contains("comment-mode"));
        assert!(corner(&plain, false).contains("theme-toggle"));
        assert!(
            !head(&off).contains("localStorage"),
            "the system decides when there is no selector"
        );
    }

    #[test]
    fn a_logo_sits_before_the_title_and_is_the_tab_icon() {
        let dir = folder("logo");
        std::fs::write(
            dir.join("logo.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        let with = site(&dir, &format!("{NAVBAR}logo: logo.svg\n"));
        let bar = render(&with, &nav::tree(&with), "/README.md", true);
        let img = bar
            .find("<img class=\"logo\" src=\"/logo.svg\" alt=\"\">")
            .expect(&bar);
        assert!(
            img < bar.find("Garden").unwrap(),
            "the logo comes first: {bar}"
        );
        assert!(head(&with).contains("<link rel=\"icon\" href=\"/logo.svg\">"));
        assert_eq!(logo_problem(&with), None);
        // A doc and a deck take one too, for their tab.
        std::fs::write(
            dir.join("deck.yml"),
            "kind: slides\ntitle: D\nslides: [guide/deep.md]\nlogo: logo.svg\n",
        )
        .unwrap();
        let deck =
            Document::load(&dir.join("deck.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
        assert!(head(&deck).contains("rel=\"icon\" href=\"/logo.svg\""));
        // None, no image and no icon.
        let without = site(&dir, NAVBAR);
        assert!(!render(&without, &nav::tree(&without), "/README.md", true).contains("<img"));
        assert!(!head(&without).contains("rel=\"icon\""));
    }

    #[test]
    fn a_missing_logo_is_a_problem_and_the_page_shows_without_it() {
        let dir = folder("logo-missing");
        let document = site(&dir, &format!("{NAVBAR}logo: gone.svg\n"));
        let problem = logo_problem(&document).expect("a problem");
        assert_eq!(problem.message, "logo: gone.svg — no such file");
        assert_eq!(problem.file, "devpage.yml");
        assert!(!render(&document, &nav::tree(&document), "/README.md", true).contains("<img"));
        assert!(!head(&document).contains("rel=\"icon\""));
        // Outside the served folder is no logo either.
        let outside = site(&dir, &format!("{NAVBAR}logo: ../../etc/hosts\n"));
        assert!(logo_problem(&outside).is_some());
    }
}
