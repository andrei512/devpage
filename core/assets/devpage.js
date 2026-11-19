// Mounts widgets, and keeps the page live: when a watched file changes, the
// page re-fetches itself and swaps its content in place.
//
// Browsers allow only six open connections per host over plain HTTP, and the
// live-update stream holds one for as long as it is open. So a tab keeps a
// single stream however many pages it visits — links inside the document swap
// the page in place instead of loading a new one — and a hidden tab closes its
// stream, reopening it (and catching up) when it is shown again.

const print = document.currentScript.hasAttribute("data-print");
// Comments are off in print mode and on a page served with `--no-comments`:
// no threads, no toggle, no way to start one.
const comments = !print && !document.currentScript.hasAttribute("data-no-comments");
// The prefix every address of this document is under: `/d/<id>/v/<n>` for a
// version in server mode, empty everywhere else. The page asks its server's
// endpoints under it, and names a page to its server by its address without
// it — the path the server sees.
const mount = document.documentElement.dataset.mount ?? "";
const unmounted = (address) => (address.startsWith(`${mount}/`) ? address.slice(mount.length) : address);

// ---- Widgets --------------------------------------------------------------
// The server leaves each widget as a placeholder carrying its name, its props'
// source, its own source (`data-widget-source`) and its content rendered as
// Markdown — its Mermaid diagrams drawn already. Each placeholder is mounted
// with React, on load and in every page swapped in.

const widgetModules = {};

// Widgets render other widgets by name; compiled widgets look them up here.
globalThis.__devpageWidget = (name) => widgetModules[name]?.default ?? (() => { throw new Error(`<${name}> is not a widget`); });

const widgetUrls = {};

// Why a widget's module didn't load — a compile error in it or in a helper it
// imports — so the widget's place shows the reason.
const widgetLoadErrors = {};

// Loads a widget's module, again when its address changed — an edited widget
// or helper gives the folder a new version — and puts its exported CSS in the
// page once. Then does the same for the widgets it renders by name, which it
// lists, and theirs in turn, whether or not its own address changed: an edited
// widget rendered by name inside an unchanged one has a new address while the
// outer one keeps its own. `seen` stops a cycle of widgets rendering each other.
async function loadWidget(name, seen = new Set()) {
  seen.add(name);
  const url = (window.devpageWidgets || {})[name];
  if (!url) throw new Error(`<${name}> is not a widget`);
  if (widgetUrls[name] !== url) {
    try {
      widgetModules[name] = await import(url);
    } catch (error) {
      // An edit that broke it shows the error, not the last version that worked.
      delete widgetModules[name];
      delete widgetUrls[name];
      throw error;
    }
    widgetUrls[name] = url;
    document.head.querySelector(`style[data-widget="${name}"]`)?.remove();
    const css = widgetModules[name].css;
    if (css) {
      const style = document.createElement("style");
      style.dataset.widget = name;
      style.textContent = css;
      document.head.append(style);
    }
  }
  const module = widgetModules[name];
  await Promise.all((module.__devpageUses || []).filter((used) => !seen.has(used)).map((used) => loadWidget(used, seen)
    .then(() => { delete widgetLoadErrors[used]; })
    .catch((error) => { widgetLoadErrors[used] = error; })));
  return module;
}

// Every React root mounted, by the element it renders into; and for each
// outermost one, what it was mounted from (`keyOf`).
const roots = new Map();
const keys = new Map();

// The content's outermost widget placeholders.
function placeholdersIn(root) {
  return [...root.querySelectorAll("[data-widget]:not(style)")].filter((el) => !el.parentElement.closest("[data-widget]"));
}

// What an outermost placeholder mounts from: its HTML as the server sent it,
// and the address of every widget it can render — those it names, nested
// ones included, and those they render by name. An address changes exactly
// when its compiled module would, so the same key mounts the same thing.
function keyOf(placeholder) {
  const names = new Set([placeholder, ...placeholder.querySelectorAll("[data-widget]")].map((el) => el.dataset.widget));
  for (const name of names) for (const used of widgetModules[name]?.__devpageUses ?? []) names.add(used);
  const urls = window.devpageWidgets || {};
  return [placeholder.outerHTML, ...[...names].sort().map((name) => `${name} ${urls[name] ?? ""}`)].join("\n");
}

// Unmounts every root no longer in the page, which stops its effects and
// timers and lets the design script close the streams of designs in it.
function unmountGone() {
  for (const [el, root] of roots) {
    if (el.isConnected) continue;
    roots.delete(el);
    keys.delete(el);
    try {
      root.unmount();
    } catch (error) {
      console.error(error);
    }
  }
}

async function mountWidgets(root) {
  const placeholders = placeholdersIn(root);
  if (!placeholders.length) return;
  // Only the widgets this content names, nested ones included, and those they
  // render by name, transitively (`loadWidget`) — loaded alongside React, not
  // after it.
  const names = new Set([...root.querySelectorAll("[data-widget]")].map((el) => el.dataset.widget));
  const [React, { createRoot }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    ...[...names].map((name) => loadWidget(name)
      .then(() => { delete widgetLoadErrors[name]; })
      .catch((error) => { widgetLoadErrors[name] = error; })),
  ]);
  // Keyed before mounting, while each holds what the server sent.
  for (const el of placeholders) keys.set(el, keyOf(el));

  class Boundary extends React.Component {
    constructor(props) { super(props); this.state = { error: null }; }
    static getDerivedStateFromError(error) { return { error }; }
    render() {
      if (!this.state.error) return this.props.children;
      return React.createElement("pre", { className: "widget-error" }, `<${this.props.name}>: ${this.state.error.message}`);
    }
  }

  const attributes = (el) => {
    const props = {};
    for (const { name, value } of el.attributes) {
      if (name === "class") props.className = value;
      else if (name === "for") props.htmlFor = value;
      else if (name === "style") props.style = Object.fromEntries(value.split(";").filter(Boolean).map((d) => {
        const [k, ...v] = d.split(":");
        return [k.trim().replace(/-([a-z])/g, (_, c) => c.toUpperCase()), v.join(":").trim()];
      }));
      else props[name] = value;
    }
    return props;
  };

  // Turns rendered content into React elements, so a widget's children can
  // hold other widgets and a Tabs can read its Tabs' titles.
  const toReact = (node, key) => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent;
    if (node.nodeType !== Node.ELEMENT_NODE) return null;
    if (node.dataset.widget) return widgetElement(node, key);
    const props = { ...attributes(node), key };
    if (!node.querySelector("[data-widget]")) {
      if (["BR", "HR", "IMG", "INPUT"].includes(node.tagName)) return React.createElement(node.tagName.toLowerCase(), props);
      return React.createElement(node.tagName.toLowerCase(), { ...props, dangerouslySetInnerHTML: { __html: node.innerHTML } });
    }
    return React.createElement(node.tagName.toLowerCase(), props, ...[...node.childNodes].map((child, i) => toReact(child, i)));
  };

  const widgetElement = (node, key) => {
    const name = node.dataset.widget;
    let props;
    try {
      props = new Function(`return (${node.dataset.props || "{}"});`)();
    } catch (error) {
      return React.createElement("pre", { className: "widget-error", key }, `<${name}>: ${error.message}`);
    }
    const children = [...node.childNodes].map((child, i) => toReact(child, i)).filter((c) => c !== null && c !== "\n");
    const component = widgetModules[name]?.default;
    if (!component) return React.createElement("pre", { className: "widget-error", key }, `<${name}>: ${widgetLoadErrors[name]?.message ?? "could not be loaded"}`);
    // Nested widgets are plain elements, so a parent reads their props — a
    // Tabs reads each Tab's title; the outermost one catches what they throw.
    return React.createElement(component, { ...props, key, source: node.dataset.widgetSource ?? "", sourceHtml: node.dataset.widgetSourceHtml, html: node.innerHTML, children: children.length ? children : undefined });
  };

  const mount = (els) => Promise.all(els.map((el) => new Promise((resolve) => {
    el.dataset.mounted = "";
    const element = React.createElement(Boundary, { name: el.dataset.widget }, widgetElement(el, 0));
    const root = createRoot(el);
    roots.set(el, root);
    root.render(React.createElement(Mounted, { onMount: resolve }, element));
  })));
  await mount(placeholders);
  // A widget that renders its content as HTML — Steps and Cards draw their
  // items from it — puts the placeholders of widgets inside that content back
  // on the page unmounted. Mount those too, outermost first, until none is left.
  for (let depth = 0; depth < 8; depth++) {
    // (A widget's own <style data-widget> in the head is not a placeholder.)
    const left = [...root.querySelectorAll("[data-widget]:not([data-mounted]):not(style)")]
      .filter((el) => !el.parentElement.closest("[data-widget]:not([data-mounted])"));
    if (!left.length) break;
    await mount(left);
  }

  function Mounted({ onMount, children }) {
    React.useEffect(() => { onMount(); }, []);
    return children;
  }
}

