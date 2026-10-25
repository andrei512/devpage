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

// Fetches a page of this document and swaps its content and sidebar in.
async function load(url) {
  const response = await fetch(url, { cache: "no-store" });
  if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) return false;
  const next = new DOMParser().parseFromString(await response.text(), "text/html");
  for (const id of ["page", "nav"]) {
    const fresh = next.getElementById(id);
    if (fresh) document.getElementById(id).replaceChildren(...fresh.childNodes);
  }
  document.title = next.title;
  drawDiagrams(document.getElementById("page"));
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


// Comments: threads anchored to rendered text, kept by the server in
// .devpage/comments/. Each section with a data-source is one page's text.
const sources = () => [...document.querySelectorAll("[data-source]")];

async function loadComments() {
  if (print) return;
  document.querySelectorAll("mark.comment").forEach((mark) => mark.replaceWith(...mark.childNodes));
  let panel = document.getElementById("comments");
  if (!panel) {
    panel = document.createElement("aside");
    panel.id = "comments";
    document.body.append(panel);
  }
  panel.replaceChildren();
  const detached = [];
  for (const section of sources()) {
    const page = section.dataset.source;
    const response = await fetch(`/__devpage/comments?page=${encodeURIComponent(page)}`, { cache: "no-store" });
    if (!response.ok) continue;
    for (const thread of await response.json()) {
      const mark = highlight(section, thread.anchor);
      const card = threadCard(page, thread);
      if (mark) {
        mark.addEventListener("click", () => card.scrollIntoView({ block: "nearest" }));
        panel.append(card);
      } else {
        detached.push(card);
      }
    }
  }
  if (detached.length) {
    const heading = document.createElement("h3");
    heading.textContent = "Detached comments";
    panel.prepend(heading, ...detached);
  }
}

// Wraps the first occurrence of prefix+quote+suffix (or just the quote) in a mark.
function highlight(section, anchor) {
  const nodes = [];
  const walker = document.createTreeWalker(section, NodeFilter.SHOW_TEXT);
  let text = "";
  while (walker.nextNode()) {
    nodes.push({ node: walker.currentNode, start: text.length });
    text += walker.currentNode.data;
  }
  let at = text.indexOf(anchor.prefix + anchor.quote + anchor.suffix);
  at = at >= 0 ? at + anchor.prefix.length : text.indexOf(anchor.quote);
  if (at < 0 || !anchor.quote) return null;
  const locate = (offset) => {
    const hit = nodes.findLast((n) => n.start <= offset);
    return [hit.node, offset - hit.start];
  };
  const range = document.createRange();
  range.setStart(...locate(at));
  range.setEnd(...locate(at + anchor.quote.length));
  const mark = document.createElement("mark");
  mark.className = "comment";
  try {
    range.surroundContents(mark);
  } catch {
    mark.append(range.extractContents());
    range.insertNode(mark);
  }
  return mark;
}

function threadCard(page, thread) {
  const card = document.createElement("div");
  card.className = "thread";
  const quote = document.createElement("blockquote");
  quote.textContent = thread.anchor.quote;
  card.append(quote);
  for (const message of thread.thread) {
    const p = document.createElement("p");
    p.innerHTML = "<b></b> ";
    p.firstChild.textContent = message.author;
    p.append(message.text);
    card.append(p);
  }
  const reply = document.createElement("input");
  reply.placeholder = "Reply";
  reply.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && reply.value) post("reply", { page, id: thread.id, text: reply.value });
  });
  const resolve = document.createElement("button");
  resolve.textContent = "Resolve";
  resolve.addEventListener("click", () => post("resolve", { page, id: thread.id }));
  card.append(reply, resolve);
  return card;
}

async function post(action, body) {
  const url = action ? `/__devpage/comments/${action}` : "/__devpage/comments";
  await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
}

// Hovering a paragraph — or a list item, table, code block or diagram —
// offers a comment on the whole of it.
const blocks = "p, li, pre, table, .diagram, blockquote";
let hovered = null;
document.addEventListener("mouseover", (event) => {
  if (print) return;
  const block = event.target.closest?.(blocks);
  if (!block || block === hovered || !block.closest("[data-source]") || block.closest("#comments")) return;
  hovered = block;
  document.getElementById("paragraph-button")?.remove();
  const button = document.createElement("button");
  button.id = "paragraph-button";
  button.textContent = "+";
  button.title = "Comment on this paragraph";
  const rect = block.getBoundingClientRect();
  button.style.cssText = `position:absolute;top:${scrollY + rect.top}px;left:${scrollX + rect.left - 34}px`;
  button.addEventListener("click", () => {
    const text = prompt("Comment");
    if (text) post("", { page: block.closest("[data-source]").dataset.source, anchor: { quote: block.innerText.trim(), prefix: "", suffix: "" }, text });
  });
  document.body.append(button);
});

// Selecting text inside one source offers a comment on it.
document.addEventListener("mouseup", () => {
  document.getElementById("comment-button")?.remove();
  const selection = getSelection();
  if (print || selection.isCollapsed) return;
  const range = selection.getRangeAt(0);
  const section = range.commonAncestorContainer.parentElement?.closest("[data-source]");
  if (!section) return;
  const before = document.createRange();
  before.setStart(section, 0);
  before.setEnd(range.startContainer, range.startOffset);
  const after = document.createRange();
  after.setStart(range.endContainer, range.endOffset);
  after.setEndAfter(section.lastChild);
  const anchor = {
    quote: range.toString(),
    prefix: before.toString().slice(-32),
    suffix: after.toString().slice(0, 32),
  };
  const button = document.createElement("button");
  button.id = "comment-button";
  button.textContent = "Comment";
  const rect = range.getBoundingClientRect();
  button.style.cssText = `position:absolute;top:${scrollY + rect.bottom + 6}px;left:${scrollX + rect.left}px`;
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", () => {
    const text = prompt("Comment");
    button.remove();
    if (text) post("", { page: section.dataset.source, anchor, text });
  });
  document.body.append(button);
});

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
  if (event.target.closest?.("input, textarea")) return;
  const state = showSlide();
  if (!state) return;
  const go = (n) => { location.hash = `#${Math.min(Math.max(n, 1), state.count)}`; };
  if (["ArrowRight", " ", "PageDown", "ArrowDown"].includes(event.key)) go(state.index + 1);
  else if (["ArrowLeft", "PageUp", "ArrowUp"].includes(event.key)) go(state.index - 1);
  else if (event.key === "Home") go(1);
  else if (event.key === "End") go(state.count);
  else if (event.key === "c") document.body.classList.toggle("show-comments");
  else if (event.key === "f") document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen();
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
markBroken(document);
showSlide();
followOutline();
loadComments();
// Print mode has no live connection; Chrome waits for this before printing.
document.documentElement.dataset.ready = "true";
if (!print && !document.hidden) connect();
