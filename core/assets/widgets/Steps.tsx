// Numbered steps from a Markdown list: each item's first line is its title,
// the rest its body, and a paragraph of "$ " lines becomes command chips.
const BLOCK = /^(P|UL|OL|PRE|DIV|TABLE|BLOCKQUOTE|FIGURE|ASIDE|H[1-6])$/;
const html = (n: Node) =>
  n instanceof Element ? n.outerHTML : (n.textContent ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;");

function Commands({ lines }: { lines: string[] }) {
  return (
    <div className="step-commands">
      {lines.map((l) => <code className="step-command"><span className="step-prompt">$</span>{l.slice(2)}</code>)}
    </div>
  );
}

function Step({ item, number }: { item: Element; number: number }) {
  const nodes = Array.from(item.childNodes);
  const isBlock = (n: Node) => n instanceof Element && BLOCK.test(n.tagName);
  // A loose list wraps the first line in a <p>; a tight one doesn't.
  const firstBlock = nodes.find(isBlock) as Element | undefined;
  const loose = firstBlock?.tagName === "P" && nodes.filter((n) => !isBlock(n)).every((n) => !(n.textContent ?? "").trim());
  const head = loose ? firstBlock!.innerHTML : nodes.filter((n) => !isBlock(n)).map(html).join("");
  const blocks = nodes.filter(isBlock).slice(loose ? 1 : 0) as Element[];
  const [title, ...rest] = head.trim().split("\n");
  // Lines of the first paragraph that start with "$ " are commands too.
  const text = (line: string) => new DOMParser().parseFromString(line, "text/html").body.textContent ?? "";
  const leadCommands = rest.map(text).map((l) => l.trim()).filter((l) => l.startsWith("$ "));
  const lead = rest.filter((l) => !text(l).trim().startsWith("$ ")).join("\n").trim();
  return (
    <li className="step">
      <span className="step-number" aria-hidden="true">{number}</span>
      <div className="step-content">
        <div className="step-title" dangerouslySetInnerHTML={{ __html: title }} />
        {lead ? <p dangerouslySetInnerHTML={{ __html: lead }} /> : null}
        {leadCommands.length ? <Commands lines={leadCommands} /> : null}
        {blocks.map((block) => {
          const lines = (block.textContent ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
          if (block.tagName === "P" && lines.length && lines.every((l) => l.startsWith("$ "))) {
            return <Commands lines={lines} />;
          }
          return <div className="step-block" dangerouslySetInnerHTML={{ __html: block.outerHTML }} />;
        })}
      </div>
    </li>
  );
}

export default function Steps({ html: content }: { html: string }) {
  const list = new DOMParser().parseFromString(content, "text/html").querySelector("ol, ul");
  if (!list) return null;
  const start = Number(list.getAttribute("start") ?? 1);
  return (
    <ol className="steps">
      {Array.from(list.children).map((item, i) => <Step item={item} number={start + i} />)}
    </ol>
  );
}