// ---- The sidebar ----------------------------------------------------------
// A site's sidebar is the DevpageSidebar widget. The server renders its markup
// and its tree as JSON (`#devpage-sidebar`); React takes the sidebar over from
// that markup — hydrating it, so not a node changes — and it stays mounted
// for as long as the tab shows the site: each page swapped in gives it that
// page's tree and address, in the swap's commit.

// What the sidebar shows, set in the commit; null until it has mounted, and
// when it couldn't.
let showInSidebar = null;

const sidebarData = (doc) => {
  const script = doc.getElementById("devpage-sidebar");
  return script ? JSON.parse(script.textContent) : null;
};

async function mountSidebar() {
  const nav = document.querySelector("#devpage-nav[data-sidebar]");
  const data = sidebarData(document);
  if (!nav || !data) return;
  const [React, { hydrateRoot, flushSync }, module] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    loadWidget("DevpageSidebar"),
  ]);
  await new Promise((resolve) => {
    function Sidebar() {
      const [shown, setShown] = React.useState(data);
      React.useEffect(() => {
        showInSidebar = (next) => flushSync(() => setShown(next));
        resolve();
      }, []);
      return React.createElement(module.default, shown);
    }
    hydrateRoot(nav, React.createElement(Sidebar), {
      onRecoverableError: (error) => console.error("devpage: the sidebar's markup and DevpageSidebar differ", error),
    });
  });
}

// The sidebar is mounting: a swap waits for it, so it never replaces nodes
// React is taking over.
const sidebarMounted = print ? Promise.resolve() : mountSidebar().catch((error) => {
  console.error("devpage: the sidebar stays as the server drew it", error);
});

// ---- Places ---------------------------------------------------------------
// A document's `places` name widgets shown outside its pages' text, on every
// page: beside the theme selector (`navbar`), in the panel at the window's
// right edge (`panel`), and floating at the middle of its bottom edge
// (`float`). Each is mounted once and stays mounted while the tab shows the
// document — a page swapped in leaves it be — and is mounted again when its
// module, or one it renders by name, changed. Never in print, nor on a
// design's own page.

const PLACES = ["navbar", "panel", "float"];

// Each place's element, once it has held a widget.
const placeHosts = {};

// Puts a place's element where it shows: the navbar's at the start of the
// navbar's end — or of the corner, on a page with no navbar — and the others
// in the body.
function seatPlace(place) {
  const host = placeHosts[place];
  if (!host) return;
  if (place !== "navbar") {
    if (!host.isConnected) document.body.append(host);
    return;
  }
  let end = document.querySelector("#devpage-navbar .navbar-end") ?? document.getElementById("devpage-corner");
  if (!end) {
    end = document.createElement("div");
    end.id = "devpage-corner";
    document.body.prepend(end);
  }
  if (host.parentElement !== end) end.prepend(host);
}

// Whether a widget opened or closed the panel (`page.panel`); until one
// does, it is open where there is room for it beside the page.
let panelOpen = null;

// The panel is beside the page on a wide window, and over its right edge on
// a narrow one and on a deck.
function layoutPanel() {
  const wide = !document.body.classList.contains("slides") && matchMedia("(min-width: 1100px)").matches;
  const open = !!placeHosts.panel?.isConnected && (panelOpen ?? wide);
  document.body.classList.toggle("with-panel", open);
  document.body.classList.toggle("panel-beside", open && wide);
  layoutComments();
}
window.addEventListener("resize", layoutPanel);

async function place() {
  const places = window.devpagePlaces || {};
  const fresh = [];
  for (const name of PLACES) {
    const widgets = places[name] ?? [];
    if (!widgets.length) {
      placeHosts[name]?.remove();
      delete placeHosts[name];
      continue;
    }
    if (!placeHosts[name]) {
      placeHosts[name] = document.createElement(name === "panel" ? "aside" : "div");
      placeHosts[name].id = `devpage-place-${name}`;
    }
    const host = placeHosts[name];
    seatPlace(name);
    widgets.forEach((widget, i) => {
      const item = document.createElement("div");
      item.className = "placed";
      const placeholder = document.createElement("div");
      placeholder.dataset.widget = widget;
      placeholder.dataset.props = "{}";
      item.append(placeholder);
      const had = host.children[i];
      if (had && keys.get(had.firstElementChild) === keyOf(placeholder)) return;
      if (had) had.replaceWith(item);
      else host.append(item);
      fresh.push(item);
    });
    while (host.children.length > widgets.length) host.lastElementChild.remove();
  }
  unmountGone();
  layoutPanel();
  await Promise.all(fresh.map((item) => mountWidgets(item)));
}

// Shows the places the page names now, one pass after another.
let placing = Promise.resolve();
function showPlaces() {
  if (print || document.body.classList.contains("design-page")) return placing;
  placing = placing.then(place).catch((error) => console.error("devpage: a place couldn't be shown", error));
  return placing;
}

// Marks every link and image the page's problems name, where it stands — in
// the text and in diagrams. The problems bar lists their targets as written.
function markBroken(root) {
  const bar = root.querySelector(".problems[data-broken]");
  const broken = new Set(bar ? JSON.parse(bar.dataset.broken) : []);
  for (const element of root.querySelectorAll("a[href], img[src]")) {
    const target = element.getAttribute(element.tagName === "IMG" ? "src" : "href");
    element.classList.toggle("broken", broken.has(target));
  }
}

// A diagram fits the width it is shown in. On a narrow screen one wider than
// that is shrunk no further than DIAGRAM_SCALE of its own size, so its text
// stays readable, and scrolls sideways in its box.
const DIAGRAM_SCALE = 0.65;
const narrow = matchMedia("screen and (max-width: 860px)");
// A slide's diagrams fill the stage instead, and one out of sight (a hidden
// tab) has no width to fit and keeps its size.
function fitDiagrams(root = document) {
  for (const svg of root.querySelectorAll("article .diagram > svg[width]")) {
    const own = parseFloat(svg.getAttribute("width"));
    const room = svg.parentElement.clientWidth - 32;
    if (room <= 0) continue;
    svg.style.width = narrow.matches && own > room ? `${Math.max(room, own * DIAGRAM_SCALE)}px` : "";
  }
}
// A screen grown wide shows the sidebar beside the page: the navigator is
// closed, so nothing waits for a touch that would close it.
narrow.addEventListener("change", () => {
  fitDiagrams();
  if (!narrow.matches) setNavigator(false);
});

// Embedded designs are drawn by the design script, loaded only once a page
// has one: it starts every canvas not yet drawn and stops those gone — on
// every page after that, so leaving the last design closes its stream.
let designScript = null;
function drawDesigns() {
  if (!designScript && !document.querySelector("canvas.design")) return;
  // build.rs rewrites the address to design.js's content address.
  designScript ??= import("/__devpage/design.js");
  designScript
    .then((designs) => designs.scan())
    .catch((error) => {
      // The design script itself didn't load: each canvas says so.
      console.error("devpage: the design script didn't load", error);
      for (const canvas of document.querySelectorAll("canvas.design:not([data-failed])")) {
        canvas.dataset.failed = "true";
        const note = document.createElement("p");
        note.className = "design-failed";
        note.textContent = "This design can't be drawn: the design script didn't load.";
        canvas.before(note);
      }
    });
}

// A widget can put a design on the page, or take one off, after it mounted —
// `Tabs` shows only the open tab — so each canvas that comes or goes starts or
// stops its design then, once per batch of changes.
let designsChanged = false;
const holdsDesign = (node) =>
  node.nodeType === Node.ELEMENT_NODE && (node.matches("canvas.design") || node.querySelector("canvas.design"));
new MutationObserver((records) => {
  if (designsChanged) return;
  for (const { addedNodes, removedNodes } of records) {
    if (![...addedNodes, ...(designScript ? removedNodes : [])].some(holdsDesign)) continue;
    designsChanged = true;
    queueMicrotask(() => {
      designsChanged = false;
      drawDesigns();
    });
    return;
  }
}).observe(document.body, { childList: true, subtree: true });

