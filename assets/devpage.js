// Draws Mermaid diagrams with pele, and keeps the page live: when a watched file
// changes, the page re-fetches itself and swaps its content in place.
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

// ---- Widgets --------------------------------------------------------------
// The server leaves each widget as a placeholder carrying its name, its props'
// source and its content rendered as Markdown; Mermaid fences stay as
// <pre lang="mermaid"> and become Diagram widgets here. Each placeholder is
// mounted with React, on load and after every live update.

const widgetModules = {};

// Widgets render other widgets by name; compiled widgets look them up here.
globalThis.__devpageWidget = (name) => widgetModules[name]?.default ?? (() => { throw new Error(`<${name}> is not a widget`); });

const widgetUrls = {};

// Why a widget's module didn't load — a compile error in it or in a helper it
// imports — so the widget's place shows the reason.
const widgetLoadErrors = {};

// Loads a widget's module, again when its address changed — an edited widget
// or helper gives the folder a new version — puts its exported CSS in the page
// once, and loads the widgets it renders by name, which it lists.
async function loadWidget(name) {
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
    await Promise.all((widgetModules[name].__devpageUses || []).map((used) => loadWidget(used)
      .then(() => { delete widgetLoadErrors[used]; })
      .catch((error) => { widgetLoadErrors[used] = error; })));
    document.head.querySelector(`style[data-widget="${name}"]`)?.remove();
    const css = widgetModules[name].css;
    if (css) {
      const style = document.createElement("style");
      style.dataset.widget = name;
      style.textContent = css;
      document.head.append(style);
    }
  }
  return widgetModules[name];
}

