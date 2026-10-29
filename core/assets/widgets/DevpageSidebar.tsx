import { useEffect, useId, useRef, useState } from "react";

// A site's sidebar: devpage's own chrome, not a widget for pages. The server
// renders the same markup (core/src/page/sidebar.rs) and the tree as JSON; the
// page script has React take the sidebar over from that markup and keeps it
// mounted across page swaps, giving it each fresh page's tree and address.
//
// Collapsed: the branch down to the page shown is open, its own children
// included; a chevron opens or closes any entry with children. What the
// reader opens is remembered per document in localStorage — the page's
// inline script opens those entries before the first paint, as this does.
//
// The search box at its top asks /__devpage/search as the reader types;
// while it holds text, the results take the tree's place, grouped by page:
// each page's row, its sections under it, then how many more it holds.
// `/` or ⌘K / Ctrl+K focuses it; arrows move through every row, Enter opens
// one through the page swap, Escape or the ⓧ button clears it. A row's link
// carries the words it matched (`data-found`): the page script marks them
// where the row opens.

// Each page is in the tree once, at its position.
type Entry = { label: string; href: string; children: Group[] };
type Group = { title: string | null; entries: Entry[] };
type Title = { label: string; href: string; logo: string | null };
type Props = { document?: string; document_path?: string; current?: string; title?: Title | null; groups?: Group[] };
// The endpoint's answer: the pages found, best first, each with its best
// sections and a count of the rest.
type Section = { heading: string; path: string[]; url: string; snippet: string; words: string[] };
type FoundPage = {
  title: string;
  url: string;
  trail: string[];
  document: { title: string; path: string };
  kind: "page" | "chapter" | "slide";
  title_match: boolean;
  lede: string;
  sections: Section[];
  more: number;
};
// What the server answered, and for which query.
type Found = { query: string; pages: FoundPage[] };
// One row of the results, in the order the arrows move through them.
type Row = { kind: "page"; page: FoundPage } | { kind: "section"; page: FoundPage; section: Section } | { kind: "more"; page: FoundPage };

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

const isShown = (entry: Entry, current: string) => entry.href === current;

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

// Each page's rows: its own, its sections', then the rest's.
const rowsOf = (page: FoundPage): Row[] => [
  { kind: "page", page },
  ...page.sections.map((section): Row => ({ kind: "section", page, section })),
  ...(page.more ? [{ kind: "more", page } as Row] : []),
];

// The row selected when results come: the best page's row when it was found
// by its title, else its best section.
const firstPick = (pages: FoundPage[]) => (pages[0] && !pages[0].title_match && pages[0].sections.length ? 1 : 0);

// Whether a key pressed there is typing: `/` types there, not searches.
const typing = (target: EventTarget | null) =>
  target instanceof Element && (target.closest("input, textarea, select") !== null || (target as HTMLElement).isContentEditable);

// The search box and, while it holds text, its hits. Lives as long as the
// sidebar, so its text and hits persist across page swaps.
function useSearch() {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Found>({ query: "", pages: [] });
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const rowLinks = useRef<(HTMLAnchorElement | null)[]>([]);
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
        const response = await fetch(`${document.documentElement.dataset.mount ?? ""}/__devpage/search?q=${encodeURIComponent(q)}`, { signal: controller.signal });
        const { pages = [] } = await response.json();
        if (generation !== asked.current) return;
        setFound({ query, pages });
        setSelected(firstPick(pages));
      } catch {
        // Aborted by newer text, or the server is gone: the results shown stay.
      }
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  // While the box holds text, the last results answered take the tree's
  // place — none yet, before the first answer — so the tree never flashes
  // back between keystrokes. They are `fresh` once they answer the text typed.
  const pages = query.trim() ? found.pages : null;
  const rows = pages?.flatMap(rowsOf) ?? [];
  const fresh = found.query === query;

  // Once the results for the text typed are in, a pending Enter opens the
  // row selected first.
  useEffect(() => {
    if (!openOnArrival.current || !fresh) return;
    openOnArrival.current = false;
    rowLinks.current[selected]?.click();
  });

  // The selected row stays in view as the arrows move it.
  useEffect(() => {
    rowLinks.current[selected]?.scrollIntoView({ block: "nearest" });
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
      if (!rows.length) return;
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((at) => (at + step + rows.length) % rows.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (fresh) rowLinks.current[selected]?.click();
      else if (query.trim()) openOnArrival.current = true;
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (!query) input.current?.blur();
      clear();
    }
  };

  // Empties the box and brings the tree back.
  const clear = () => {
    openOnArrival.current = false;
    setQuery("");
    setFound({ query: "", pages: [] });
  };

  return { query, setQuery, pages, rows, fresh, selected, setSelected, input, rowLinks, onKeyDown, clear };
}

