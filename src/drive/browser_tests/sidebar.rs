//! The sidebar in Chrome: hydration, resizing, navigation, and search.

use std::path::PathBuf;

use chromiumoxide::Page;

use super::support::{open, open_logged, since, text, widget_folder};
use crate::adapters::chrome::testing::{self, eval, until};

/// A site whose tree is two levels deep under *Guide*, with *Fields*
/// under a group heading.
fn sidebar_folder(name: &str) -> PathBuf {
    widget_folder(
        name,
        "# Home\n\n## Topics\n\n- [Guide](guide/README.md)\n\n### Reference\n\n- [Fields](fields.md)\n",
        &[
            (
                "docs/guide/README.md",
                "# Guide\n\n## Topics\n\n- [Step](step.md)\n- [Deep](deep.md)\n",
            ),
            (
                "docs/guide/deep.md",
                "# Deep\n\n## Topics\n\n- [Leaf](leaf.md)\n",
            ),
            ("docs/guide/step.md", "# Step\n"),
            ("docs/guide/leaf.md", "# Leaf\n"),
            ("docs/guide/more.md", "# More\n"),
            (
                "docs/fields.md",
                "# Fields\n\n## Topics\n\n- [Field](field.md)\n",
            ),
            ("docs/field.md", "# Field\n"),
        ],
    )
}

/// Records the sidebar as the server drew it, before the page script
/// runs: the document turns `interactive` before deferred scripts do.
const BEFORE_TAKEOVER: &str = "document.addEventListener('readystatechange', () => {
        if (document.readyState !== 'interactive') return;
        const nav = document.getElementById('devpage-nav');
        const nodes = [nav, ...nav.querySelectorAll('*')];
        window.before = { nodes, boxes: nodes.map((n) => JSON.stringify(n.getBoundingClientRect())), html: nav.outerHTML };
      });";

/// Whether the sidebar is the very DOM the server drew — every node, its
/// markup and its place — now that React has taken it over.
const SAME_AS_BEFORE: &str = "(() => {
        const nav = document.getElementById('devpage-nav');
        const nodes = [nav, ...nav.querySelectorAll('*')];
        return [nodes.length === window.before.nodes.length && nodes.every((n, i) => n === window.before.nodes[i]),
          nav.outerHTML === window.before.html,
          nodes.every((n, i) => JSON.stringify(n.getBoundingClientRect()) === window.before.boxes[i])].join(' ');
      })()";

/// Which branches of the sidebar are open, by label.
const OPEN: &str = "[...document.querySelectorAll('#devpage-nav li.branch.open > .entry > a')].map((a) => a.textContent).join(',')";

/// Clicks the chevron of the sidebar entry labelled `label`.
fn chevron(label: &str) -> String {
    format!(
        "[...document.querySelectorAll('#devpage-nav .chevron')].find((b) => b.getAttribute('aria-label') === '{label}').click(); 0"
    )
}

/// A desktop's window, where the sidebar stands beside the page.
async fn desktop(page: &Page) {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    page.execute(SetDeviceMetricsOverrideParams::new(1300, 900, 1.0, false))
        .await
        .unwrap();
}

