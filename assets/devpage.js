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
  showSlide();
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

window.addEventListener("popstate", () => load(location.href));

// The live-update stream, open only while the tab is visible.
let stream = null;

function connect() {
  if (stream) return;
  stream = new EventSource("/__devpage/events");
  stream.onmessage = () => load(location.href);
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
  const state = showSlide();
  if (!state) return;
  const go = (n) => { location.hash = `#${Math.min(Math.max(n, 1), state.count)}`; };
  if (["ArrowRight", " ", "PageDown", "ArrowDown"].includes(event.key)) go(state.index + 1);
  else if (["ArrowLeft", "PageUp", "ArrowUp"].includes(event.key)) go(state.index - 1);
  else if (event.key === "Home") go(1);
  else if (event.key === "End") go(state.count);
  else if (event.key === "f") document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen();
  else return;
  event.preventDefault();
});
window.addEventListener("hashchange", showSlide);
window.addEventListener("resize", showSlide);

drawDiagrams(document);
showSlide();
// Print mode has no live connection; Chrome waits for this before printing.
document.documentElement.dataset.ready = "true";
if (!print && !document.hidden) connect();
