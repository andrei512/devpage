// A folder tree, from a nested Markdown list: a name ending in / is a folder.
const escapeHtml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function Entries({ list }: { list: Element }) {
  return (
    <ul>
      {Array.from(list.children).map((item) => {
        const nested = item.querySelector(":scope > ul");
        // The entry's own line, a <p> in a loose list: its name, then — after
        // " — " — a note that keeps its inline Markdown (code, emphasis, links).
        const own = item.querySelector(":scope > p") ?? item;
        const label = Array.from(own.childNodes).filter((n) => n !== nested && !(n instanceof HTMLUListElement));
        const html = label.map((n) => (n instanceof Element ? n.outerHTML : escapeHtml(n.textContent ?? ""))).join("").trim();
        const [nameHtml, ...noteHtml] = html.split(" — ");
        const name = new DOMParser().parseFromString(nameHtml, "text/html").body.textContent?.trim() ?? "";
        const link = own.querySelector(":scope > a") as HTMLAnchorElement | null;
        const folder = name.endsWith("/") || nested !== null;
        return (
          <li className={folder ? "folder" : "file"}>
            <span className="filetree-icon" aria-hidden="true">{folder ? "▸" : "·"}</span>
            {link ? <a href={link.getAttribute("href") ?? ""}>{name}</a> : <span>{name}</span>}
            {noteHtml.length ? <span className="filetree-note" dangerouslySetInnerHTML={{ __html: noteHtml.join(" — ") }} /> : null}
            {nested ? <Entries list={nested} /> : null}
          </li>
        );
      })}
    </ul>
  );
}

export default function FileTree({ html }: { html: string }) {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const list = parsed.querySelector("ul");
  return <div className="filetree">{list ? <Entries list={list} /> : null}</div>;
}