async function mountWidgets(root) {
  for (const pre of root.querySelectorAll('pre[lang="mermaid"]')) {
    const placeholder = document.createElement("div");
    placeholder.dataset.widget = "Diagram";
    placeholder.dataset.props = "{}";
    placeholder.dataset.source = pre.textContent;
    pre.replaceWith(placeholder);
  }
  const placeholders = [...root.querySelectorAll("[data-widget]")].filter((el) => !el.parentElement.closest("[data-widget]"));
  if (!placeholders.length) return;
  const React = await import("react");
  const { createRoot } = await import("react-dom/client");
  // Only the widgets this content names, nested ones included, and those they
  // render by name.
  const names = new Set([...root.querySelectorAll("[data-widget]")].map((el) => el.dataset.widget));
  await Promise.all([...names].map((name) => loadWidget(name)
    .then(() => { delete widgetLoadErrors[name]; })
    .catch((error) => { widgetLoadErrors[name] = error; })));

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
    return React.createElement(component, { ...props, key, source: node.dataset.source ?? "", sourceHtml: node.dataset.sourceHtml, html: node.innerHTML, children: children.length ? children : undefined });
  };

  const mount = (els) => Promise.all(els.map((el) => new Promise((resolve) => {
    el.dataset.mounted = "";
    const element = React.createElement(Boundary, { name: el.dataset.widget }, widgetElement(el, 0));
    const root = createRoot(el);
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

// A page fetched again keeps the canvases it already has: each embedded
// design still in the page keeps its drawing and its stream. They never leave
// this document — a canvas moved between documents may lose its context. An
// embed inside a widget is React's to make again; the design script carries
// what it shows across that instead.
function keepDesigns(swap) {
  const outside = (embed) => !embed.closest("[data-widget]");
  const kept = [...document.querySelectorAll("#devpage-page .design-embed")].filter(outside);
  swap();
  for (const embed of [...document.querySelectorAll("#devpage-page .design-embed")].filter(outside)) {
    // By selector: a problems bar can sit before the canvas.
    const src = embed.querySelector("canvas.design")?.getAttribute("src");
    const at = kept.findIndex((old) => old.querySelector("canvas.design")?.getAttribute("src") === src);
    if (at >= 0) embed.replaceWith(kept.splice(at, 1)[0]);
  }
}

// The frame a body was rendered in: its classes, less `show-comments`, which
// this script adds while the comments are shown and no fetched page carries;
// and the devpage build that rendered it, so a page from a new build — a new
// page script, a new stylesheet — is loaded whole.
function frameOf(body) {
  const classes = [...body.classList].filter((name) => name !== "show-comments").sort();
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

// Fetches a page of this document and swaps its content and sidebar in —
// unless it is the page shown, unchanged.
async function load(url) {
  await seeded;
  const response = await fetch(url, { cache: "no-cache" });
  if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) return false;
  const etag = response.headers.get("etag");
  if (etag && etag === shown.etag && pageKey(url) === shown.url) return true;
  const next = new DOMParser().parseFromString(await response.text(), "text/html");
  // A different frame — a fixed document file replacing its problem page, or
  // a document that changed kind — can't be swapped in part: load it whole.
  if (frameOf(next.body) !== frameOf(document.body)) {
    location.assign(url);
    return "navigating";
  }
  shown = { url: pageKey(url), etag };
  keepDesigns(() => {
    for (const id of ["devpage-page", "devpage-nav", "devpage-navbar"]) {
      const fresh = next.getElementById(id);
      if (fresh) document.getElementById(id)?.replaceChildren(...fresh.childNodes);
    }
  });
  document.getElementById("devpage-navbar")?.classList.remove("open");
  applyTheme();
  applyCommentMode();
  document.title = next.title;
  // The fresh page names its widgets' current versions.
  const list = [...next.scripts].find((script) => script.textContent.startsWith("window.devpageWidgets"));
  if (list) new Function(list.textContent)();
  await mountWidgets(document.getElementById("devpage-page"));
  drawDesigns();
  markBroken(document.getElementById("devpage-page"));
  showSlide();
  loadComments();
  return true;
}

// Links to other Markdown pages of this document load in place — in the text
// and in diagrams, where a Mermaid `click node href "…"` line becomes an SVG
// link. An SVG link's `href` property is not a string, so the attribute is
// resolved against the page instead.
document.addEventListener("click", async (event) => {
  const link = event.target.closest("a[href]");
  if (!link || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
  const url = new URL(link.getAttribute("href"), location.href);
  if (url.origin !== location.origin || !url.pathname.endsWith(".md")) return;
  event.preventDefault();
  if (url.pathname === location.pathname) {
    if (url.hash) location.hash = url.hash;
    return;
  }
  const loaded = await load(url);
  // A page of another kind loads whole; touching history now would cancel it.
  if (loaded === "navigating") return;
  if (loaded) {
    history.pushState(null, "", url);
    if (url.hash) document.querySelector(url.hash)?.scrollIntoView();
    else window.scrollTo(0, 0);
  } else {
    location.href = url;
  }
});


// ---------------------------------------------------------------------------
// Comments: threads anchored to rendered text, kept by the server in
// .devpage/comments/. Each element with a data-source is one Markdown file's
// text — a page, a doc's chapter or a deck's slide.
//
// One text model serves both ends: a section's text is its text nodes in order
// with every run of whitespace collapsed to one space. A new comment's quote,
// prefix and suffix are cut from that text, and a thread is found again by
// searching the same text, so nested lists, tables and code anchor the same
// way they were quoted.

const sources = () => [...document.querySelectorAll("[data-source]")];
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
// the page's structure is never moved.
function highlight(model, [start, end], id) {
  const marks = [];
  let i = start;
  while (i < end) {
    const { node, offset } = model.chars[i];
    let last = i;
    while (last + 1 < end && model.chars[last + 1].node === node) last++;
    const range = document.createRange();
    range.setStart(node, offset);
    range.setEnd(node, model.chars[last].offset + 1);
    const mark = document.createElement("mark");
    mark.className = "comment";
    mark.dataset.thread = id;
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

// Fetches every source's threads, then marks and lists them in one go; a
// newer call that finishes first wins, so overlapping refreshes never mix.
let commentsGeneration = 0;
async function loadComments() {
  if (!comments) return;
  const generation = ++commentsGeneration;
  const fetched = [];
  for (const section of sources()) {
    const page = section.dataset.source;
    const response = await fetch(`/__devpage/comments?page=${encodeURIComponent(page)}`, { cache: "no-cache" });
    fetched.push({ section, page, threads: response.ok ? await response.json() : [] });
  }
  if (generation !== commentsGeneration) return;
  document.querySelectorAll("mark.comment").forEach((mark) => mark.replaceWith(...mark.childNodes));
  sources().forEach((section) => section.normalize());
  const aside = panel();
  aside.querySelectorAll(".thread:not(.composer), .detached-heading").forEach((card) => card.remove());
  const placed = [];
  const detached = [];
  for (const { section, page, threads } of fetched) {
    if (!section.isConnected) continue;
    for (const thread of threads) {
      const card = threadCard(page, thread);
      const model = textModel(section);
      const at = find(model, thread.anchor);
      if (!at) {
        detached.push(card);
        continue;
      }
      const marks = highlight(model, at, thread.id);
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
  const beside = !document.body.classList.contains("slides") && matchMedia("(min-width: 1100px)").matches;
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

function threadCard(page, thread) {
  const card = document.createElement("div");
  card.className = "thread";
  card.dataset.thread = thread.id;
  const quote = document.createElement("blockquote");
  quote.textContent = thread.anchor.quote;
  card.append(quote, ...thread.thread.map(messageLine));
  const resolve = document.createElement("button");
  resolve.textContent = "Resolve";
  resolve.addEventListener("click", () => send("resolve", { page, id: thread.id }, card));
  card.append(editor("Reply", (text) => send("reply", { page, id: thread.id, text }, card), [resolve]));
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
  const response = await fetch(action ? `/__devpage/comments/${action}` : "/__devpage/comments", {
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
    const refused = await send("", { page: section.dataset.source, anchor, text });
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
  if (!comments || !commentMode() || !getSelection().isCollapsed) return;
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
  if (!comments || !commentMode() || event.target.closest?.(".comment-ui")) return;
  setTimeout(() => {
    const selection = getSelection();
    if (selection.isCollapsed) return;
    const range = selection.getRangeAt(0);
    const common = range.commonAncestorContainer;
    const section = (common.nodeType === Node.ELEMENT_NODE ? common : common.parentElement)?.closest("[data-source]");
    if (!section) return;
    const model = textModel(section);
    const anchor = anchorFor(model, indexOf(model, range.startContainer, range.startOffset), indexOf(model, range.endContainer, range.endOffset));
    if (!anchor.quote) return;
    const rect = range.getBoundingClientRect();
    commentButton("Comment", "Comment on the selection", scrollY + rect.bottom + 6, scrollX + rect.left, () => {
      selection.removeAllRanges();
      compose(section, anchor, range.startContainer.parentElement);
    });
  });
});
// ---------------------------------------------------------------------------

window.addEventListener("popstate", () => load(location.href));

// The live-update stream, open only while the tab is visible.
let stream = null;

function connect() {
  if (stream) return;
  stream = new EventSource("/__devpage/events");
  stream.onmessage = (event) => {
    if (event.data.startsWith("comments ")) {
      if (sources().some((s) => s.dataset.source === event.data.slice(9))) loadComments();
    } else {
      load(location.href);
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
    load(location.href); // catch up on changes made while hidden
  }
});
window.addEventListener("pagehide", disconnect);

// A deck shows one slide at a time, scaled to the window; its number is the
// address's #fragment.
function showSlide() {
  const slides = [...document.querySelectorAll(".deck .slide")];
  if (!slides.length || print) return;
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
window.addEventListener("hashchange", showSlide);
window.addEventListener("resize", showSlide);

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

drawDesigns();
mountWidgets(document).catch((error) => console.error(error)).finally(() => {
  // A widget's content mounts with it: designs it embeds exist only now.
  drawDesigns();
  markBroken(document);
  showSlide();
  followOutline();
  loadComments();
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
  const toggle = event.target.closest?.(".navbar-toggle");
  if (toggle) {
    const bar = document.getElementById("devpage-navbar");
    const open = bar.classList.toggle("open");
    toggle.setAttribute("aria-expanded", String(open));
    return;
  }
  const button = event.target.closest?.(".navbar-menu > button");
  if (button) {
    const menu = button.parentElement;
    if (menu.classList.contains("open")) closeMenus();
    else openMenu(menu, false);
    return;
  }
  // Anywhere else, or a followed link, closes what is open.
  if (!event.target.closest?.(".navbar-dropdown") || event.target.closest("a")) closeMenus();
  if (event.target.closest?.("#devpage-navbar a")) document.getElementById("devpage-navbar")?.classList.remove("open");
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