// One row of the results, a link. A page's row: where it is — its
// document, when not this sidebar's, and its place in the nav above it —
// its title, and its lede when it was found by its title; it opens the
// page at its top. A section's row: its heading and the run of its text
// that matched, the words marked by the server; it opens the page at the
// first of them, marked. The more row opens the page.
function RowLink({ row, documentPath, id, selected, link, onPoint }: { row: Row; documentPath: string; id: string; selected: boolean; link: (a: HTMLAnchorElement | null) => void; onPoint: () => void }) {
  const { page } = row;
  const props = {
    ref: link,
    id,
    role: "option",
    "aria-selected": selected,
    className: `hit hit-${row.kind}${selected ? " selected" : ""}`,
    onMouseMove: onPoint,
  };
  if (row.kind === "section") {
    const { section } = row;
    return (
      <a {...props} href={section.url} data-found={JSON.stringify(section.words)} title={section.path.join(" › ")}>
        <span className="hit-label">{section.heading || page.title}</span>
        {section.snippet ? <span className="hit-snippet" dangerouslySetInnerHTML={{ __html: section.snippet }} /> : null}
      </a>
    );
  }
  if (row.kind === "more") {
    return (
      <a {...props} href={page.url} data-found="[]">
        <span className="hit-label">{page.more} more in this page</span>
      </a>
    );
  }
  const place = [...(page.document.path !== documentPath ? [page.document.title] : []), ...page.trail.slice(0, -1)]
    .filter((label, i, all) => label !== all[i - 1] && label !== page.title)
    .join(" › ");
  return (
    <a {...props} href={page.url} data-found="[]">
      {place ? <span className="hit-place">{place}</span> : null}
      <span className="hit-label">{page.title}</span>
      {page.title_match && page.lede ? <span className="hit-lede">{page.lede}</span> : null}
    </a>
  );
}

export default function DevpageSidebar({ document = "", document_path = "", current = "", title = null, groups = [] }: Props) {
  const search = useSearch();
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
      {/* Before anything is typed this is the page's SEARCH_BOX, which the
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
          aria-expanded={search.pages !== null}
          aria-autocomplete="list"
          aria-controls={search.pages ? resultsId : undefined}
          aria-activedescendant={search.rows.length ? `${resultsId}-${search.selected}` : undefined}
          value={search.query}
          onChange={(event) => search.setQuery(event.target.value)}
          onKeyDown={search.onKeyDown}
        />
        <kbd>/</kbd>
        {search.query ? (
          // Pressing it keeps the focus in the box, so typing goes on there.
          <button type="button" className="clear" aria-label="Clear search" onMouseDown={(event) => event.preventDefault()} onClick={() => { search.clear(); search.input.current?.focus(); }} />
        ) : null}
      </div>
      {search.pages ? (
        <div className="search-results" id={resultsId} role="listbox" aria-label="Search results">
          {search.pages.length ? (
            search.pages.map((page) => {
              // The page's own row, then the rows under it, indented.
              const first = search.rows.findIndex((r) => r.page === page);
              const [own, ...under] = rowsOf(page);
              const rowLink = (row: Row, at: number) => (
                <RowLink
                  key={at}
                  row={row}
                  documentPath={document_path}
                  id={`${resultsId}-${at}`}
                  selected={at === search.selected}
                  link={(a) => {
                    search.rowLinks.current[at] = a;
                  }}
                  onPoint={() => search.setSelected(at)}
                />
              );
              return (
                <div key={page.url} className="search-page" role="group" aria-label={page.title}>
                  {rowLink(own, first)}
                  {under.length ? <div className="search-sections">{under.map((row, i) => rowLink(row, first + 1 + i))}</div> : null}
                </div>
              );
            })
          ) : search.fresh ? (
            <p className="search-empty">No results for “{search.query.trim()}”</p>
          ) : null}
        </div>
      ) : null}
      <Tree groups={groups} current={current} open={opened} toggle={toggle} hidden={search.pages !== null} />
    </>
  );
}
