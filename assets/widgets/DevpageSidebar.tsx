import { useEffect, useId, useRef, useState } from "react";

// A site's sidebar: devpage's own chrome, not a widget for pages. The server
// renders the same markup (src/render.rs `sidebar`) and the tree as JSON; the
// page script has React take the sidebar over from that markup and keeps it
// mounted across page swaps, giving it each fresh page's tree and address.
//
// Collapsed: the branch down to the page shown is open, its own children
// included; a chevron opens or closes any entry with children. What the
// reader opens is remembered per document in localStorage — the page's
// inline script opens those entries before the first paint, as this does.
//
// The search box at its top asks /__devpage/search as the reader types;
// while it holds text, the hits take the tree's place, grouped by document,
// this document's first. `/` or ⌘K / Ctrl+K focuses it; arrows move, Enter
// opens a hit through the page swap, Escape clears it.

// `home`: the page's position; a second listing of a page is a plain link.
type Entry = { label: string; href: string; home: boolean; children: Group[] };
type Group = { title: string | null; entries: Entry[] };
type Title = { label: string; href: string; logo: string | null };
type Props = { document?: string; document_path?: string; current?: string; title?: Title | null; groups?: Group[] };
type Hit = { title: string; heading: string; url: string; snippet: string; document: { title: string; path: string }; trail: string[] };
// What the server answered, and for which query.
type Found = { query: string; hits: Hit[] };

const storageKey = (document: string) => `devpage-sidebar:${document}`;

// What the reader opened on this document, by address; none when storage is
// blocked or holds something else.
function remembered(document: string): string[] {
  try {
    const opened = JSON.parse(localStorage.getItem(storageKey(document)) ?? "[]");
    return Array.isArray(opened) ? opened : [];
  } catch {
    return [];
  }
}

function remember(document: string, href: string, open: boolean) {
  try {
    const opened = remembered(document).filter((h) => h !== href);
    if (open) opened.push(href);
    localStorage.setItem(storageKey(document), JSON.stringify(opened));
  } catch {
    // Storage may be blocked; the entry stays open for this visit.
  }
}

const isShown = (entry: Entry, current: string) => entry.home && entry.href === current;

const leadsTo = (entry: Entry, current: string): boolean =>
  isShown(entry, current) || entry.children.some((g) => g.entries.some((e) => leadsTo(e, current)));

// The addresses of the entries open when `current` is shown: those it is,
// or is under, that have children.
function branch(groups: Group[], current: string): string[] {
  return groups.flatMap((g) => g.entries)
    .filter((e) => e.children.length && leadsTo(e, current))
    .flatMap((e) => [e.href, ...branch(e.children, current)]);
}

function Tree({ groups, current, open, toggle, hidden = false }: { groups: Group[]; current: string; open: Set<string>; toggle: (href: string) => void; hidden?: boolean }) {
  if (!groups.length) return null;
  return (
    <ul hidden={hidden}>
      {groups.flatMap((group, g) => [
        group.title !== null ? <li key={`group ${g}`} className="group"><span>{group.title}</span></li> : null,
        ...group.entries.map((entry, i) => {
          const link = <a href={entry.href} className={isShown(entry, current) ? "current" : undefined}>{entry.label}</a>;
          if (!entry.children.length) return <li key={`${g} ${i}`}>{link}</li>;
          const isOpen = open.has(entry.href);
          return (
            <li key={`${g} ${i}`} className={isOpen ? "branch open" : "branch"} data-href={entry.href}>
              <div className="entry">
                {link}
                <button type="button" className="chevron" aria-expanded={isOpen} aria-label={entry.label} onClick={() => toggle(entry.href)} />
              </div>
              <Tree groups={entry.children} current={current} open={open} toggle={toggle} />
            </li>
          );
        }),
      ])}
    </ul>
  );
}

// How long typing pauses before the box asks: short enough that hits come
// while typing, long enough that a word typed fast is one request.
const DEBOUNCE_MS = 60;

// The hits grouped by document, each group in the server's order, the groups
// by their best hit — this document's first.
function byDocument(hits: Hit[], documentPath: string): { title: string; path: string; hits: Hit[] }[] {
  const groups = new Map<string, { title: string; path: string; hits: Hit[] }>();
  for (const hit of hits) {
    const group = groups.get(hit.document.path) ?? { ...hit.document, hits: [] };
    group.hits.push(hit);
    groups.set(hit.document.path, group);
  }
  const all = [...groups.values()];
  return [...all.filter((g) => g.path === documentPath), ...all.filter((g) => g.path !== documentPath)];
}

// Whether a key pressed there is typing: `/` types there, not searches.
const typing = (target: EventTarget | null) =>
  target instanceof Element && (target.closest("input, textarea, select") !== null || (target as HTMLElement).isContentEditable);

