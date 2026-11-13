// A part's requirements, from the list the server writes: each item's link is
// a requirement and the text after " — " says whether it is met, or its kind
// when no test checks it. An item with a nested list is a part above, whose
// requirements apply here too.
const LABELS: Record<string, string> = {
  "met": "Met",
  "not met": "Not met",
  "no test": "No test",
  "not checked": "Not checked",
  "review rule": "Review rule",
  "objective": "Objective",
};
const TESTED = ["met", "not met", "no test", "not checked"];

// An item's own line: a <p> in a loose list, the item itself in a tight one.
const line = (item: Element) => item.querySelector(":scope > p") ?? item;
const link = (item: Element) => line(item).querySelector(":scope > a") as HTMLAnchorElement | null;
const nested = (item: Element) => item.querySelector(":scope > ul");
// What follows the last " — " of the item's own line.
const status = (item: Element) => {
  const text = Array.from(line(item).childNodes).filter((n) => !(n instanceof HTMLUListElement)).map((n) => n.textContent ?? "").join("");
  return text.split(" — ").pop()?.trim() ?? "";
};

function Row({ item }: { item: Element }) {
  const to = link(item);
  if (!to) return null;
  const is = status(item);
  return (
    <a className="requirement" href={to.getAttribute("href") ?? ""}>
      <span className={"requirement-status status-" + is.replace(/ /g, "-")}>{LABELS[is] ?? is}</span>
      <span className="requirement-title" dangerouslySetInnerHTML={{ __html: to.innerHTML }} />
    </a>
  );
}

export default function Requirements({ html }: { html: string }) {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const list = parsed.querySelector("ul");
  if (!list) return <p className="requirements-none">This part has no requirements.</p>;
  const items = Array.from(list.children);
  const own = items.filter((item) => !nested(item));
  const above = items.filter((item) => nested(item));
  const rows = [...own, ...above.flatMap((item) => Array.from(nested(item)?.children ?? []))];
  const tested = rows.some((row) => TESTED.includes(status(row)));
  const checked = Number(parsed.querySelector("[data-checked]")?.getAttribute("data-checked"));
  return (
    <div className="requirements">
      {own.length ? <div className="requirements-list">{own.map((item) => <Row item={item} />)}</div> : null}
      {above.map((item) => {
        const under = Array.from(nested(item)?.children ?? []);
        return (
          <details className="requirements-above">
            <summary>{under.length} from {link(item)?.textContent ?? "above"}</summary>
            <div className="requirements-list">{under.map((row) => <Row item={row} />)}</div>
          </details>
        );
      })}
      {tested ? (
        <p className="requirements-checked">
          {checked
            ? "Tests last run " + new Date(checked * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) + "."
            : "Tests not run yet: run devpage check."}
        </p>
      ) : null}
    </div>
  );
}