/// Reloads the page, and waits until it is ready.
async fn reload_watching_takeover(page: &Page) {
    page.evaluate("delete document.documentElement.dataset.ready; 0")
        .await
        .unwrap();
    page.reload().await.unwrap();
    until(page, "document.documentElement.dataset.ready === 'true'").await;
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_sidebar_is_taken_over_without_a_change_and_its_chevrons_open_and_close() {
    let dir = sidebar_folder("sidebar-takeover");
    let (mut browser, page) = open(&dir, "/docs/guide/deep.md").await;
    desktop(&page).await;
    page.evaluate_on_new_document(BEFORE_TAKEOVER)
        .await
        .unwrap();
    reload_watching_takeover(&page).await;
    // First paint: the sidebar, collapsed, the branch to the page open.
    assert_eq!(
            text(&page, "window.before.nodes.filter((n) => n.matches('li.branch.open')).map((n) => n.querySelector('a').textContent).join(',')").await,
            "Guide,Deep"
        );
    assert_eq!(
        text(
            &page,
            "String(window.before.nodes.some((n) => n.matches('.search input')))"
        )
        .await,
        "true",
        "the search box is drawn with the sidebar"
    );
    assert_eq!(
        text(&page, SAME_AS_BEFORE).await,
        "true true true",
        "React takes the sidebar over without changing it"
    );
    // React re-renders every branch, twice, and writes what it believes
    // is there: the markup it renders is the server's.
    page.evaluate(
        "for (const b of document.querySelectorAll('#devpage-nav .chevron')) b.click(); 0",
    )
    .await
    .unwrap();
    page.evaluate(
        "for (const b of document.querySelectorAll('#devpage-nav .chevron')) b.click(); 0",
    )
    .await
    .unwrap();
    assert_eq!(
        text(
            &page,
            "String(document.getElementById('devpage-nav').outerHTML === window.before.html)"
        )
        .await,
        "true"
    );
    assert_eq!(
            text(&page, "getComputedStyle(document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"] > ul')).display").await,
            "none",
            "a closed branch hides its children"
        );

    // A chevron opens and closes its entry; the link beside it is left.
    page.evaluate(chevron("Fields")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Guide,Deep,Fields'")).await;
    assert_eq!(
            text(&page, "document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"] .chevron').getAttribute('aria-expanded') + ' ' + location.pathname").await,
            "true /docs/guide/deep.md"
        );
    page.evaluate(chevron("Deep")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Guide,Fields'")).await;

    // A reload opens what the reader opened before the first paint, and
    // React takes that over without a change either.
    reload_watching_takeover(&page).await;
    assert_eq!(
            text(&page, "window.before.nodes.filter((n) => n.matches('li.branch.open')).map((n) => n.querySelector('a').textContent).join(',')").await,
            "Guide,Deep,Fields"
        );
    assert_eq!(text(&page, SAME_AS_BEFORE).await, "true true true");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// The sidebar's width and what the browser keeps of it.
const WIDTH: &str = "document.getElementById('devpage-nav').getBoundingClientRect().width + ' ' + localStorage.getItem('devpage-sidebar-width')";

/// Drags the sidebar's handle to `x`, as a pointer does.
fn drag_handle_to(x: u32) -> String {
    format!(
        "(() => {{ const at = (type, clientX) => new PointerEvent(type, {{ clientX, button: 0, bubbles: true }});
           document.querySelector('#devpage-nav .resize').dispatchEvent(at('pointerdown', 280));
           window.dispatchEvent(at('pointermove', {x}));
           const during = document.documentElement.classList.contains('resizing-sidebar');
           window.dispatchEvent(at('pointerup', {x}));
           return String(during && !document.documentElement.classList.contains('resizing-sidebar')); }})()"
    )
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_sidebar_is_resized_by_its_handle_and_keeps_its_width() {
    let dir = sidebar_folder("sidebar-resize");
    let (mut browser, page) = open(&dir, "/docs/guide/deep.md").await;
    desktop(&page).await;
    page.evaluate_on_new_document(BEFORE_TAKEOVER)
        .await
        .unwrap();
    reload_watching_takeover(&page).await;
    assert_eq!(text(&page, WIDTH).await, "280 null");
    // The handle stands over the sidebar's right edge, the sidebar's height.
    assert_eq!(
        text(&page, "(() => { const h = document.querySelector('#devpage-nav .resize').getBoundingClientRect(); return [h.left, h.right, h.height].join(' '); })()").await,
        "276 284 900"
    );

    // A drag sets the width, and it is kept.
    assert_eq!(text(&page, &drag_handle_to(400)).await, "true");
    assert_eq!(text(&page, WIDTH).await, "400 400");
    // Never narrower than a sidebar can be read at, nor wider than half
    // the window.
    page.evaluate(drag_handle_to(40)).await.unwrap();
    assert_eq!(text(&page, WIDTH).await, "200 200");
    page.evaluate(drag_handle_to(1200)).await.unwrap();
    assert_eq!(text(&page, WIDTH).await, "640 640");
    page.evaluate(drag_handle_to(400)).await.unwrap();

    // A reload draws the sidebar at that width from the first frame, and
    // React takes it over without a change.
    reload_watching_takeover(&page).await;
    assert_eq!(
        text(&page, "JSON.parse(window.before.boxes[0]).width + ' ' + document.querySelector('#devpage-page').getBoundingClientRect().left").await,
        "400 400"
    );
    assert_eq!(text(&page, SAME_AS_BEFORE).await, "true true true");

    // The arrow keys move it, and a double click gives devpage's width back.
    page.evaluate("document.querySelector('#devpage-nav .resize').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); 0")
        .await
        .unwrap();
    assert_eq!(text(&page, WIDTH).await, "416 416");
    page.evaluate("document.querySelector('#devpage-nav .resize').dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); 0")
        .await
        .unwrap();
    assert_eq!(text(&page, WIDTH).await, "280 null");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Whether the sidebar shows the current page's entry, whole.
const CURRENT_IN_SIGHT: &str = "(() => { const nav = document.getElementById('devpage-nav').getBoundingClientRect();
        const at = document.querySelector('#devpage-nav ul a.current').getBoundingClientRect();
        return document.querySelector('#devpage-nav ul a.current').textContent + ' ' + (at.top >= nav.top && at.bottom <= nav.bottom); })()";

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_page_far_down_a_long_sidebar_is_in_sight() {
    let topics: String = (1..=80).map(|n| format!("- [P{n}](p{n}.md)\n")).collect();
    let pages: Vec<(String, String)> = (1..=80)
        .map(|n| (format!("docs/p{n}.md"), format!("# P{n}\n")))
        .collect();
    let files: Vec<(&str, &str)> = pages.iter().map(|(f, t)| (&f[..], &t[..])).collect();
    let dir = widget_folder(
        "sidebar-in-sight",
        &format!("# Home\n\n## Topics\n\n{topics}"),
        &files,
    );
    let (mut browser, page) = open(&dir, "/docs/p80.md").await;
    desktop(&page).await;
    page.evaluate_on_new_document(
        "document.addEventListener('readystatechange', () => {
           if (document.readyState === 'interactive') window.scrolledAtFirst = document.getElementById('devpage-nav').scrollTop;
         });",
    )
    .await
    .unwrap();
    reload_watching_takeover(&page).await;
    // Scrolled there before the page script ran, and still.
    assert_eq!(
        text(&page, "String(window.scrolledAtFirst > 0 && window.scrolledAtFirst === document.getElementById('devpage-nav').scrollTop)").await,
        "true"
    );
    assert_eq!(text(&page, CURRENT_IN_SIGHT).await, "P80 true");
    // A page moved to that is out of sight is brought in; one in sight
    // leaves the sidebar where it is scrolled.
    assert_eq!(
        text(
            &page,
            &format!("load('/docs/p1.md', 'push').then(() => {CURRENT_IN_SIGHT})")
        )
        .await,
        "P1 true"
    );
    assert_eq!(
        text(&page, &format!("(() => {{ const before = document.getElementById('devpage-nav').scrollTop; return load('/docs/p3.md', 'push').then(() => {CURRENT_IN_SIGHT} + ' ' + (document.getElementById('devpage-nav').scrollTop === before)); }})()")).await,
        "P3 true true"
    );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn the_sidebar_stays_mounted_across_swaps_and_opens_the_branch_navigated_to() {
    let dir = sidebar_folder("sidebar-swap");
    let (mut browser, page, answered) = open_logged(&dir, "/docs/README.md").await;
    desktop(&page).await;
    reload_watching_takeover(&page).await;
    assert_eq!(text(&page, OPEN).await, "");
    page.evaluate(chevron("Fields")).await.unwrap();
    until(&page, &format!("{OPEN} === 'Fields'")).await;
    // Every frame from here on: the sidebar is the same element, in the
    // same place, never empty, and highlights the page the frame shows.
    page.evaluate(
            "window.nav = document.getElementById('devpage-nav'); window.fields = document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]');
            window.navBox = JSON.stringify(window.nav.getBoundingClientRect());
            window.frames_ = []; (function tick() {
              const nav = document.getElementById('devpage-nav');
              window.frames_.push({
                same: nav === window.nav && JSON.stringify(nav.getBoundingClientRect()) === window.navBox,
                entries: nav.querySelectorAll('a').length,
                agrees: [...nav.querySelectorAll('a.current')].every((a) => a.getAttribute('href') === document.querySelector('#devpage-page article').dataset.source)
                  && nav.querySelectorAll('a.current').length > 0,
              });
              requestAnimationFrame(tick);
            })(); 0",
        )
        .await
        .unwrap();
    let from = answered.lock().unwrap().len();
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.textContent === 'Guide').click(); 0")
            .await
            .unwrap();
    until(&page, "location.pathname === '/docs/guide/README.md'").await;
    assert_eq!(text(&page, OPEN).await, "Guide,Fields", "its branch opens");
    // The sidebar follows in the swap's own commit, not a task later.
    assert_eq!(
            text(&page, &format!("load('/docs/guide/deep.md', 'push').then(() => document.querySelector('#devpage-nav a.current').textContent + ' ' + {OPEN})")).await,
            "Deep Guide,Deep,Fields"
        );
    page.evaluate("history.back()").await.unwrap();
    // Swapped back in: the page shown is the guide's.
    until(
        &page,
        "location.pathname === '/docs/guide/README.md' \
         && document.querySelector('#devpage-page article').dataset.source === '/docs/guide/README.md'",
    )
    .await;
    assert_eq!(
            text(&page, "[window.fields === document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]'), document.querySelector('#devpage-nav a.current').textContent].join(' ')").await,
            "true Guide",
            "not remounted: the same nodes, the highlight moved"
        );
    assert_eq!(
        text(&page, OPEN).await,
        "Guide,Deep,Fields",
        "open stays open"
    );
    until(&page, "window.frames_.length > 5").await;
    assert_eq!(
            text(
                &page,
                "String(window.frames_.length > 5 && window.frames_.every((f) => f.same && f.entries > 0 && f.agrees))"
            )
            .await,
            "true",
            "no frame moves the sidebar, empties it, or highlights another page than it shows: {}", text(&page, "JSON.stringify(window.frames_.filter((f) => !(f.same && f.entries > 0 && f.agrees)).slice(0, 5)) + window.navBox + JSON.stringify(document.getElementById('devpage-nav').getBoundingClientRect())").await
        );
    let fetched: Vec<_> = since(&answered, from)
        .into_iter()
        .filter(|(path, _)| path.starts_with("/__devpage/"))
        .collect();
    assert!(
        !fetched
            .iter()
            .any(|(path, _)| path.contains("DevpageSidebar")),
        "{fetched:?}"
    );

    // A live update that edits the tree reaches the mounted sidebar. The
    // page it adds is listed nowhere else: a page already in the tree
    // keeps its first, shallowest place.
    std::fs::write(
        dir.join("docs/guide/README.md"),
        "# Guide\n\n## Topics\n\n- [Step](step.md)\n- [Deep](deep.md)\n- [More](more.md)\n",
    )
    .unwrap();
    page.evaluate("load(location.href)").await.unwrap();
    until(
                &page,
                "[...document.querySelectorAll('#devpage-nav li[data-href$=\"/guide/README.md\"] a')].some((a) => a.textContent === 'More')"
            ).await;
    assert_eq!(
            text(&page, "String(window.nav === document.getElementById('devpage-nav') && window.fields === document.querySelector('#devpage-nav li[data-href$=\"/fields.md\"]'))").await,
            "true"
        );
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// A site whose *Retries* page has *The outbox* far down it — the word
/// far down that section, in prose, in code and in a widget — then
/// *Later*, which has it too; another site its Topics reach,
/// *Elsewhere*, titled *Outbox*; and a doc, *Notes*, whose *Drain*
/// section has the word far down it.
fn search_folder(name: &str) -> PathBuf {
    let filler = "Words to scroll past.\n\n".repeat(40);
    widget_folder(
        name,
        "# Home\n\n## Topics\n\n- [Guide](guide.md)\n- [Retries](retries.md)\n- [Elsewhere](../other/devpage.yml)\n",
        &[
            ("docs/guide.md", "# Guide\n\nRead on.\n"),
            (
                "docs/retries.md",
                &format!(
                    "# Retries\n\n{filler}## The outbox\n\n{filler}The client keeps every write in its outbox; `outbox` in code is left alone.\n\n<Callout>An outbox in a widget is left alone.</Callout>\n\n```text\nan outbox in a code block\n```\n\nOutboxes drain in order.\n\n{filler}## Later\n\nAnother outbox, in another section.\n\n{filler}"
                ),
            ),
            (
                "other/devpage.yml",
                "kind: site\ntitle: Elsewhere\nroot: README.md\n",
            ),
            ("other/README.md", "# Outbox\n\nAn outbox, elsewhere.\n"),
            (
                "docs/notes/notes.yml",
                "kind: doc\ntitle: Notes\nchapters: [notes.md]\n",
            ),
            (
                "docs/notes/notes.md",
                &format!("# Notes\n\n## Drain\n\n{filler}The outbox drains.\n\n{filler}"),
            ),
        ],
    )
}