// What a swap keeps of the page shown. Each outermost widget whose key — what
// it was mounted from — matches one in the fresh page stays as it is, state
// and all: the fresh placeholder gives way to a marker, and the old element
// takes the marker's place when the page is committed. Each embedded design
// outside a widget keeps its canvas — its drawing and its stream — when the
// fresh page embeds the same design. Kept elements never leave this document:
// a canvas moved between documents may lose its context.
function keepUnchanged(page, staging) {
  const mounted = [...keys.keys()].filter((el) => page.contains(el));
  const kept = [];
  for (const placeholder of placeholdersIn(staging)) {
    const key = keyOf(placeholder);
    const at = mounted.findIndex((old) => keys.get(old) === key);
    if (at < 0) continue;
    const marker = document.createElement("template");
    placeholder.replaceWith(marker);
    kept.push([marker, mounted.splice(at, 1)[0]]);
  }
  // Called in the commit, once the fresh page is mounted.
  return () => {
    const outside = (embed) => !embed.closest("[data-widget]");
    const designs = [...page.querySelectorAll(".design-embed")].filter(outside);
    for (const embed of [...staging.querySelectorAll(".design-embed")].filter(outside)) {
      // By selector: a problems bar can sit before the canvas.
      const src = embed.querySelector("canvas.design")?.getAttribute("src");
      const at = designs.findIndex((old) => old.querySelector("canvas.design")?.getAttribute("src") === src);
      if (at >= 0) embed.replaceWith(designs.splice(at, 1)[0]);
    }
    for (const [marker, old] of kept) marker.replaceWith(old);
  };
}

// The frame a body was rendered in: its classes, less `show-comments`, which
// this script adds while the comments are shown and no fetched page carries;
// and the devpage build that rendered it, so a page from a new build — a new
// page script, a new stylesheet — is loaded whole.
// The body's classes this script adds itself, which no served page carries.
const OWN_CLASSES = new Set(["show-comments", "with-versions", "with-version-banner", "with-panel", "panel-beside", "picking"]);

function frameOf(body) {
  const classes = [...body.classList].filter((name) => !OWN_CLASSES.has(name)).sort();
  return [...classes, `build:${body.dataset.build}`].join(" ");
}

// The page shown: its address and the ETag it was served with. A fetch that
// finds the same ETag at the same address has nothing to swap. The page this
// script was loaded with is in the browser's cache already: its ETag comes
// from there, without asking the server.
const pageKey = (url) => { const u = new URL(url, location.href); u.hash = ""; return u.href; };
let shown = { url: pageKey(location.href), etag: null };
const seeded = print ? Promise.resolve() : fetch(location.href, { cache: "only-if-cached", mode: "same-origin" })
  .then((response) => { if (response.ok && shown.etag === null) shown.etag = response.headers.get("etag"); })
  .catch(() => {});

// Starts what a fetched page names in its head — its modules and the design
// engine's wasm — here, so they download together before mounting asks for
// them. A link this page has already is not added twice.
function adoptPreloads(next) {
  for (const link of next.head.querySelectorAll('link[rel="modulepreload"], link[rel="preload"], link[rel="stylesheet"]')) {
    const href = link.getAttribute("href");
    const known = [...document.head.querySelectorAll(`link[rel="${link.rel}"]`)].some((have) => have.getAttribute("href") === href);
    if (!known) document.head.append(document.importNode(link, true));
  }
}

// ---- History --------------------------------------------------------------
// The path shown — its pathname and query, not its #fragment: moving within
// a page (a slide, a heading) is the browser's alone and fetches nothing.
// The script restores the scroll itself: each history entry keeps the reader's
// `scrollY` from when they left it.

const pathOf = (url) => { const u = new URL(url, location.href); return u.pathname + u.search; };
let shownPath = pathOf(location.href);
if (!print) history.scrollRestoration = "manual";

function remember() {
  history.replaceState({ ...history.state, scrollY }, "");
}

// Scrolls to the element a #fragment names; false when there is none.
function toFragment(hash) {
  const id = decodeURIComponent(hash.slice(1));
  const target = id ? document.getElementById(id) : null;
  target?.scrollIntoView();
  return !!target;
}

// ---- Swapping a page in ---------------------------------------------------
// One path for every swap — a live update, a tab shown again, a link, back and
// forward: fetch, stage, commit. The fresh page is mounted off-screen, inside
// a hidden copy of the page's frame in this document, so widgets that measure
// themselves can; the commit then puts it in place in one task, so no frame
// ever shows a page half built. One load runs at a time: a newer one makes the
// older one stale, and live updates that come during a load make one more
// load after it.

let loads = 0;
let controller = null;
let running = null;
let again = false;

// Loads `url` and swaps it in. `how` is "live" (the page shown, again: the
// scroll stays), "push" (a link: pushed onto the history, then its #fragment
// or the top) or "traverse" (back or forward: the history is there already,
// and so is the scroll the reader left it at). Resolves true once swapped in
// or found unchanged, false when it can't be (the caller lets the browser load
// it), "navigating" when the page is loaded whole, and "stale" when a newer
// load took over.
function load(url, how = "live") {
  const generation = ++loads;
  controller?.abort();
  controller = new AbortController();
  const loading = swap(new URL(url, location.href), how, generation, controller.signal)
    .catch((error) => {
      if (error.name === "AbortError") return "stale";
      console.error("devpage: the page couldn't be swapped in", error);
      return false;
    })
    .finally(() => {
      if (running !== loading) return;
      running = null;
      if (again) {
        again = false;
        load(location.href);
      }
    });
  running = loading;
  return loading;
}

// A live update or a catch-up: the page shown, loaded again — after the load
// in flight, if there is one, however many come during it.
function refresh() {
  if (running) again = true;
  else load(location.href);
}

async function swap(url, how, generation, signal) {
  const stale = () => generation !== loads;
  await seeded;
  const response = await fetch(url, { cache: "no-cache", signal });
  if (stale()) return "stale";
  if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) return false;
  const etag = response.headers.get("etag");
  if (etag && etag === shown.etag && pageKey(url) === shown.url) return true;
  const next = new DOMParser().parseFromString(await response.text(), "text/html");
  if (stale()) return "stale";
  // A different frame — a fixed document file replacing its problem page, or
  // a document that changed kind — can't be swapped in part: load it whole.
  if (frameOf(next.body) !== frameOf(document.body)) {
    if (how === "push") location.assign(url);
    else location.replace(url);
    return "navigating";
  }
  adoptPreloads(next);
  // The fresh page names its widgets' current versions.
  // It names its places and its extensions too, when it has any.
  const list = [...next.scripts].find((script) => script.textContent.startsWith("window.devpageWidgets"));
  if (list) {
    window.devpagePlaces = {};
    window.devpageExtensions = {};
    new Function(list.textContent)();
  }

  // Stage: the fresh content in a hidden copy of the page's frame.
  const page = document.getElementById("devpage-page");
  const fresh = next.getElementById("devpage-page");
  const staging = document.createElement(page.tagName);
  staging.className = fresh?.className ?? page.className;
  staging.inert = true;
  staging.style.cssText = `position: absolute; top: 0; left: 0; width: ${page.getBoundingClientRect().width}px; visibility: hidden; pointer-events: none;`;
  if (fresh) staging.append(...fresh.childNodes);
  document.body.append(staging);
  const keep = keepUnchanged(page, staging);
  showSlide(staging);
  const drop = () => {
    staging.remove();
    unmountGone();
  };
  try {
    await mountWidgets(staging);
  } catch (error) {
    drop();
    throw error;
  }
  await sidebarMounted;
  if (stale()) {
    drop();
    return "stale";
  }

  // Commit, in one task.
  const scroll = scrollY;
  keep();
  page.className = staging.className;
  page.replaceChildren(...staging.childNodes);
  staging.remove();
  // The sidebar stays mounted and is given the fresh page's tree; a doc's
  // outline, the navbar, and a sidebar React couldn't take over are replaced.
  const sidebar = sidebarData(next);
  if (sidebar && showInSidebar) {
    showInSidebar(sidebar);
    document.getElementById("devpage-nav").dataset.sidebar = sidebar.document;
  }
  for (const id of ["devpage-nav", "devpage-navbar"]) {
    if (id === "devpage-nav" && sidebar && showInSidebar) continue;
    const part = next.getElementById(id);
    if (part) document.getElementById(id)?.replaceChildren(...part.childNodes);
  }
  // The navbar's place goes back into the fresh navbar, still mounted.
  seatPlace("navbar");
  shown = { url: pageKey(url), etag };
  if (how === "push") history.pushState(null, "", url);
  shownPath = pathOf(url);
  // The navigator's copy of the navbar's links marks the page shown, as the
  // navbar does.
  const links = next.querySelector(".navigator-links");
  if (links) document.querySelector(".navigator-links")?.replaceChildren(...links.childNodes);
  if (how !== "live") setNavigator(false);
  applyTheme();
  applyCommentMode();
  document.title = next.title;
  showSlide();
  markBroken(page);
  fitDiagrams(page);
  if (how === "live") scrollTo(scrollX, scroll);
  else if (how === "traverse" && typeof history.state?.scrollY === "number") scrollTo(0, history.state.scrollY);
  else if (!toFragment(url.hash)) scrollTo(0, 0);
  unmountGone();
  drawDesigns();
  loadComments();
  showPlaces();
  showMarks();
  emit("change", shownPage.current());
  return true;
}

