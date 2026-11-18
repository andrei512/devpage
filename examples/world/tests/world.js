import { createInterface } from "node:readline";
import { relative } from "node:path";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";

const reporter = fileURLToPath(new URL("./reporter.mjs", import.meta.url));

export default function tests({ action, kind }) {
  let active = false;
  kind("result", { line: result => `${result.name} ${result.status}.` });
  kind("finished", { line: result => result.summary, wakes: true });
  action("run", { for: "slow", input: {}, description: "Run the project's Node tests and show each result in the panel." }, (_, run) => {
    if (active) throw new Error("Tests are already running.");
    run.set("summary", null);
    run.follow(async publish => {
      active = true;
      let summary;
      try {
        const root = process.env.DEVPAGE_ROOT;
        const child = Bun.spawn(["node", "--test", "--test-reporter", reporter], {
          cwd: root, stdout: "pipe", stderr: "inherit", stdin: "ignore",
        });
        const stop = () => child.kill();
        process.once("exit", stop);
        let count = 0;
        try {
          const lines = createInterface({ input: Readable.fromWeb(child.stdout), crlfDelay: Infinity });
          for await (const line of lines) {
            const result = JSON.parse(line);
            result.file = result.file ? relative(root, result.file) : "";
            const id = JSON.stringify([result.file, result.line, result.column, result.name]);
            await publish(write => {
              write.set("results", { ...write.get("results"), [id]: result });
              write.add("result", result);
            });
            count++;
          }
          const code = await child.exited;
          summary = code === 0 ? `Tests finished with ${count} results.` : `Tests failed (exit ${code}, ${count} results).`;
        } finally {
          if (child.exitCode === null) child.kill();
          await child.exited;
          process.off("exit", stop);
        }
      } catch (error) {
        summary = `Tests could not finish: ${error.message}`;
      }
      try {
        await publish(write => {
          write.set("summary", summary);
          write.add("finished", { summary });
        });
      } finally {
        active = false;
      }
    });
    return "Tests started. Results appear in the panel as they finish.";
  });
}