/// The endpoint's answers by query, in ticket 04's shape: `outbox` finds
/// Elsewhere's page by its title, then *Retries*' two sections and two
/// more, then the doc's *Drain*; `retries` finds *Retries* by its title.
const SEARCH_ANSWERS: &str = r#"{
      "outbox": { "pages": [
        { "title": "Outbox", "url": "/other/README.md", "trail": [], "document": { "title": "Elsewhere", "path": "/other/devpage.yml" },
          "kind": "page", "title_match": true, "lede": "An outbox, elsewhere.", "sections": [], "more": 0 },
        { "title": "Retries", "url": "/docs/retries.md", "trail": ["Retries"], "document": { "title": "W", "path": "/docs/devpage.yml" },
          "kind": "page", "title_match": false, "lede": "Words to scroll past.", "more": 2, "sections": [
            { "heading": "The outbox", "path": ["Retries", "The outbox"], "url": "/docs/retries.md#the-outbox",
              "snippet": "The client keeps every write in its <mark>outbox</mark>; …", "words": ["outbox", "Outboxes"] },
            { "heading": "Later", "path": ["Retries", "Later"], "url": "/docs/retries.md#later",
              "snippet": "Another <mark>outbox</mark>, in another section.", "words": ["outbox"] } ] },
        { "title": "Notes", "url": "/docs/notes/notes.yml", "trail": [], "document": { "title": "Notes", "path": "/docs/notes/notes.yml" },
          "kind": "chapter", "title_match": false, "lede": "", "more": 0, "sections": [
            { "heading": "Drain", "path": ["Notes", "Drain"], "url": "/docs/notes/notes.yml#drain",
              "snippet": "The <mark>outbox</mark> drains.", "words": ["outbox"] } ] }
      ] },
      "drain": { "pages": [
        { "title": "Notes", "url": "/docs/notes/notes.yml", "trail": [], "document": { "title": "Notes", "path": "/docs/notes/notes.yml" },
          "kind": "chapter", "title_match": false, "lede": "", "more": 0, "sections": [
            { "heading": "Drain", "path": ["Notes", "Drain"], "url": "/docs/notes/notes.yml#drain",
              "snippet": "The outbox <mark>drains</mark>.", "words": ["drains"] } ] } ] },
      "retries": { "pages": [
        { "title": "Retries", "url": "/docs/retries.md", "trail": ["Retries"], "document": { "title": "W", "path": "/docs/devpage.yml" },
          "kind": "page", "title_match": true, "lede": "Words to scroll past.", "sections": [], "more": 0 } ] }
    }"#;

