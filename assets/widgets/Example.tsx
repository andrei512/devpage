// Markdown source, highlighted, beside what it renders to.
export default function Example({ title, source, sourceHtml, children }: { title?: string; source: string; sourceHtml?: string; children?: any }) {
  return (
    <figure className="example">
      {title ? <figcaption>{title}</figcaption> : null}
      <pre className="example-source">
        {sourceHtml !== undefined ? <code dangerouslySetInnerHTML={{ __html: sourceHtml }} /> : <code>{source}</code>}
      </pre>
      <div className="example-result">{children}</div>
    </figure>
  );
}