// Links to other Markdown pages of this document load in place — in the text
// and in diagrams, where a Mermaid `click node href "…"` line becomes an SVG
// link. An SVG link's `href` property is not a string, so the attribute is
// resolved against the page instead. A search result's link carries the words
// it matched (`data-found`): where it opens, they are marked (`land`) — after
// a whole load too, handed over in the tab's session storage.
document.addEventListener("click", async (event) => {
  const link = event.target.closest("a[href]");
  if (!link || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
  const url = new URL(link.getAttribute("href"), location.href);
  if (url.origin !== location.origin) return;
  const found = link.dataset.found ? JSON.parse(link.dataset.found) : null;
  clearFound();
  if (!url.pathname.endsWith(".md")) {
    if (found) handOver(url, found);
    return;
  }
  event.preventDefault();
  remember();
  if (pathOf(url) === shownPath) {
    // The #fragment shown already is scrolled to again: setting it is no move.
    if (url.hash && url.hash === location.hash) toFragment(url.hash);
    else if (url.hash) location.hash = url.hash;
    else if (found) {
      // A page's own result opens it at its top.
      if (url.href !== location.href) history.pushState(null, "", url);
      scrollTo(0, 0);
    }
    if (found) land(url, found);
    return;
  }
  const loaded = await load(url, "push");
  if (found && loaded === true) land(url, found);
  if (found && (loaded === false || loaded === "navigating")) handOver(url, found);
  if (loaded === false) location.href = url;
});

// Back and forward: within the page shown, only the slide and the scroll
// move; to another page, it is swapped in. Either way, it is a move away from
// the words marked.
window.addEventListener("popstate", () => {
  if (location.href !== foundAt) clearFound();
  if (pathOf(location.href) !== shownPath) {
    load(location.href, "traverse");
    return;
  }
  showSlide();
  if (typeof history.state?.scrollY === "number") scrollTo(0, history.state.scrollY);
  else if (location.hash) toFragment(location.hash);
});

// ---- Landing on a search result -------------------------------------------
// Opening a search result marks the words it matched in its section — from
// the heading its #fragment names to the next heading of any level, the
// heading left out; a deck's slide whole; a chapter's text, or the page's,
// before its first heading when the #fragment names a chapter or nothing —
// and scrolls the first of them to the middle of the window. A result with
// no words, a page's own, marks nothing. The marks stay until Escape or the
// next navigation.

// What marking never enters: code, diagrams, designs, widgets' own DOM.
const UNMARKED = "pre, code, svg, canvas, script, style, [data-widget], .design-embed, .problems, mark.devpage-found";
const HEADING = /^H[1-6]$/;

// The address the words were marked at; a move to another takes them out.
let foundAt = null;

function land(url, words) {
  clearFound();
  foundAt = url.href;
  const page = document.getElementById("devpage-page");
  if (!page || !words.length) return;
  markFound(sectionText(page, url.hash), words);
  if (!page.classList.contains("deck")) firstFound()?.scrollIntoView({ block: "center" });
}

const firstFound = () => document.querySelector("#devpage-page mark.devpage-found");

// The text nodes of the section a #fragment names, outside what marking
// never enters.
function sectionText(page, hash) {
  const deck = page.classList.contains("deck");
  let root = page;
  let start = null;
  if (deck) {
    const slides = [...page.children].filter((el) => el.classList.contains("slide"));
    root = slides[Math.min(Math.max(parseInt(hash.slice(1)) || 1, 1), slides.length) - 1];
  } else {
    const target = hash.length > 1 ? document.getElementById(decodeURIComponent(hash.slice(1))) : null;
    if (target && HEADING.test(target.tagName)) start = target;
    else if (target && page.contains(target)) root = target;
  }
  if (!root) return [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.nodeType === Node.ELEMENT_NODE && node.matches(UNMARKED) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  const texts = [];
  let inside = !start;
  let heading = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.nodeType === Node.ELEMENT_NODE) {
      if (deck || !HEADING.test(node.tagName)) continue;
      if (inside) break;
      inside = node === start;
      heading = node;
    } else if (inside && !heading?.contains(node)) {
      texts.push(node);
    }
  }
  return texts;
}

// Wraps every whole-word occurrence of the words, in any case, in the text
// nodes given in `<mark class="devpage-found">`.
function markFound(texts, words) {
  const escaped = words.filter(Boolean).sort((a, b) => b.length - a.length).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!escaped.length) return;
  const pattern = new RegExp(`(?<![\\p{L}\\p{N}_])(?:${escaped.join("|")})(?![\\p{L}\\p{N}_])`, "giu");
  for (const text of texts) {
    // From the last, so `text` keeps the part before each match.
    for (const match of [...text.data.matchAll(pattern)].reverse()) {
      const word = text.splitText(match.index);
      word.splitText(match[0].length);
      const mark = document.createElement("mark");
      mark.className = "devpage-found";
      word.replaceWith(mark);
      mark.append(word);
    }
  }
}

// Takes the marks out, joining each word's text back into the text around it.
function clearFound() {
  foundAt = null;
  for (const mark of document.querySelectorAll("mark.devpage-found")) {
    const word = mark.firstChild;
    if (mark.childNodes.length !== 1 || word.nodeType !== Node.TEXT_NODE) {
      mark.replaceWith(...mark.childNodes);
      continue;
    }
    const before = mark.previousSibling;
    const text = before?.nodeType === Node.TEXT_NODE ? before : word;
    if (text === before) text.appendData(word.data);
    mark.replaceWith(...(text === word ? [word] : []));
    const after = text.nextSibling;
    if (after?.nodeType === Node.TEXT_NODE) {
      text.appendData(after.data);
      after.remove();
    }
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") clearFound();
});

// A result loaded whole — another site's page, a doc's chapter, a deck's
// slide — lands once the new page is ready.
const HANDED_OVER = "devpage-found";

function handOver(url, words) {
  try {
    sessionStorage.setItem(HANDED_OVER, JSON.stringify({ url: url.href, words }));
  } catch {
    // Storage may be blocked: the result opens without its words marked.
  }
}

function landHandedOver() {
  try {
    const handed = JSON.parse(sessionStorage.getItem(HANDED_OVER));
    sessionStorage.removeItem(HANDED_OVER);
    if (handed?.url !== location.href) return;
    land(new URL(handed.url), handed.words);
    // Until the page has loaded, the browser may scroll to its #fragment
    // again: the first word is scrolled to once it has.
    if (document.readyState !== "complete") {
      addEventListener("load", () => requestAnimationFrame(() => {
        if (foundAt === handed.url) firstFound()?.scrollIntoView({ block: "center" });
      }), { once: true });
    }
  } catch {
    // Nothing handed over, or storage is blocked.
  }
}

// ---------------------------------------------------------------------------
// Comments: threads anchored to rendered text, kept by the server in
// .devpage/comments/. Each element in the page with a data-source is one
// Markdown file's text — a page, a doc's chapter or a deck's slide.
//
// One text model serves both ends: a section's text is its text nodes in order
// with every run of whitespace collapsed to one space. A new comment's quote,
// prefix and suffix are cut from that text, and a thread is found again by
// searching the same text, so nested lists, tables and code anchor the same
// way they were quoted.

