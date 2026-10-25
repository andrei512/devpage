// The three kinds of document, side by side, each opening the page that teaches it.
const kinds = [
  { name: "Site", like: "a documentation website", href: "/docs/boxes/viewer/organizing-navigation/README.md" },
  { name: "Doc", like: "a Word document", href: "/docs/boxes/viewer/writing-a-doc/README.md" },
  { name: "Slides", like: "a PowerPoint deck", href: "/docs/boxes/viewer/making-slides/README.md" },
];

export const css = `
  .kind-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr)); gap: 1rem; margin: 1rem 0; }
  .kind-cards a { display: block; padding: 1rem; border: 1px solid var(--border); border-radius: 8px; color: inherit; text-decoration: none; }
  .kind-cards a:hover { border-color: var(--accent); }
  .kind-cards strong { display: block; font-size: 1.15rem; color: var(--accent); }
`;

export default function KindCards() {
  return (
    <div className="kind-cards">
      {kinds.map((kind) => (
        <a href={kind.href}>
          <strong>{kind.name}</strong>
          {kind.like}
        </a>
      ))}
    </div>
  );
}