// The search box and, while it holds text, its hits. Lives as long as the
// sidebar, so its text and hits persist across page swaps.
function useSearch(documentPath: string) {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Found>({ query: "", hits: [] });
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const hitLinks = useRef<(HTMLAnchorElement | null)[]>([]);
  // The latest request; an answer to any other is stale and dropped.
  const asked = useRef(0);
  // Enter pressed before the hits for the text typed came: open the first.
  const openOnArrival = useRef(false);

  useEffect(() => {
    const generation = ++asked.current;
    const q = query.trim();
    if (!q) return;
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const response = await fetch(`/__devpage/search?q=${encodeURIComponent(q)}`, { signal: controller.signal });
        const { hits } = await response.json();
        if (generation !== asked.current) return;
        setFound({ query, hits });
        setSelected(0);
      } catch {
        // Aborted by newer text, or the server is gone: the hits shown stay.
      }
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  // While the box holds text, the last hits answered take the tree's place —
  // none yet, before the first answer — so the tree never flashes back
  // between keystrokes. They are `fresh` once they answer the text typed.
  const groups = query.trim() ? byDocument(found.hits, documentPath) : null;
  const hits = groups?.flatMap((g) => g.hits) ?? [];
  const fresh = found.query === query;

  // Once the hits for the text typed are in, a pending Enter opens the first.
  useEffect(() => {
    if (!openOnArrival.current || !fresh) return;
    openOnArrival.current = false;
    hitLinks.current[0]?.click();
  });

  // The selected hit stays in view as the arrows move it.
  useEffect(() => {
    hitLinks.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  // `/` anywhere but in a field, or ⌘K / Ctrl+K anywhere, focuses the box.
  useEffect(() => {
    const focus = (event: KeyboardEvent) => {
      const commandK = event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey;
      const slash = event.key === "/" && !event.metaKey && !event.ctrlKey && !event.altKey && !typing(event.target);
      if (!commandK && !slash) return;
      event.preventDefault();
      input.current?.focus();
      input.current?.select();
    };
    document.addEventListener("keydown", focus);
    return () => document.removeEventListener("keydown", focus);
  }, []);

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    // An input method composing a word owns its keys.
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (!hits.length) return;
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((at) => (at + step + hits.length) % hits.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (fresh) hitLinks.current[selected]?.click();
      else if (query.trim()) openOnArrival.current = true;
    } else if (event.key === "Escape") {
      event.preventDefault();
      openOnArrival.current = false;
      if (!query) input.current?.blur();
      setQuery("");
      setFound({ query: "", hits: [] });
    }
  };

  return { query, setQuery, groups, hits, fresh, selected, setSelected, input, hitLinks, onKeyDown };
}

// One hit: where it is — its page's place in the nav — then its heading, then
// the run of its text that matched, the words marked by the server.
function HitLink({ hit, id, selected, link, onPoint }: { hit: Hit; id: string; selected: boolean; link: (a: HTMLAnchorElement | null) => void; onPoint: () => void }) {
  const heading = hit.heading || hit.title;
  const place = (hit.trail.length ? hit.trail : [hit.title]).join(" › ");
  return (
    <a ref={link} id={id} href={hit.url} role="option" aria-selected={selected} className={selected ? "hit selected" : "hit"} onMouseMove={onPoint}>
      {place !== heading ? <span className="hit-place">{place}</span> : null}
      <span className="hit-heading">{heading}</span>
      {hit.snippet ? <span className="hit-snippet" dangerouslySetInnerHTML={{ __html: hit.snippet }} /> : null}
    </a>
  );
}

export default function DevpageSidebar({ document = "", document_path = "", current = "", title = null, groups = [] }: Props) {
  const search = useSearch(document_path);
  const resultsId = useId();
  const [open, setOpen] = useState(() => new Set([...remembered(document), ...branch(groups, current)]));
  // A page swapped in opens its branch; what else is open stays open. A page
  // of another site starts from what the reader opened there.
  const [shown, setShown] = useState({ document, current });
  let opened = open;
  if (shown.document !== document || shown.current !== current) {
    opened = new Set([...(shown.document === document ? open : remembered(document)), ...branch(groups, current)]);
    setOpen(opened);
    setShown({ document, current });
  }
  // From the state as it is when React applies the click, so clicks that
  // come before a render each count. Remembering is idempotent.
  const toggle = (href: string) =>
    setOpen((before) => {
      const next = new Set(before);
      const isOpen = !next.has(href);
      if (isOpen) next.add(href);
      else next.delete(href);
      remember(document, href, isOpen);
      return next;
    });
  return (
    <>
      {title ? (
        <a className={title.href === current ? "document-title current" : "document-title"} href={title.href}>
          {title.logo ? <img className="logo" src={title.logo} alt="" /> : null}
          {title.label}
        </a>
      ) : null}
      {/* Before anything is typed this is render::SEARCH_BOX, which the
          server draws: React takes it over unchanged. */}
      <div className="search">
        <input
          ref={search.input}
          type="search"
          placeholder="Search"
          aria-label="Search the documents"
          autoComplete="off"
          spellCheck={false}
          role="combobox"
          aria-expanded={search.groups !== null}
          aria-autocomplete="list"
          aria-controls={search.groups ? resultsId : undefined}
          aria-activedescendant={search.hits.length ? `${resultsId}-${search.selected}` : undefined}
          value={search.query}
          onChange={(event) => search.setQuery(event.target.value)}
          onKeyDown={search.onKeyDown}
        />
        <kbd>/</kbd>
      </div>
      {search.groups ? (
        <div className="search-results" id={resultsId} role="listbox" aria-label="Search results">
          {search.groups.length ? (
            search.groups.map((group) => (
              <div key={group.path} className="search-group" role="group" aria-label={group.title}>
                <div className="search-document">{group.title}</div>
                {group.hits.map((hit) => {
                  const at = search.hits.indexOf(hit);
                  return (
                    <HitLink
                      key={hit.url}
                      hit={hit}
                      id={`${resultsId}-${at}`}
                      selected={at === search.selected}
                      link={(a) => {
                        search.hitLinks.current[at] = a;
                      }}
                      onPoint={() => search.setSelected(at)}
                    />
                  );
                })}
              </div>
            ))
          ) : search.fresh ? (
            <p className="search-empty">No results for “{search.query.trim()}”</p>
          ) : null}
        </div>
      ) : null}
      <Tree groups={groups} current={current} open={opened} toggle={toggle} hidden={search.groups !== null} />
    </>
  );
}
