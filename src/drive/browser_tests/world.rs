//! The live session's world on a page, in chrome-headless-shell: a widget
//! reads a part's state and changes it through a function of the part, over
//! the connection the session's own widgets hold. A small program stands in
//! for the session's, which would start a coding agent: it keeps one part as
//! the session's registry does, sends it to each page and takes calls.

use std::sync::Arc;

use super::page_script::widget_folder;
use crate::adapters::chrome::testing::{self, eval, until_is};
use crate::adapters::processes::Processes;
use crate::drive::http;
use devpage_core::document::Document;
use devpage_core::folder::ServedFolder;

/// The stand-in for the session's program: one part, `counter`, sent to a
/// page when it connects and to every page when a call changed it.
const PROGRAM: &str = r#"const sockets = new Set();
let count = 0;
const part = () => JSON.stringify({ type: "state", part: "counter", state: { count } });
Bun.serve({
  hostname: "127.0.0.1",
  port: Number(process.env.DEVPAGE_PORT),
  fetch: (request, server) => (server.upgrade(request) ? undefined : new Response("not found", { status: 404 })),
  websocket: {
    open(socket) {
      sockets.add(socket);
      socket.send(JSON.stringify({ type: "state", voice: "off", problem: null, tasks: [], agents: [], subject: null, canvas: null }));
      socket.send(part());
    },
    message(_socket, message) {
      const said = typeof message === "string" ? JSON.parse(message) : {};
      if (said.type !== "call" || said.name !== "counter.add") return;
      count += said.input.by;
      for (const socket of sockets) socket.send(part());
    },
    close: (socket) => void sockets.delete(socket),
  },
});
"#;

/// The widget: the count as the session last sent it, and a click that adds.
const WIDGET: &str = r#"import { useWorld, world } from "devpage";
export default function Counter() {
  const state = useWorld("counter");
  return <button className="counter" onClick={() => world.call("counter.add", { by: 2 })}>{state ? String(state.count ?? 0) : "none"}</button>;
}
"#;

/// Drives chrome-headless-shell and runs the stand-in with devpage's own
/// bun; run with `cargo test -- --ignored`. Where `devpage setup` has not
/// installed bun there is nothing to run it with, and the test says so.
#[tokio::test]
#[ignore]
async fn a_widget_reads_a_part_of_the_world_and_changes_it_through_the_session() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return eprintln!("skipped: no home folder");
    };
    let bun = crate::adapters::live::Installed::new(&home.join(".devpage")).bun();
    if !bun.is_file() {
        return eprintln!("skipped: devpage's bun is not installed");
    }
    crate::adapters::extensions::install();
    // The stand-in is the document's own `live` package, so devpage places
    // none of the session's widgets: the document places one itself, which
    // opens the session's connection as on any page.
    let dir = widget_folder(
        "world",
        "# Home\n",
        &[
            (
                "docs/devpage.yml",
                "kind: site\ntitle: W\nroot: README.md\nwidgets: widgets\nplaces:\n  navbar: [DevpageLiveTasksButton]\n  panel: [Counter]\nextensions:\n  live:\n    package: ./session\n",
            ),
            ("docs/widgets/Counter.tsx", WIDGET),
            (
                "docs/session/extension.yml",
                "id: live\nversion: \"1\"\napi: 1\nrun: [bun, program.ts]\n",
            ),
            ("docs/session/program.ts", PROGRAM),
        ],
    );
    let scope = ServedFolder::new(&dir).unwrap();
    let document = Arc::new(Document::load(&dir.join("docs"), &scope).unwrap());
    let processes = Processes::new("http://localhost:1".into(), "T".into(), bun);
    let address = testing::serve(http::router(
        http::App::testing(document, Default::default()).with_processes(processes.clone()),
    ))
    .await;
    let browser = testing::browser_on(1400, 900).await;
    let page = testing::open(&browser, &format!("{address}/docs/README.md")).await;
    let count = "document.querySelector('.counter')?.textContent ?? ''";
    // The part's state arrives on the session's connection: nothing was counted yet.
    until_is(&page, count, "0").await;
    eval::<bool>(&page, "(document.querySelector('.counter').click(), true)").await;
    until_is(&page, count, "2").await;
    eval::<bool>(&page, "(document.querySelector('.counter').click(), true)").await;
    until_is(&page, count, "4").await;
    // The count is the session's, not the page's: a page loaded anew is sent it.
    let again = testing::open(&browser, &format!("{address}/docs/README.md")).await;
    until_is(&again, count, "4").await;
    processes.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// The manifest brings a whole widget module tree and appends a panel section
/// to the document's own. The shared page module is available to both.
#[tokio::test]
#[ignore]
async fn a_package_places_a_widget_and_serves_its_relative_imports() {
    crate::adapters::extensions::install();
    let dir = widget_folder(
        "package-panel",
        "# Home\n",
        &[
            (
                "docs/devpage.yml",
                "kind: site\ntitle: Panel\nroot: README.md\nwidgets: widgets\nplaces: {panel: [First]}\nextensions: {second: {package: ./second}}\n",
            ),
            (
                "docs/widgets/First.tsx",
                "export default () => <section>First</section>;",
            ),
            (
                "docs/second/extension.yml",
                "id: second\nversion: '1'\napi: 1\nwidgets: {Second: widgets/Second.tsx}\nplaces: {panel: [Second]}",
            ),
            (
                "docs/second/widgets/Second.tsx",
                "import { label } from '../label'; import { useWorld } from 'devpage'; export default () => <section>{useWorld('second')?.label ?? label}</section>;",
            ),
            ("docs/second/label.ts", "export const label = 'Second';"),
        ],
    );
    let (_browser, page) = super::page_script::open(&dir, "/docs/README.md").await;
    until_is(&page, "[...document.querySelectorAll('#devpage-place-panel section')].map(el => el.textContent).join(',')", "First,Second").await;
    std::fs::remove_dir_all(dir).unwrap();
}