/// Has the page's searches answered from [`SEARCH_ANSWERS`] — no pages
/// for any other text — until ticket 04's endpoint gives that shape. The
/// text `slow` names is answered only once `release()` is called, whether
/// aborted or not; `window.answered` lists each text as it is answered.
async fn stub_search(page: &Page, slow: &str) {
    page.evaluate(format!(
            "window.asked = []; window.answered = []; const fetch_ = window.fetch; const answers = {SEARCH_ANSWERS};
            let released; const held = new Promise((done) => {{ released = done; }}); window.release = released;
            window.fetch = async (url, options) => {{
              if (!String(url).includes('/__devpage/search')) return fetch_(url, options);
              const q = new URL(url, location.href).searchParams.get('q');
              window.asked.push(q);
              const body = JSON.stringify(answers[q] ?? {{ pages: [] }});
              if (q === '{slow}') await held;
              window.answered.push(q);
              return new Response(body, {{ headers: {{ 'content-type': 'application/json' }} }});
            }}; 0"
        ))
        .await
        .unwrap();
}

/// The sidebar's search box.
const SEARCH_INPUT: &str = "#devpage-nav .search input";

/// The result rows shown, in order: each one's kind and its label.
const ROWS: &str = "[...document.querySelectorAll('#devpage-nav .search-results .hit')].map((a) => a.classList[1] + ' ' + a.querySelector('.hit-label').textContent).join('|')";