const sources = () => [...document.querySelectorAll("#devpage-page [data-source]")];
const CONTEXT = 32;

// The section's normalised text, and for each of its characters the text node
// and offset it came from.
function textModel(section) {
  const chars = [];
  let text = "";
  const walker = document.createTreeWalker(section, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.parentElement?.closest(".comment-ui") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  while (walker.nextNode()) {
    const node = walker.currentNode;
    for (let offset = 0; offset < node.data.length; offset++) {
      const white = /\s/.test(node.data[offset]);
      if (white && text.endsWith(" ")) continue;
      text += white ? " " : node.data[offset];
      chars.push({ node, offset });
    }
  }
  return { text, chars };
}

// The index in the model of a DOM boundary point inside the section.
function indexOf(model, container, offset) {
  const point = document.createRange();
  point.setStart(container, offset);
  const index = model.chars.findIndex(({ node, offset: at }) => point.comparePoint(node, at) >= 0);
  return index < 0 ? model.text.length : index;
}

// The anchor for the text between two model indexes, trimmed of spaces.
function anchorFor(model, start, end) {
  while (start < end && model.text[start] === " ") start++;
  while (end > start && model.text[end - 1] === " ") end--;
  return {
    quote: model.text.slice(start, end),
    prefix: model.text.slice(Math.max(0, start - CONTEXT), start),
    suffix: model.text.slice(end, end + CONTEXT),
  };
}

// Where an anchor is in the model now: its quote with its context, else the
// quote alone; null when the quote is gone.
function find(model, anchor) {
  if (!anchor.quote) return null;
  const quote = anchor.quote.replace(/\s+/g, " ");
  const prefix = anchor.prefix.replace(/\s+/g, " ");
  const suffix = anchor.suffix.replace(/\s+/g, " ");
  let at = model.text.indexOf(prefix + quote + suffix);
  if (at >= 0) at += prefix.length;
  else at = model.text.indexOf(quote);
  return at < 0 ? null : [at, at + quote.length];
}

// Wraps the text between two model indexes in marks, one per text node, so
// the page's structure is never moved; `dress` gives each its class and data.
// A diagram's text is SVG, where an HTML element isn't drawn: its mark is a
// `tspan`.
function highlight(model, [start, end], dress) {
  const marks = [];
  let i = start;
  while (i < end) {
    const { node, offset } = model.chars[i];
    let last = i;
    while (last + 1 < end && model.chars[last + 1].node === node) last++;
    const range = document.createRange();
    range.setStart(node, offset);
    range.setEnd(node, model.chars[last].offset + 1);
    const mark = node.parentElement instanceof SVGElement
      ? document.createElementNS("http://www.w3.org/2000/svg", "tspan")
      : document.createElement("mark");
    dress(mark);
    range.surroundContents(mark);
    marks.push(mark);
    i = last + 1;
  }
  return marks;
}

function panel() {
  let aside = document.getElementById("devpage-comments");
  if (!aside) {
    aside = document.createElement("aside");
    aside.id = "devpage-comments";
    aside.className = "comment-ui";
    document.body.append(aside);
  }
  return aside;
}

// Fetches every source's threads, all at once, then marks and lists them in
// one go; a newer call that finishes first wins, so overlapping refreshes
// never mix.
let commentsGeneration = 0;
async function loadComments() {
  if (!comments) return;
  const generation = ++commentsGeneration;
  const fetched = await Promise.all(sources().map(async (section) => {
    const page = unmounted(section.dataset.source);
    const response = await fetch(`${mount}/__devpage/comments?page=${encodeURIComponent(page)}`, { cache: "no-cache" });
    return { section, page, threads: response.ok ? await response.json() : [] };
  }));
  if (generation !== commentsGeneration) return;
  document.querySelectorAll("mark.comment, tspan.comment").forEach((mark) => mark.replaceWith(...mark.childNodes));
  sources().forEach((section) => section.normalize());
  const aside = panel();
  aside.querySelectorAll(".thread:not(.composer), .detached-heading").forEach((card) => card.remove());
  const placed = [];
  const detached = [];
  for (const { section, page, threads } of fetched) {
    if (!section.isConnected) continue;
    for (const thread of threads) {
      const card = threadCard(thread);
      const model = textModel(section);
      // A thread whose text the server found gone from this version stays
      // detached, even where its words happen to appear again.
      const at = thread.detached ? null : find(model, thread.anchor);
      if (!at) {
        detached.push(card);
        continue;
      }
      const marks = highlight(model, at, (mark) => {
        mark.classList.add("comment");
        mark.dataset.thread = thread.id;
      });
      for (const mark of marks) mark.addEventListener("click", () => focusThread(thread.id));
      card.anchorMark = marks[0];
      placed.push(card);
    }
  }
  if (detached.length) {
    const heading = document.createElement("h3");
    heading.className = "detached-heading";
    heading.textContent = "Detached comments";
    aside.prepend(heading, ...detached);
  }
  aside.append(...placed);
  layoutComments();
}

// Where the threads aren't beside the text, a button opens and closes them.
function toggleButton() {
  let button = document.getElementById("devpage-comments-toggle");
  if (!button) {
    button = document.createElement("button");
    button.id = "devpage-comments-toggle";
    button.className = "comment-ui";
    button.addEventListener("click", () => document.body.classList.toggle("show-comments"));
    document.body.append(button);
  }
  return button;
}

// Beside the text on a wide page: each card level with its anchor, pushed down
// so none overlap. Elsewhere — narrow screens, decks — the panel is a list.
function layoutComments() {
  const aside = document.getElementById("devpage-comments");
  if (!aside) return;
  // The panel, where it is beside the page, has the threads' column.
  const beside = !document.body.classList.contains("slides") && !document.body.classList.contains("panel-beside") && matchMedia("(min-width: 1100px)").matches;
  aside.classList.toggle("beside", beside);
  let floor = 0;
  for (const card of aside.children) {
    card.style.top = "";
    if (!beside) continue;
    const top = aside.getBoundingClientRect().top;
    const want = card.anchorMark ? card.anchorMark.getBoundingClientRect().top - top : floor;
    const at = Math.max(want, floor);
    card.style.top = `${at}px`;
    floor = at + card.offsetHeight + 12;
  }
  aside.style.minHeight = beside ? `${floor}px` : "";
  const count = aside.querySelectorAll(".thread:not(.composer)").length;
  const button = toggleButton();
  button.textContent = `Comments (${count})`;
  button.classList.toggle("shown", !beside && count > 0);
}
window.addEventListener("resize", layoutComments);

function focusThread(id) {
  document.body.classList.add("show-comments");
  const card = document.querySelector(`#devpage-comments .thread[data-thread="${CSS.escape(id)}"]`);
  card?.scrollIntoView({ block: "nearest" });
  card?.querySelector("textarea")?.focus();
}

function messageLine(message) {
  const p = document.createElement("p");
  const author = document.createElement("b");
  author.textContent = message.author;
  const time = document.createElement("time");
  time.textContent = message.at.slice(0, 16).replace("T", " ");
  p.append(author, " ", time, document.createElement("br"), message.text);
  return p;
}

function threadCard(thread) {
  const card = document.createElement("div");
  card.className = "thread";
  card.dataset.thread = thread.id;
  const quote = document.createElement("blockquote");
  quote.textContent = thread.anchor.quote;
  card.append(quote, ...thread.thread.map(messageLine));
  const resolve = document.createElement("button");
  resolve.textContent = "Resolve";
  resolve.addEventListener("click", () => send("resolve", { id: thread.id }, card));
  card.append(editor("Reply", (text) => send("reply", { id: thread.id, text }, card), [resolve]));
  return card;
}

// A text box with a submit button: ⌘/Ctrl-Enter submits, Escape cancels.
function editor(label, submit, extra = [], cancel = null) {
  const form = document.createElement("form");
  form.className = "comment-editor";
  const box = document.createElement("textarea");
  box.rows = 2;
  box.placeholder = label === "Reply" ? "Reply" : "Comment";
  const ok = document.createElement("button");
  ok.type = "submit";
  ok.textContent = label;
  const error = document.createElement("p");
  error.className = "comment-error";
  form.append(box, error, ok, ...extra);
  if (cancel) {
    const no = document.createElement("button");
    no.type = "button";
    no.textContent = "Cancel";
    no.addEventListener("click", cancel);
    form.append(no);
  }
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const text = box.value.trim();
    if (!text) return;
    error.textContent = (await submit(text)) ?? "";
    if (!error.textContent) box.value = "";
  });
  box.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) form.requestSubmit();
    if (event.key === "Escape" && cancel) cancel();
    event.stopPropagation();
  });
  return form;
}

