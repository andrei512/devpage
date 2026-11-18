// The durable state of one project's live session.
import { Events } from "./events.ts";
import { Database } from "bun:sqlite";
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync } from "node:fs";
import { join } from "node:path";
import type { Entry as WorldEntry } from "./world.ts";
import type { Entry as LogEntry } from "./log.ts";

export class SessionStore {
  readonly db: Database;
  readonly events: Events;

  constructor(folder: string, mode: "service" | "read" = "service") {
    const file = join(folder, "world.sqlite");
    if (mode === "read") {
      if (existsSync(file)) {
        const existing = new Database(file, { readonly: true });
        existing.run("pragma busy_timeout = 5000");
        if (existing.query("select 1 from sqlite_master where name = 'session_state'").get()) {
          this.db = existing;
          this.events = new Events(existing);
          return;
        }
        existing.close();
      }
      // Reports can still inspect a project whose service has not migrated yet.
      this.db = new Database(":memory:");
    } else {
      mkdirSync(folder, { recursive: true });
      this.db = new Database(file);
    }
    this.events = new Events(this.db);
    this.db.run("pragma busy_timeout = 5000");
    this.db.transaction(() => {
      this.db.run("create table if not exists session_state (name text primary key, value text not null)");
      this.events.install();
      this.db.run("create table if not exists session_log (id integer primary key, day text not null, value text not null)");
      if (this.get<string[]>("imported") !== undefined) {
        if (this.hasLegacyWorld()) {
          this.events.importCore(this.world());
          this.db.run("drop table session_world");
        }
        return;
      }
      const history: WorldEntry[] = [];
      const files: string[] = [];
      for (const name of ["tasks", "agents"]) {
        const file = name + ".json";
        if (!existsSync(join(folder, file))) continue;
        const value = JSON.parse(readFileSync(join(folder, file), "utf8"));
        if (name === "tasks" ? !Array.isArray(value) : !Array.isArray(value?.agents)) throw new Error("Invalid " + file);
        this.set(name, value);
        files.push(file);
      }
      const daily = join(folder, "log");
      const logs = existsSync(daily) ? readdirSync(daily).filter(name => /^\d{4}-\d\d-\d\d\.jsonl$/.test(name)).sort() : [];
      for (const file of ["world.jsonl", ...logs.map(name => join("log", name))]) {
        if (!existsSync(join(folder, file))) continue;
        for (const line of readFileSync(join(folder, file), "utf8").split("\n")) {
          if (!line.trim()) continue;
          const entry = JSON.parse(line);
          if (file === "world.jsonl") history.push(entry);
          else this.addLog(file.slice(4, 14), entry);
        }
        files.push(file);
      }
      for (const file of files) {
        if (existsSync(join(folder, file + ".migrated"))) throw new Error("Backup already exists for " + file);
      }
      this.events.importCore(history);
      this.set("imported", files);
    }).immediate();
    if (mode === "read") return;
    // A restart after the commit can finish archiving without importing twice.
    this.db.transaction(() => {
      for (const file of this.get<string[]>("imported")!) {
        const path = join(folder, file);
        if (!existsSync(path)) continue;
        if (existsSync(path + ".migrated")) throw new Error("Both legacy file and backup exist: " + path);
        renameSync(path, path + ".migrated");
      }
      this.set("imported", []);
    }).immediate();
  }

  get<T>(name: string): T | undefined {
    const row = this.db.query("select value from session_state where name = ?").get(name) as { value: string } | null;
    return row ? JSON.parse(row.value) : undefined;
  }

  set(name: string, value: unknown) {
    this.db.run("insert or replace into session_state values (?, ?)", [name, JSON.stringify(value)]);
  }

  private hasLegacyWorld() {
    return !!this.db.query("select 1 from sqlite_master where name='session_world'").get();
  }

  world(): WorldEntry[] {
    if (this.hasLegacyWorld()) {
      return this.db.query("select value from session_world order by n").values().map(([value]) => JSON.parse(String(value)));
    }
    return this.events.core();
  }

  addLog(day: string, entry: LogEntry) {
    this.db.run("insert into session_log(day, value) values (?, ?)", [day, JSON.stringify(entry)]);
  }

  logs(days = -1): LogEntry[] {
    return this.db.query("select value from session_log where day in (select distinct day from session_log order by day desc limit ?) order by id").values(days).map(([value]) => JSON.parse(String(value)));
  }
}
