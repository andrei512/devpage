import { useWorld } from "devpage";

export default function TestResults() {
  const state = useWorld("tests");
  const results = Object.entries(state?.results ?? {}) as [string, { file: string; name: string; status: string }][];
  return <section aria-label="Test results"><h2>Tests</h2>
    <ul>{results.map(([id, { file, name, status }]) =>
      <li key={id} title={file}>{name} — {status}</li>)}</ul>
    {state?.summary && <p>{state.summary}</p>}
  </section>;
}
