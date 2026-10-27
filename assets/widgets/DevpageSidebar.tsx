import { useState } from "react";

// A site's sidebar: devpage's own chrome, not a widget for pages. The server
// renders the same markup (src/render.rs `sidebar`) and the tree as JSON; the
// page script has React take the sidebar over from that markup and keeps it
// mounted across page swaps, giving it each fresh page's tree and address.
//
// Collapsed: the branch down to the page shown is open, its own children
// included; a chevron opens or closes any entry with children. What the
// reader opens is remembered per document in localStorage — the page's
// inline script opens those entries before the first paint, as this does.

type Entry = { label: string; href: string; children: Group[] };
type Group = { title: string | null; entries: Entry[] };
type Title = { label: string; href: string; logo: string | null };
type Props = { document?: string; current?: string; title?: Title | null; groups?: Group[] };

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

const leadsTo = (entry: Entry, current: string): boolean =>
  entry.href === current || entry.children.some((g) => g.entries.some((e) => leadsTo(e, current)));

// The addresses of the entries open when `current` is shown: those it is,
// or is under, that have children.
function branch(groups: Group[], current: string): string[] {
  return groups.flatMap((g) => g.entries)
    .filter((e) => e.children.length && leadsTo(e, current))
    .flatMap((e) => [e.href, ...branch(e.children, current)]);
}

function Tree({ groups, current, open, toggle }: { groups: Group[]; current: string; open: Set<string>; toggle: (href: string) => void }) {
  if (!groups.length) return null;
  return (
    <ul>
      {groups.flatMap((group, g) => [
        group.title !== null ? <li key={`group ${g}`} className="group"><span>{group.title}</span></li> : null,
        ...group.entries.map((entry, i) => {
          const link = <a href={entry.href} className={entry.href === current ? "current" : undefined}>{entry.label}</a>;
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

export default function DevpageSidebar({ document = "", current = "", title = null, groups = [] }: Props) {
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
      {/* The search box goes here, at the top of the sidebar; while it
          holds text, its results take the tree's place. */}
      <Tree groups={groups} current={current} open={opened} toggle={toggle} />
    </>
  );
}
