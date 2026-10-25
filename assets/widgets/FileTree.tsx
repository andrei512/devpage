// A folder tree, from a nested Markdown list: a name ending in / is a folder.
function Entries({ list }: { list: Element }) {
  return (
    <ul>
      {Array.from(list.children).map((item) => {
        const nested = item.querySelector(":scope > ul");
        const label = Array.from(item.childNodes).filter((n) => n !== nested);
        const text = label.map((n) => n.textContent).join("").trim();
        const [name, ...note] = text.split(" — ");
        const link = item.querySelector(":scope > a, :scope > p > a") as HTMLAnchorElement | null;
        const folder = name.endsWith("/") || nested !== null;
        return (
          <li className={folder ? "folder" : "file"}>
            <span className="filetree-icon" aria-hidden="true">{folder ? "▸" : "·"}</span>
            {link ? <a href={link.getAttribute("href") ?? ""}>{name}</a> : <span>{name}</span>}
            {note.length ? <span className="filetree-note">{note.join(" — ")}</span> : null}
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
