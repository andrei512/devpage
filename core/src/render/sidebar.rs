//! A site's sidebar: its title, the search box and the tree its Topics
//! sections form, as markup and as the JSON `DevpageSidebar` takes over from.

use crate::document::Document;
use crate::nav;

use super::escape;

/// A site's sidebar, unless it sets `sidebar: false`: its title — left to the
/// navbar when there is one — then the tree its Topics sections form,
/// collapsed but for the branch down to the page shown, under the search box.
/// It is the markup
/// [`DevpageSidebar`](crate::widgets::SIDEBAR) renders from the same tree,
/// which follows it as JSON: the page script has React take the sidebar over
/// without changing a node. Before the page paints, a small script opens the
/// entries the reader opened on another visit, as the widget will.
pub(super) fn sidebar(document: &Document, current: &str) -> String {
    if !document.sidebar {
        return String::new();
    }
    let groups = nav::tree(document);
    let title = document.navbar.is_empty().then(|| SidebarTitle {
        label: document.title.clone(),
        href: document.entry(),
        logo: document.logo_url(),
    });
    let mut html = format!(
        "<nav id=\"devpage-nav\" data-sidebar=\"{}\">",
        escape(&document.entry())
    );
    if let Some(title) = &title {
        html.push_str(&format!(
            "<a class=\"document-title{}\" href=\"{}\">{}{}</a>",
            if title.href == current {
                " current"
            } else {
                ""
            },
            escape(&title.href),
            crate::navbar::logo(document),
            escape(&title.label)
        ));
    }
    html.push_str(SEARCH_BOX);
    let groups: Vec<SidebarGroup> = groups
        .iter()
        .map(|g| SidebarGroup::of(document, g))
        .collect();
    tree(&groups, current, &mut html);
    html.push_str("</nav>");
    let data = SidebarData {
        document: document.entry(),
        document_path: document.url(&document.path()),
        current: current.to_string(),
        title,
        groups,
    };
    // `<` escaped, so no label can close the script.
    let json = serde_json::to_string(&data)
        .unwrap_or_default()
        .replace('<', "\\u003c");
    html.push_str(&format!(
        "<script type=\"application/json\" id=\"devpage-sidebar\">{json}</script><script>{OPEN_REMEMBERED}</script>"
    ));
    html
}

/// The search box at the top of the sidebar, as `DevpageSidebar` renders it
/// before anything is typed: the field, and the `/` that focuses it.
const SEARCH_BOX: &str = "<div class=\"search\"><input type=\"search\" placeholder=\"Search\" aria-label=\"Search the documents\" autocomplete=\"off\" spellcheck=\"false\" role=\"combobox\" aria-expanded=\"false\" aria-autocomplete=\"list\" value=\"\"><kbd>/</kbd></div>";

/// Opens, before the page paints, the entries the reader opened on another
/// visit to the document, which `DevpageSidebar` keeps in `localStorage`
/// under `devpage-sidebar:` and the document's address. It changes only what
/// the widget's first render will, so React takes over the same DOM.
const OPEN_REMEMBERED: &str = "(()=>{const nav=document.getElementById(\"devpage-nav\");let opened;try{opened=JSON.parse(localStorage.getItem(\"devpage-sidebar:\"+nav.dataset.sidebar))}catch{}if(!Array.isArray(opened))return;for(const li of nav.querySelectorAll(\"li.branch:not(.open)\"))if(opened.includes(li.dataset.href)){li.classList.add(\"open\");li.querySelector(\".chevron\").setAttribute(\"aria-expanded\",\"true\")}})()";

/// What `DevpageSidebar` renders: the document it belongs to (its address,
/// which keys what the reader opened, and its document file's, which a
/// search hit names it by), the page shown, the title link when the sidebar
/// has one, and the tree.
#[derive(serde::Serialize)]
struct SidebarData {
    document: String,
    document_path: String,
    current: String,
    title: Option<SidebarTitle>,
    groups: Vec<SidebarGroup>,
}

#[derive(serde::Serialize)]
struct SidebarTitle {
    label: String,
    href: String,
    logo: Option<String>,
}

#[derive(serde::Serialize)]
struct SidebarGroup {
    title: Option<String>,
    entries: Vec<SidebarEntry>,
}

#[derive(serde::Serialize)]
struct SidebarEntry {
    label: String,
    href: String,
    /// Whether this is the page's position, not a second listing of it.
    home: bool,
    children: Vec<SidebarGroup>,
}

impl SidebarGroup {
    fn of(document: &Document, group: &nav::Group) -> Self {
        Self {
            title: group.title.clone(),
            entries: group
                .entries
                .iter()
                .map(|entry| SidebarEntry {
                    label: entry.label.clone(),
                    href: document.url(&entry.file),
                    home: entry.home,
                    children: entry
                        .children
                        .iter()
                        .map(|g| SidebarGroup::of(document, g))
                        .collect(),
                })
                .collect(),
        }
    }
}

impl SidebarEntry {
    /// Whether the page shown is this entry or under it: the entries open
    /// when a page is shown.
    fn leads_to(&self, current: &str) -> bool {
        self.is(current)
            || self
                .children
                .iter()
                .flat_map(|g| &g.entries)
                .any(|e| e.leads_to(current))
    }
}

impl SidebarEntry {
    /// Whether this entry is the page shown: its position, never a second
    /// listing of it.
    fn is(&self, current: &str) -> bool {
        self.home && self.href == current
    }
}

/// The tree as nested `<ul>` lists: a group's title as a heading in place, an
/// entry with children as a branch — its link, a chevron that opens and
/// closes it, and its children's list, shown while open. No text sits
/// between elements: React renders none.
fn tree(groups: &[SidebarGroup], current: &str, html: &mut String) {
    if groups.is_empty() {
        return;
    }
    html.push_str("<ul>");
    for group in groups {
        if let Some(title) = &group.title {
            html.push_str(&format!(
                "<li class=\"group\"><span>{}</span></li>",
                escape(title)
            ));
        }
        for entry in &group.entries {
            let href = escape(&entry.href);
            let label = escape(&entry.label);
            let class = if entry.is(current) {
                " class=\"current\""
            } else {
                ""
            };
            if entry.children.is_empty() {
                html.push_str(&format!("<li><a href=\"{href}\"{class}>{label}</a></li>"));
                continue;
            }
            let open = entry.leads_to(current);
            html.push_str(&format!(
                "<li class=\"branch{}\" data-href=\"{href}\"><div class=\"entry\"><a href=\"{href}\"{class}>{label}</a><button type=\"button\" class=\"chevron\" aria-expanded=\"{open}\" aria-label=\"{label}\"></button></div>",
                if open { " open" } else { "" },
            ));
            tree(&entry.children, current, html);
            html.push_str("</li>");
        }
    }
    html.push_str("</ul>");
}
