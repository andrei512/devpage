// A highlighted box: note (default), tip, warning or danger, with an optional title.
const icons: Record<string, string> = { note: "ℹ", tip: "✓", warning: "!", danger: "✕" };

export default function Callout({ kind = "note", title, children }: { kind?: string; title?: string; children?: any }) {
  return (
    <aside className={`callout callout-${kind}`}>
      <div className="callout-head">
        <span className="callout-icon" aria-hidden="true">{icons[kind] ?? icons.note}</span>
        {title ? <strong>{title}</strong> : null}
      </div>
      <div className="callout-body">{children}</div>
    </aside>
  );
}
