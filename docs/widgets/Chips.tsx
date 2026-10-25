// A Markdown list drawn as a wrap of rounded chips — names, tags, features.
export const css = `
  .chips { list-style: none; padding: 0; margin: 0; display: flex; flex-wrap: wrap; gap: 20px; }
  .chips li {
    font-size: 40px; font-weight: 600; padding: 16px 34px; border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 10%, var(--background));
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border));
    color: var(--text);
  }
  .chips li code { font-size: 0.85em; background: none; }
`;

export default function Chips({ html }: { html: string }) {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const items = Array.from(parsed.querySelectorAll("li"));
  return (
    <ul className="chips">
      {items.map((item) => <li dangerouslySetInnerHTML={{ __html: item.innerHTML }} />)}
    </ul>
  );
}
