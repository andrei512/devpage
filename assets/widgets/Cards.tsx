// A grid of link cards from a Markdown list: each item's first link is the
// card's title and target, the text after it its summary, and an icon drawn
// before the link (by the server, from "(icon)") sits on top.
const escapeHtml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function Card({ item }: { item: Element }) {
  const own = item.querySelector(":scope > p") ?? item;
  const link = own.querySelector(":scope > a") as HTMLAnchorElement | null;
  const icon = own.querySelector(":scope > svg.icon");
  let seen = false;
  const summary = Array.from(own.childNodes)
    .filter((n) => {
      if (n === link) { seen = true; return false; }
      return seen && n !== icon && !(n instanceof HTMLUListElement);
    })
    .map((n) => (n instanceof Element ? n.outerHTML : escapeHtml(n.textContent ?? "")))
    .join("")
    .trim()
    .replace(/^(—|–|-|:)\s*/, "");
  const body = (
    <>
      {icon ? <span className="card-icon" dangerouslySetInnerHTML={{ __html: icon.outerHTML }} /> : null}
      <span className="card-title">
        <span dangerouslySetInnerHTML={{ __html: link ? link.innerHTML : escapeHtml(own.textContent ?? "") }} />
        {link ? <span className="card-arrow" aria-hidden="true">→</span> : null}
      </span>
      {summary ? <span className="card-summary" dangerouslySetInnerHTML={{ __html: summary }} /> : null}
    </>
  );
  return link ? <a className="card" href={link.getAttribute("href") ?? ""}>{body}</a> : <div className="card">{body}</div>;
}

export default function Cards({ html, columns }: { html: string; columns?: number | string }) {
  const list = new DOMParser().parseFromString(html, "text/html").querySelector("ul, ol");
  if (!list) return null;
  const n = Math.min(Math.max(Number(columns) || 0, 0), 4);
  const style = n >= 2 ? { gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))` } : undefined;
  return (
    <div className={n >= 2 ? "cards cards-fixed" : "cards"} style={style}>
      {Array.from(list.children).map((item) => <Card item={item} />)}
    </div>
  );
}
