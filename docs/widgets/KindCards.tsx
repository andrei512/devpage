// The three kinds of document, side by side, each opening the page that teaches it.
const kinds = [
  {
    name: "Site",
    kind: "site",
    like: "a documentation website",
    madeOf: "a root page and the pages its Topics sections reach",
    readAs: "many pages, with a sidebar",
    href: "/docs/boxes/viewer/organizing-navigation/README.md",
  },
  {
    name: "Doc",
    kind: "doc",
    like: "a Word document",
    madeOf: "one or more chapter files, in order",
    readAs: "one long page, with an outline that follows you",
    href: "/docs/boxes/viewer/writing-a-doc/README.md",
  },
  {
    name: "Slides",
    kind: "slides",
    like: "a PowerPoint deck",
    madeOf: "one Markdown file per slide, in order",
    readAs: "one slide at a time, 16:9",
    href: "/docs/boxes/viewer/making-slides/README.md",
  },
];

export const css = `
  .kind-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); gap: 1rem; margin: 1.25rem 0; }
  .kind-cards a { display: flex; flex-direction: column; gap: 0.35rem; padding: 1rem 1.1rem; border: 1px solid var(--border); border-radius: 10px; color: inherit; text-decoration: none; transition: border-color 0.15s; }
  .kind-cards a:hover { border-color: var(--accent); }
  .kind-cards .kind-name { display: flex; align-items: baseline; justify-content: space-between; gap: 0.5rem; }
  .kind-cards strong { font-size: 1.2rem; color: var(--accent); }
  .kind-cards code { font-size: 0.8rem; }
  .kind-cards .kind-like { color: var(--muted); font-style: italic; }
  .kind-cards dl { margin: 0.25rem 0 0; font-size: 0.9rem; }
  .kind-cards dt { font-weight: 600; margin-top: 0.4rem; }
  .kind-cards dd { margin: 0; }
`;

export default function KindCards() {
  return (
    <div className="kind-cards">
      {kinds.map((kind) => (
        <a href={kind.href}>
          <span className="kind-name">
            <strong>{kind.name}</strong>
            <code>kind: {kind.kind}</code>
          </span>
          <span className="kind-like">Like {kind.like}</span>
          <dl>
            <dt>Made of</dt>
            <dd>{kind.madeOf}</dd>
            <dt>Read as</dt>
            <dd>{kind.readAs}</dd>
          </dl>
        </a>
      ))}
    </div>
  );
}
