// Draws Mermaid diagrams with pele, and keeps the page live: when a watched file
// changes, the page re-fetches itself and swaps its content in place.
//
// Browsers allow only six open connections per host over plain HTTP, and the
// live-update stream holds one for as long as it is open. So a tab keeps a
// single stream however many pages it visits — links inside the document swap
// the page in place instead of loading a new one — and a hidden tab closes its
// stream, reopening it (and catching up) when it is shown again.

const print = document.currentScript.hasAttribute("data-print");

function drawDiagrams(root) {
  for (const pre of root.querySelectorAll('pre[lang="mermaid"]')) {
    const diagram = document.createElement("div");
    diagram.className = "diagram";
    try {
      diagram.innerHTML = pele.render(pre.textContent).svg;
    } catch (error) {
      const message = document.createElement("pre");
      message.className = "diagram-error";
      message.textContent = `Diagram error: ${error.message}\n\n${pre.textContent}`;
      diagram.append(message);
    }
    pre.replaceWith(diagram);
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
// this document — a canvas moved between documents may lose its context.
function keepDesigns(swap) {
  const kept = [...document.querySelectorAll("#page .design-embed")];
  swap();
  for (const embed of document.querySelectorAll("#page .design-embed")) {
    // By selector: a problems bar can sit before the canvas.
    const src = embed.querySelector("canvas.design")?.getAttribute("src");
    const at = kept.findIndex((old) => old.querySelector("canvas.design")?.getAttribute("src") === src);
    if (at >= 0) embed.replaceWith(kept.splice(at, 1)[0]);
  }
}

// Fetches a page of this document and swaps its content and sidebar in.
async function load(url) {
  const response = await fetch(url, { cache: "no-store" });
  if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) return false;
  const next = new DOMParser().parseFromString(await response.text(), "text/html");
  // A different frame — a fixed document file replacing its problem page, or
  // a document that changed kind — can't be swapped in part: load it whole.
  if (next.body.className !== document.body.className) {
    location.assign(url);
    return true;
  }
  keepDesigns(() => {
    for (const id of ["page", "nav"]) {
      const fresh = next.getElementById(id);
      if (fresh) document.getElementById(id).replaceChildren(...fresh.childNodes);
    }
  });
  document.title = next.title;
  drawDiagrams(document.getElementById("page"));
  drawDesigns();
  markBroken(document.getElementById("page"));
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
  if (await load(url)) {
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
  let aside = document.getElementById("comments");
  if (!aside) {
    aside = document.createElement("aside");
    aside.id = "comments";
    aside.className = "comment-ui";
    document.body.append(aside);
  }
  return aside;
}

// Fetches every source's threads, then marks and lists them in one go; a
// newer call that finishes first wins, so overlapping refreshes never mix.
let commentsGeneration = 0;
async function loadComments() {
  if (print) return;
  const generation = ++commentsGeneration;
  const fetched = [];
  for (const section of sources()) {
    const page = section.dataset.source;
    const response = await fetch(`/__devpage/comments?page=${encodeURIComponent(page)}`, { cache: "no-store" });
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
  let button = document.getElementById("comments-toggle");
  if (!button) {
    button = document.createElement("button");
    button.id = "comments-toggle";
    button.className = "comment-ui";
    button.addEventListener("click", () => document.body.classList.toggle("show-comments"));
    document.body.append(button);
  }
  return button;
}

// Beside the text on a wide page: each card level with its anchor, pushed down
// so none overlap. Elsewhere — narrow screens, decks — the panel is a list.
function layoutComments() {
  const aside = document.getElementById("comments");
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
  const card = document.querySelector(`#comments .thread[data-thread="${CSS.escape(id)}"]`);
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
  document.querySelector("#comments .composer")?.remove();
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

function commentButton(label, title, top, left, onClick) {
  document.querySelectorAll(".comment-button").forEach((button) => button.remove());
  const button = document.createElement("button");
  button.className = "comment-button comment-ui";
  button.textContent = label;
  button.title = title;
  button.style.cssText = `position:absolute;top:${top}px;left:${left}px`;
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", onClick);
  document.body.append(button);
  return button;
}

// Hovering a paragraph — or a list item, table, code block or diagram —
// offers a comment on the whole of it.
const blocks = "p, li, pre, table, .diagram, blockquote, h1, h2, h3, h4";
let hovered = null;
document.addEventListener("mouseover", (event) => {
  if (print || !getSelection().isCollapsed) return;
  const block = event.target.closest?.(blocks);
  const section = block?.closest("[data-source]");
  if (!block || block === hovered || !section || block.closest(".comment-ui")) return;
  hovered = block;
  const rect = block.getBoundingClientRect();
  commentButton("+", "Comment on this paragraph", scrollY + rect.top, scrollX + Math.max(rect.left - 34, 0), () => {
    const model = textModel(section);
    const anchor = anchorFor(model, indexOf(model, block, 0), indexOf(model, block, block.childNodes.length));
    if (anchor.quote) compose(section, anchor, block);
  });
});

// Selecting text inside one source offers a comment on it.
document.addEventListener("mouseup", (event) => {
  if (print || event.target.closest?.(".comment-ui")) return;
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
  else if (key === "c") document.body.classList.toggle("show-comments");
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

drawDiagrams(document);
drawDesigns();
markBroken(document);
showSlide();
followOutline();
loadComments();
// Print mode has no live connection; Chrome waits for this before printing.
document.documentElement.dataset.ready = "true";
if (!print && !document.hidden) connect();