// Sends a write; returns the server's refusal as text, or nothing.
async function send(action, body, card) {
  const response = await fetch(`${mount}/__devpage/comments${action ? `/${action}` : ""}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const reason = await response.text();
    if (card && action === "resolve") card.querySelector(".comment-error").textContent = reason;
    return reason || `${response.status}`;
  }
  loadComments();
}

// A new comment on an anchor: a card at the top of the panel until it is sent
// or cancelled.
function compose(section, anchor, near) {
  document.querySelectorAll(".comment-button").forEach((button) => button.remove());
  document.querySelector("#devpage-comments .composer")?.remove();
  const aside = panel();
  const card = document.createElement("div");
  card.className = "thread composer";
  const quote = document.createElement("blockquote");
  quote.textContent = anchor.quote;
  const close = () => {
    card.remove();
    layoutComments();
  };
  card.append(quote, editor("Comment", async (text) => {
    const refused = await send("", { page: unmounted(section.dataset.source), anchor, text });
    if (!refused) close();
    return refused;
  }, [], close));
  card.anchorMark = near;
  aside.prepend(card);
  document.body.classList.add("show-comments");
  layoutComments();
  card.querySelector("textarea").focus();
}

// A speech bubble, drawn in the text color so it follows the theme.
const commentIcon =
  '<svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true"><path d="M4 3.5h12A1.5 1.5 0 0 1 17.5 5v8a1.5 1.5 0 0 1-1.5 1.5H9l-3.5 3v-3H4A1.5 1.5 0 0 1 2.5 13V5A1.5 1.5 0 0 1 4 3.5Z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/></svg>';

function commentButton(label, title, top, left, onClick) {
  document.querySelectorAll(".comment-button").forEach((button) => button.remove());
  const button = document.createElement("button");
  button.className = label ? "comment-button comment-ui" : "comment-button comment-icon comment-ui";
  button.innerHTML = commentIcon;
  if (label) button.append(label);
  button.setAttribute("aria-label", title);
  button.title = title;
  button.style.cssText = `position:absolute;top:${top}px;left:${left}px`;
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", onClick);
  document.body.append(button);
  return button;
}

// Commenting is opt-in: the comment-mode switch beside the theme toggle turns
// on the two ways to start a comment below, and the browser remembers it.
// Threads already written show either way.
function commentMode() {
  try {
    return localStorage.getItem("devpage-comment-mode") === "on";
  } catch {
    return document.documentElement.dataset.commentMode === "on";
  }
}

function applyCommentMode(on = commentMode()) {
  document.documentElement.dataset.commentMode = on ? "on" : "off";
  for (const button of document.querySelectorAll(".comment-mode")) {
    button.setAttribute("aria-pressed", String(on));
  }
  if (!on) {
    document.querySelectorAll(".comment-button").forEach((button) => button.remove());
    hovered = null;
  }
}

document.addEventListener("click", (event) => {
  if (!event.target.closest?.(".comment-mode")) return;
  const on = !commentMode();
  try {
    localStorage.setItem("devpage-comment-mode", on ? "on" : "off");
  } catch {
    // Storage may be blocked; the switch still holds for this page.
  }
  applyCommentMode(on);
});

// Hovering a paragraph — or a list item, table, code block or diagram —
// offers a comment on the whole of it.
const blocks = "p, li, pre, table, .diagram, blockquote, h1, h2, h3, h4";
let hovered = null;
applyCommentMode();
document.addEventListener("mouseover", (event) => {
  if (!comments || !commentMode() || picking || !getSelection().isCollapsed) return;
  const block = event.target.closest?.(blocks);
  const section = block?.closest("[data-source]");
  if (!block || block === hovered || !section || block.closest(".comment-ui")) return;
  hovered = block;
  const rect = block.getBoundingClientRect();
  // The icon sits just right of the block, level with its first line — or,
  // when the window has no room there, just inside the block's right edge.
  const size = 28;
  const room = document.documentElement.clientWidth - 8;
  const left = rect.right + 10 + size <= room ? rect.right + 10 : Math.max(Math.min(rect.right, room) - size - 4, 0);
  commentButton("", "Comment on this paragraph", scrollY + rect.top, scrollX + left, () => {
    const model = textModel(section);
    const anchor = anchorFor(model, indexOf(model, block, 0), indexOf(model, block, block.childNodes.length));
    if (anchor.quote) compose(section, anchor, block);
  });
});

// Selecting text inside one source offers a comment on it.
document.addEventListener("mouseup", (event) => {
  if (!comments || !commentMode() || picking || event.target.closest?.(".comment-ui")) return;
  setTimeout(() => {
    const selected = selectedText();
    if (!selected) return;
    const { section, anchor, range } = selected;
    const rect = range.getBoundingClientRect();
    commentButton("Comment", "Comment on the selection", scrollY + rect.bottom + 6, scrollX + rect.left, () => {
      getSelection().removeAllRanges();
      compose(section, anchor, range.startContainer.parentElement);
    });
  });
});

// The text selected, when it is inside one source: the source's section, the
// selection's anchor in it, and its range.
function selectedText() {
  const selection = getSelection();
  if (selection.isCollapsed || !selection.rangeCount) return null;
  const range = selection.getRangeAt(0);
  const common = range.commonAncestorContainer;
  const section = (common.nodeType === Node.ELEMENT_NODE ? common : common.parentElement)?.closest("#devpage-page [data-source]");
  if (!section) return null;
  const model = textModel(section);
  const anchor = anchorFor(model, indexOf(model, range.startContainer, range.startOffset), indexOf(model, range.endContainer, range.endOffset));
  return anchor.quote ? { section, anchor, range } : null;
}
// ---------------------------------------------------------------------------

// The live-update stream, open only while the tab is visible.
let stream = null;

function connect() {
  if (stream) return;
  stream = new EventSource(`${mount}/__devpage/events`);
  stream.onmessage = (event) => {
    if (event.data.startsWith("comments ")) {
      if (sources().some((s) => unmounted(s.dataset.source) === event.data.slice(9))) loadComments();
    } else {
      refresh();
      loadVersions();
    }
  };
}

function disconnect() {
  stream?.close();
  stream = null;
}

document.addEventListener("visibilitychange", () => {
  if (print) return;
  if (document.hidden) {
    disconnect();
  } else {
    connect();
    refresh(); // catch up on changes made while hidden
    loadVersions();
  }
});
window.addEventListener("pagehide", () => {
  disconnect();
  // A reload comes back to where the reader was.
  if (!print) remember();
});

// A deck shows one slide at a time, scaled to the window; its number is the
// address's #fragment. `page` is the page shown, or a fresh one being staged.
function showSlide(page = document.getElementById("devpage-page")) {
  if (print || !page?.classList.contains("deck")) return;
  const slides = [...page.children].filter((el) => el.classList.contains("slide"));
  if (!slides.length) return;
  const index = Math.min(Math.max(parseInt(location.hash.slice(1)) || 1, 1), slides.length);
  const scale = Math.min(innerWidth / 1920, innerHeight / 1080);
  const left = (innerWidth - 1920 * scale) / 2, top = (innerHeight - 1080 * scale) / 2;
  slides.forEach((slide, i) => {
    slide.classList.toggle("shown", i === index - 1);
    slide.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
  });
  return { index, count: slides.length };
}

document.addEventListener("keydown", (event) => {
  // Typing, and the browser's own shortcuts (⌘C, ⌘F), are left alone.
  if (event.target.closest?.("input, textarea") || event.metaKey || event.ctrlKey || event.altKey) return;
  const state = showSlide();
  if (!state) return;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  const go = (n) => { location.hash = `#${Math.min(Math.max(n, 1), state.count)}`; };
  if (["ArrowRight", " ", "PageDown", "ArrowDown"].includes(key)) go(state.index + 1);
  else if (["ArrowLeft", "PageUp", "ArrowUp"].includes(key)) go(state.index - 1);
  else if (key === "Home") go(1);
  else if (key === "End") go(state.count);
  else if (key === "c" && comments) document.body.classList.toggle("show-comments");
  else if (key === "f") document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen();
  else return;
  event.preventDefault();
});
// On a touch screen a swipe left or a tap on the right of a slide goes on; a
// swipe right or a tap on its left third goes back. A tap on a link, a
// button, a field, a design or a comment is theirs, and a long press or one
// that leaves text selected is selecting, not turning.
const TAP_MS = 400;
let touchStart = null;
document.addEventListener("touchstart", (event) => {
  touchStart = event.touches.length === 1 ? { x: event.touches[0].clientX, y: event.touches[0].clientY, at: event.timeStamp } : null;
}, { passive: true });
document.addEventListener("touchend", (event) => {
  const start = touchStart;
  touchStart = null;
  if (!start || event.target.closest?.("a, button, input, textarea, select, summary, canvas, .design-embed, mark.comment, tspan.comment, .comment-button, #devpage-comments, #devpage-corner, #devpage-place-panel, #devpage-place-float")) return;
  const selection = document.getSelection();
  if (selection && !selection.isCollapsed) return;
  const state = showSlide();
  if (!state) return;
  const touch = event.changedTouches[0];
  const dx = touch.clientX - start.x, dy = touch.clientY - start.y;
  let step = 0;
  if (Math.abs(dx) > 40 && Math.abs(dx) > Math.abs(dy)) step = dx < 0 ? 1 : -1;
  else if (Math.abs(dx) < 10 && Math.abs(dy) < 10 && event.timeStamp - start.at < TAP_MS) step = touch.clientX < innerWidth / 3 ? -1 : 1;
  if (!step) return;
  const next = Math.min(Math.max(state.index + step, 1), state.count);
  if (next !== state.index) location.hash = `#${next}`;
  event.preventDefault();
});
window.addEventListener("hashchange", () => showSlide());
window.addEventListener("resize", () => {
  showSlide();
  fitDiagrams();
});

