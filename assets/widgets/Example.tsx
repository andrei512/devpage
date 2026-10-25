// Markdown source beside what it renders to.
export default function Example({ title, source, children }: { title?: string; source: string; children?: any }) {
  return (
    <figure className="example">
      {title ? <figcaption>{title}</figcaption> : null}
      <pre className="example-source"><code>{source}</code></pre>
      <div className="example-result">{children}</div>
    </figure>
  );
}