/// The selected row's label.
const SELECTED: &str =
    "document.querySelector('#devpage-nav .hit.selected .hit-label')?.textContent";

/// The words marked as found in the page, in order.
const FOUND: &str = "[...document.querySelectorAll('#devpage-page mark.devpage-found')].map((m) => m.textContent).join(',')";

/// Types `outbox` in the search box and waits for its results.
async fn search_outbox(page: &Page) -> chromiumoxide::Element {
    let input = page.find_element(SEARCH_INPUT).await.unwrap();
    input.click().await.unwrap();
    input.type_str("outbox").await.unwrap();
    until(page, &format!("{ROWS}.includes('Drain')")).await;
    input
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn search_results_are_grouped_by_page_with_their_sections_under_them() {
    let dir = search_folder("search-typing");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    let input = search_outbox(&page).await;
    // The tree's place: each page in the server's order, its sections
    // under it, then what else it holds.
    assert_eq!(
        text(&page, ROWS).await,
        "hit-page Outbox|hit-page Retries|hit-section The outbox|hit-section Later|hit-more 2 more in this page|hit-page Notes|hit-section Drain"
    );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .search-page')].map((p) => [...p.querySelectorAll('.hit-label')].map((l) => l.textContent).join('/')).join(',')").await,
            "Outbox,Retries/The outbox/Later/2 more in this page,Notes/Drain",
            "a page's rows are one group"
        );
    assert_eq!(
        text(
            &page,
            "getComputedStyle(document.querySelector('#devpage-nav > ul')).display"
        )
        .await,
        "none",
        "the tree is hidden while the box holds text"
    );
    // A page found by its title shows its lede; another document's page
    // says which document it is in; a section's snippet marks the words.
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .hit-page')].map((a) => [a.querySelector('.hit-place')?.textContent, a.querySelector('.hit-lede')?.textContent].join('/')).join(',')").await,
            "Elsewhere/An outbox, elsewhere.,/,/",
            "a document's title is not said again over a page of that title"
        );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .hit-section')[0].querySelectorAll('.hit-snippet mark')].map((m) => m.textContent).join(',')").await,
            "outbox"
        );
    assert_eq!(
            text(&page, "[...document.querySelectorAll('#devpage-nav .search-results .hit')].map((a) => a.getAttribute('href')).join(' ')").await,
            "/other/README.md /docs/retries.md /docs/retries.md#the-outbox /docs/retries.md#later /docs/retries.md /docs/notes/notes.yml /docs/notes/notes.yml#drain",
            "the more row opens the page"
        );
    // The page found by its title is selected first; the arrows move
    // through every row, round from the last to the first.
    assert_eq!(text(&page, SELECTED).await, "Outbox");
    for label in [
        "Retries",
        "The outbox",
        "Later",
        "2 more in this page",
        "Notes",
        "Drain",
        "Outbox",
    ] {
        input.press_key("ArrowDown").await.unwrap();
        until(&page, &format!("{SELECTED} === '{label}'")).await;
    }
    input.press_key("ArrowUp").await.unwrap();
    until(&page, &format!("{SELECTED} === 'Drain'")).await;

    // Escape clears the box and brings the tree back.
    input.press_key("Escape").await.unwrap();
    until(
        &page,
        "document.querySelector('#devpage-nav > ul').checkVisibility()",
    )
    .await;
    assert_eq!(
            text(&page, &format!("[document.querySelector('{SEARCH_INPUT}').value, document.querySelectorAll('#devpage-nav .hit').length, !!document.querySelector('#devpage-nav .search .clear')].join(' ')")).await,
            " 0 false"
        );

    // The clear button clears it as Escape does, leaving the focus in the box.
    input.type_str("outbox").await.unwrap();
    until(
        &page,
        "document.querySelectorAll('#devpage-nav .hit').length > 0",
    )
    .await;
    page.evaluate("document.querySelector('#devpage-nav .search .clear').click(); 0")
        .await
        .unwrap();
    until(
        &page,
        "document.querySelector('#devpage-nav > ul').checkVisibility()",
    )
    .await;
    assert_eq!(
            text(&page, &format!("[document.querySelector('{SEARCH_INPUT}').value, document.querySelectorAll('#devpage-nav .hit').length, document.activeElement === document.querySelector('{SEARCH_INPUT}'), !!document.querySelector('#devpage-nav .search .clear')].join(' ')")).await,
            " 0 true false"
        );

    // Without a page found by its title, its best section is selected first.
    input.type_str("drain").await.unwrap();
    until(
        &page,
        &format!("{ROWS} === 'hit-page Notes|hit-section Drain'"),
    )
    .await;
    assert_eq!(text(&page, SELECTED).await, "Drain");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Whether the first word marked as found is in view, the heading with