// A doc's outline follows the reader: the section in view is marked current.
function followOutline() {
  const links = [...document.querySelectorAll(".outline ul a")];
  if (!links.length) return;
  const targets = links.map((link) => document.getElementById(decodeURIComponent(link.hash.slice(1)))).filter(Boolean);
  const mark = () => {
    let current = targets[0];
    for (const target of targets) if (target.getBoundingClientRect().top < innerHeight / 3) current = target;
    for (const link of links) link.classList.toggle("current", link.hash === `#${current?.id}`);
  };
  addEventListener("scroll", mark, { passive: true });
  mark();
}

// ---- The page module ------------------------------------------------------
// What a widget imports as `devpage`: `page`, which reads and steers the page
// shown, and `base`, the address an extension of the document answers under.
// A part of a page is `{ page, quote, prefix, suffix }` — a source's address,
// as the server names it, and words in it with the text around them: what a
// comment is anchored by.

const listeners = { change: new Set(), select: new Set(), pick: new Set() };

function emit(event, value) {
  for (const listener of [...listeners[event]]) {
    try {
      listener(value);
    } catch (error) {
      console.error(error);
    }
  }
}

const partOf = (section, anchor) => ({ page: unmounted(section.dataset.source), ...anchor });
const sectionOf = (address) => sources().find((section) => unmounted(section.dataset.source) === address);

// Where a part is on the page shown: its source's section and text model,
// and the part's place in the model; null when the page has no such source
// or the words are gone.
function locate(part) {
  const section = sectionOf(part.page);
  if (!section) return null;
  const model = textModel(section);
  const at = find(model, { prefix: "", suffix: "", ...part });
  return at ? { section, model, at } : null;
}

// The parts marked, by the name they were marked under. They are marked
// again on every page swapped in, wherever that page has them.
const marked = new Map();

function showMarks() {
  const old = [...document.querySelectorAll("mark.devpage-mark, tspan.devpage-mark")];
  for (const mark of old) mark.replaceWith(...mark.childNodes);
  if (old.length) sources().forEach((section) => section.normalize());
  for (const [name, parts] of marked) {
    for (const part of parts) {
      const found = locate(part);
      if (!found) continue;
      highlight(found.model, found.at, (mark) => {
        mark.classList.add("devpage-mark");
        mark.dataset.mark = name;
      });
    }
  }
  if (old.length || marked.size) layoutComments();
}

// Picking: while it is on, the block under the pointer is outlined and a
// click on its text is a pick — no comment starts. A click on a link or a
// control does what it always does, and a click beside the text picks
// nothing, which the listeners are told as `null`.
let picking = false;
// What a click is meant for itself, and what is not the page.
const CONTROLS = "a, button, input, textarea, select, summary, label, video, audio, canvas, .design-embed";
const FRAME = "#devpage-navbar, #devpage-nav, #devpage-sidebar, #devpage-corner, #devpage-comments, #devpage-place-panel, #devpage-place-float, .comment-ui";
let pickHover = null;

function pickBlock(target) {
  const block = target.closest?.(blocks);
  return block?.closest("#devpage-page [data-source]") ? block : null;
}

function setPickHover(block) {
  if (block === pickHover) return;
  pickHover?.classList.remove("devpage-pick");
  pickHover = block;
  pickHover?.classList.add("devpage-pick");
}

document.addEventListener("mouseover", (event) => {
  if (picking) setPickHover(pickBlock(event.target));
});
document.addEventListener("click", (event) => {
  if (!picking || event.target.closest?.(`${CONTROLS}, ${FRAME}`)) return;
  const block = pickBlock(event.target);
  if (!block) return emit("pick", null);
  event.preventDefault();
  event.stopPropagation();
  const section = block.closest("[data-source]");
  const model = textModel(section);
  const anchor = anchorFor(model, indexOf(model, block, 0), indexOf(model, block, block.childNodes.length));
  if (anchor.quote) emit("pick", partOf(section, anchor));
}, true);

// A selection is told once it rests, and once more when it is gone.
let selectionTold = "null";
let selectionTimer = 0;
document.addEventListener("selectionchange", () => {
  if (!listeners.select.size) return;
  clearTimeout(selectionTimer);
  selectionTimer = setTimeout(() => {
    const part = shownPage.selection();
    const told = JSON.stringify(part);
    if (told === selectionTold) return;
    selectionTold = told;
    emit("select", part);
  }, 150);
});

const shownPage = {
  // The page shown: its address, its #fragment, and its sources' addresses —
  // one for a page, one per chapter of a doc or slide of a deck.
  current() {
    return {
      address: unmounted(location.pathname),
      fragment: decodeURIComponent(location.hash.slice(1)),
      sources: sources().map((section) => unmounted(section.dataset.source)),
    };
  },
  // A source's text as the reader sees it, its white space collapsed; null
  // when the page shown has no such source.
  text(source = shownPage.current().sources[0]) {
    const section = sectionOf(source);
    return section ? textModel(section).text : null;
  },
  // The text selected, as a part; null when none is, or it isn't in one source.
  selection() {
    const selected = selectedText();
    return selected ? partOf(selected.section, selected.anchor) : null;
  },
  // A part's DOM range on the page shown; null when it isn't there.
  range(part) {
    const found = locate(part);
    if (!found) return null;
    const { model, at: [start, end] } = found;
    const range = document.createRange();
    range.setStart(model.chars[start].node, model.chars[start].offset);
    range.setEnd(model.chars[end - 1].node, model.chars[end - 1].offset + 1);
    return range;
  },
  // Opens an address as a click on a link to it would: a page of the
  // document in place, anything else whole. An address starting with `/` is
  // the document's own, as `current` and a part name it.
  async go(address) {
    const url = new URL(address.startsWith("/") ? mount + address : address, location.href);
    if (url.origin !== location.origin || !url.pathname.endsWith(".md")) {
      location.assign(url);
      return;
    }
    clearFound();
    remember();
    if (pathOf(url) === shownPath) {
      if (url.hash && url.hash === location.hash) toFragment(url.hash);
      else if (url.hash) location.hash = url.hash;
      else scrollTo(0, 0);
      return;
    }
    if ((await load(url, "push")) === false) location.assign(url);
  },
  // Marks parts under a name — `mark.devpage-mark[data-mark="<name>"]` —
  // in place of what the name marked before; no parts takes its marks away.
  mark(name, parts = []) {
    if (parts.length) marked.set(name, parts);
    else marked.delete(name);
    showMarks();
  },
  // Turns picking on or off.
  pick(on) {
    picking = !!on;
    document.body.classList.toggle("picking", picking);
    if (!picking) setPickHover(null);
    else document.querySelectorAll(".comment-button").forEach((button) => button.remove());
  },
  // Opens or closes the panel; `null` leaves it to the window's width.
  panel(open) {
    panelOpen = open;
    layoutPanel();
  },
  // Calls `listener` on "change" (a page was shown: what `current` returns),
  // "select" (the selection rests: a part, or null) and "pick" (a block was
  // picked: its part). Returns what takes the listener off.
  on(event, listener) {
    if (!listeners[event]) throw new Error(`devpage: "${event}" is not a page event`);
    listeners[event].add(listener);
    return () => listeners[event].delete(listener);
  },
};

