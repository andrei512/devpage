// A Mermaid diagram, drawn by pele. Every mermaid fence renders through it.
import pele from "pele";

export default function Diagram({ source }: { source: string }) {
  try {
    const svg = pele.render(source).svg;
    return <div className="diagram" dangerouslySetInnerHTML={{ __html: svg }} />;
  } catch (error: any) {
    return <pre className="diagram-error">{`Diagram error: ${error.message}\n\n${source}`}</pre>;
  }
}