/// the id `heading` above it scrolled past.
fn landed_on_word(heading: &str) -> String {
    format!(
            "(() => {{ const mark = document.querySelector('#devpage-page mark.devpage-found');
              if (!mark) return false;
              const word = mark.getBoundingClientRect();
              return word.top > 0 && word.bottom < innerHeight && document.getElementById('{heading}').getBoundingClientRect().bottom < 0;
            }})()"
        )
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_section_result_lands_on_its_first_word_marked_until_escape() {
    let dir = search_folder("search-landing");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    page.evaluate("window.nav = document.getElementById('devpage-nav'); window.marker = 1; 0")
        .await
        .unwrap();
    let input = search_outbox(&page).await;
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    until(&page, &format!("{SELECTED} === 'The outbox'")).await;

    // Enter swaps the page in and lands on the section's first matched
    // word, not its heading; every one of its words in the section is
    // marked — not in code, a widget, the heading or the next section.
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md#the-outbox'",
    )
    .await;
    until(&page, &landed_on_word("the-outbox")).await;
    assert_eq!(text(&page, FOUND).await, "outbox,Outboxes");
    assert_eq!(
            text(&page, "[window.marker, window.nav === document.getElementById('devpage-nav'), document.querySelector('#devpage-nav .search input').value].join(' ')").await,
            "1 true outbox",
            "swapped in, not loaded: the box keeps its text"
        );

    // Scrolled away, Enter lands there again.
    page.evaluate("scrollTo(0, 0); 0").await.unwrap();
    input.press_key("Enter").await.unwrap();
    until(&page, &landed_on_word("the-outbox")).await;
    assert_eq!(text(&page, FOUND).await, "outbox,Outboxes");

    // Escape on the page takes the marks out, leaving the text as it was.
    page.evaluate("document.activeElement.blur(); 0")
        .await
        .unwrap();
    page.find_element("body")
        .await
        .unwrap()
        .press_key("Escape")
        .await
        .unwrap();
    until(&page, &format!("{FOUND} === ''")).await;
    assert_eq!(
            text(&page, "String([...document.querySelectorAll('#devpage-page p')].some((p) => p.textContent === 'Outboxes drain in order.' && p.childNodes.length === 1))").await,
            "true"
        );

    // The next navigation takes them out too: here, the next section.
    input.click().await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    until(&page, &format!("{SELECTED} === 'Later'")).await;
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        &format!("location.hash === '#later' && {FOUND} === 'outbox'"),
    )
    .await;
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.getAttribute('href') === '/docs/retries.md#the-outbox').click(); 0")
            .await
            .unwrap();
    until(
        &page,
        &format!("location.hash === '#the-outbox' && {FOUND} === 'outbox,Outboxes'"),
    )
    .await;
    // On the page shown, the #fragment set lands on the word too.
    until(&page, &landed_on_word("the-outbox")).await;
    page.evaluate("history.back(); 0").await.unwrap();
    until(
        &page,
        &format!("location.hash === '#later' && {FOUND} === ''"),
    )
    .await;

    // A doc's chapter loads its document whole, and lands there too.
    page.evaluate("[...document.querySelectorAll('#devpage-nav a')].find((a) => a.getAttribute('href') === '/docs/notes/notes.yml#drain').click(); 0")
            .await
            .unwrap();
    until(&page, "location.pathname === '/docs/notes/notes.yml' && document.documentElement.dataset.ready === 'true'").await;
    until(&page, &landed_on_word("drain")).await;
    assert_eq!(text(&page, FOUND).await, "outbox");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_title_result_opens_its_page_at_the_top_with_nothing_marked() {
    let dir = search_folder("search-title");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    stub_search(&page, "").await;
    let input = search_outbox(&page).await;
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("ArrowDown").await.unwrap();
    input.press_key("Enter").await.unwrap();
    until(&page, &landed_on_word("the-outbox")).await;

    // Found by its title, the page shown opens at its top.
    input.press_key("Escape").await.unwrap();
    input.type_str("retries").await.unwrap();
    until(&page, &format!("{ROWS} === 'hit-page Retries'")).await;
    assert_eq!(text(&page, SELECTED).await, "Retries");
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md' && scrollY === 0",
    )
    .await;
    assert_eq!(text(&page, FOUND).await, "");

    // From another page too.
    page.evaluate("load('/docs/guide.md', 'push')")
        .await
        .unwrap();
    until(&page, "location.pathname === '/docs/guide.md'").await;
    input.press_key("Enter").await.unwrap();
    until(
        &page,
        "location.pathname + location.hash === '/docs/retries.md' && scrollY === 0",
    )
    .await;
    assert_eq!(text(&page, FOUND).await, "");
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn slash_and_command_k_focus_the_sidebar_search() {
    let dir = search_folder("search-keys");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    let focused = format!("document.activeElement === document.querySelector('{SEARCH_INPUT}')");
    assert_eq!(text(&page, &format!("String({focused})")).await, "false");
    page.find_element("body")
        .await
        .unwrap()
        .press_key("/")
        .await
        .unwrap();
    until(&page, &focused).await;
    assert_eq!(
        text(
            &page,
            &format!("document.querySelector('{SEARCH_INPUT}').value")
        )
        .await,
        "",
        "the / focuses, and isn't typed"
    );
    for modifier in ["metaKey", "ctrlKey"] {
        page.evaluate("document.activeElement.blur(); 0")
            .await
            .unwrap();
        assert_eq!(text(&page, &format!("String({focused})")).await, "false");
        page.evaluate(format!(
                "document.body.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'k', {modifier}: true, bubbles: true }})); 0"
            ))
            .await
            .unwrap();
        until(&page, &focused).await;
    }
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// Drives chrome-headless-shell; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_slow_earlier_search_never_replaces_a_later_ones_results() {
    let dir = search_folder("search-stale");
    let (mut browser, page) = open(&dir, "/docs/guide.md").await;
    desktop(&page).await;
    // The search for `retries` answers late, and ignores being aborted.
    stub_search(&page, "retries").await;
    let input = page.find_element(SEARCH_INPUT).await.unwrap();
    input.click().await.unwrap();
    input.type_str("retries").await.unwrap();
    until(&page, "window.asked.includes('retries')").await;
    input.press_key("Escape").await.unwrap();
    search_outbox(&page).await;
    // The earlier search's answer comes only now, after the later one's.
    eval::<bool>(&page, "(release(), true)").await;
    until(&page, "window.answered.includes('retries')").await;
    testing::next_frames(&page).await;
    assert!(text(&page, ROWS).await.starts_with("hit-page Outbox|"));
    browser.close().await.ok();
    std::fs::remove_dir_all(&dir).ok();
}