window.devpagePage = {
  page: shownPage,
  // The address an extension of the document answers under, by its name
  // under `extensions:`, ending in `/`.
  base(id) {
    const address = (window.devpageExtensions || {})[id];
    if (!address) throw new Error(`devpage: the document has no extension named ${id}`);
    return address;
  },
};

drawDesigns();
Promise.all([mountWidgets(document).catch((error) => console.error(error)), sidebarMounted]).finally(() => {
  // A widget's content mounts with it: designs it embeds exist only now.
  drawDesigns();
  markBroken(document);
  fitDiagrams();
  showSlide();
  if (!print && typeof history.state?.scrollY === "number") scrollTo(0, history.state.scrollY);
  if (!print) landHandedOver();
  followOutline();
  loadComments();
  loadVersions();
  showPlaces();
  // Print mode has no live connection; Chrome waits for this — every widget
  // mounted — before printing.
  document.documentElement.dataset.ready = "true";
});
if (!print && !document.hidden) connect();

// ---- Theme ----------------------------------------------------------------
// One button switches between light and dark, and the browser remembers the
// theme switched to. Until the reader switches, the page follows the system
// setting, live. The page's head set the theme before it painted; this keeps
// it current.

const themeQuery = matchMedia("(prefers-color-scheme: dark)");

function savedTheme() {
  if (!document.querySelector(".theme-toggle")) return null;
  try {
    const saved = localStorage.getItem("devpage-theme");
    return saved === "light" || saved === "dark" ? saved : null;
  } catch {
    return null;
  }
}

function applyTheme(theme = savedTheme()) {
  const root = document.documentElement;
  root.dataset.theme = theme || (themeQuery.matches ? "dark" : "light");
  const label = root.dataset.theme === "dark" ? "Switch to light" : "Switch to dark";
  for (const button of document.querySelectorAll(".theme-toggle")) {
    button.title = label;
    button.setAttribute("aria-label", label);
  }
}

applyTheme();
themeQuery.addEventListener("change", () => applyTheme());

document.addEventListener("click", (event) => {
  if (!event.target.closest?.(".theme-toggle")) return;
  const theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  try {
    localStorage.setItem("devpage-theme", theme);
  } catch {
    // Storage may be blocked; the switch still holds for this page.
  }
  applyTheme(theme);
});

// ---- Navbar ---------------------------------------------------------------
// Dropdowns open on click or from the keyboard and close on Escape, on a click
// elsewhere, or when a link in them is followed. On a narrow screen the links
// fold into a menu the toggle opens.

// On a narrow screen the navigator — the navbar's links, then the sidebar, or
// a doc's outline — slides in over the page: its toggle opens it with the
// sidebar's current entry in view; a followed link, a touch beside it or
// Escape closes it.
function setNavigator(open) {
  document.body.classList.toggle("nav-open", open);
  for (const toggle of document.querySelectorAll(".nav-toggle")) toggle.setAttribute("aria-expanded", String(open));
  if (open) document.querySelector("#devpage-nav a.current")?.scrollIntoView({ block: "center" });
}

document.addEventListener("click", (event) => {
  if (event.target.closest?.(".nav-toggle")) {
    setNavigator(!document.body.classList.contains("nav-open"));
    return;
  }
  if (!document.body.classList.contains("nav-open")) return;
  const inside = event.target.closest?.(".navigator");
  if (inside && !event.target.closest("a")) return;
  // The touch beside the navigator only closes it; a link in it is followed.
  if (!inside) {
    event.preventDefault();
    event.stopPropagation();
  }
  setNavigator(false);
}, true);

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape" || !document.body.classList.contains("nav-open")) return;
  setNavigator(false);
  [...document.querySelectorAll(".nav-toggle")].find((toggle) => toggle.offsetParent)?.focus();
});

function closeMenus(except) {
  for (const menu of document.querySelectorAll(".navbar-menu.open")) {
    if (menu === except) continue;
    menu.classList.remove("open");
    menu.querySelector(":scope > button").setAttribute("aria-expanded", "false");
  }
}

function openMenu(menu, focusFirst) {
  closeMenus(menu);
  menu.classList.add("open");
  menu.querySelector(":scope > button").setAttribute("aria-expanded", "true");
  if (focusFirst) menu.querySelector(".navbar-entry")?.focus();
}

document.addEventListener("click", (event) => {
  const button = event.target.closest?.(".navbar-menu > button");
  if (button) {
    const menu = button.parentElement;
    if (menu.classList.contains("open")) closeMenus();
    else openMenu(menu, false);
    return;
  }
  // Anywhere else, or a followed link, closes what is open.
  if (!event.target.closest?.(".navbar-dropdown") || event.target.closest("a")) closeMenus();
});

document.addEventListener("keydown", (event) => {
  const menu = event.target.closest?.(".navbar-menu");
  if (!menu) return;
  const entries = [...menu.querySelectorAll(".navbar-entry")];
  const at = entries.indexOf(document.activeElement);
  if (event.key === "Escape") {
    closeMenus();
    menu.querySelector(":scope > button").focus();
  } else if (event.key === "ArrowDown") {
    event.preventDefault();
    if (!menu.classList.contains("open")) openMenu(menu, true);
    else entries[(at + 1) % entries.length]?.focus();
  } else if (event.key === "ArrowUp" && menu.classList.contains("open")) {
    event.preventDefault();
    entries[(at - 1 + entries.length) % entries.length]?.focus();
  }
});

// ---- Versions ---------------------------------------------------------------
// A page a devpage server serves under a published document's mount asks it
// which version the page shows: a menu of every version, the download of the
// one shown, and — on an old version — a banner linking to the latest. A
// local page has no mount and asks nothing; a mount with no versions answers
// 404 and shows nothing.

async function loadVersions() {
  if (print || !mount) return;
  let info;
  try {
    const response = await fetch(`${mount}/__devpage/versions`, { cache: "no-cache" });
    if (!response.ok) return;
    info = await response.json();
  } catch {
    return;
  }
  showVersions(info);
}

function showVersions(info) {
  let bar = document.getElementById("devpage-versions");
  if (!bar) {
    bar = document.createElement("div");
    bar.id = "devpage-versions";
    document.body.append(bar);
  }
  const menu = document.createElement("details");
  menu.className = "version-menu";
  const summary = document.createElement("summary");
  summary.textContent = `Version ${info.version}${info.version === info.latest ? " · latest" : ""}`;
  const list = document.createElement("ul");
  for (const version of info.versions) {
    const item = document.createElement("li");
    const link = document.createElement("a");
    link.href = version.url;
    link.textContent = `Version ${version.number}`;
    if (version.number === info.version) link.setAttribute("aria-current", "page");
    const when = document.createElement("span");
    when.textContent = ` ${version.published} · ${version.author}`;
    item.append(link, when);
    list.append(item);
  }
  menu.append(summary, list);
  const parts = [menu];
  if (info.download) {
    const download = document.createElement("a");
    download.className = "version-download";
    download.href = info.download.url;
    download.setAttribute("download", "");
    download.textContent = `Download ${info.download.format}`;
    parts.push(download);
  }
  bar.replaceChildren(...parts);
  document.body.classList.add("with-versions");

  let banner = document.getElementById("devpage-version-banner");
  const old = info.pinned && info.version !== info.latest;
  document.body.classList.toggle("with-version-banner", old);
  if (!old) {
    banner?.remove();
    return;
  }
  if (!banner) {
    banner = document.createElement("div");
    banner.id = "devpage-version-banner";
    document.body.prepend(banner);
  }
  const latest = document.createElement("a");
  latest.href = info.latest_url;
  latest.textContent = `the latest is version ${info.latest}`;
  banner.replaceChildren(`You are reading version ${info.version}; `, latest, ".");
}
