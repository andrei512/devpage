// Node's test runner feeds the reporter structured events. Test output and
// diagnostics stay separate from the records consumed by the extension.
export default async function* results(events) {
  for await (const { type, data } of events) {
    if (type !== "test:pass" && type !== "test:fail") continue;
    if (data.details?.type === "suite") continue;
    const { file, line, column, name } = data;
    const status = data.skip || data.todo ? "skipped" : type === "test:pass" ? "passed" : "failed";
    yield JSON.stringify({ file, line, column, name, status }) + "\n";
  }
}
