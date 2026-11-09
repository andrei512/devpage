# devpage

Live documents from a repository. devpage serves the Markdown in your repo as
a site, a doc or a slide deck, and draws your UI designs from `.design` files.
Open pages update the moment a file is saved, whether you or an agent saved
it. It also has comments, link checking, PDF and video.

**Docs:** [devpage.ai](https://devpage.ai/docs/README.md), served by devpage
from [`docs/`](docs/README.md).

## Quickstart

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/andrei512/devpage/releases/latest/download/devpage-installer.sh | sh
devpage README.md
```

`devpage README.md` prints an address such as `http://localhost:3030`. Open it
and edit the file: the page updates without a reload.

```sh
devpage docs              # serve the site, doc or deck in docs/devpage.yml
devpage check docs        # check every link; exit 1 if one is broken
devpage pdf talk          # print the doc or deck in talk/devpage.yml to <title>.pdf
devpage png app.design    # draw a design to a PNG; --slide <n> for a deck's slide
devpage checkout.design   # draw a design on a canvas
devpage --help            # every command
```

The quickstart's `curl` installs the prebuilt binary on macOS or Linux. Windows
has a PowerShell one-liner, and `cargo install devpage --locked` builds it with Rust 1.97 or
later ([Installing](https://devpage.ai/docs/installing.md)).

## What it does

- [**Sites, docs and decks**](https://devpage.ai/docs/boxes/viewer/writing-a-document/README.md):
  Markdown plus a small `devpage.yml` that names the kind. A lone Markdown
  file is a doc on its own.
- [**Live updates**](https://devpage.ai/docs/boxes/viewer/live-updates/README.md):
  a save refreshes every open page in place, without a build step.
- [**Designs**](https://devpage.ai/docs/boxes/viewer/designs/README.md): a UI
  an agent writes as a `.design` file, clicked through as a prototype and
  fetched as a PNG so the agent can see its work.
- [**Diagrams**](https://devpage.ai/docs/boxes/viewer/diagrams/README.md) and
  [**widgets**](https://devpage.ai/docs/boxes/viewer/widgets/README.md):
  `mermaid` fences drawn as diagrams, and React components used as tags.
- [**Code reference**](https://devpage.ai/docs/boxes/viewer/code-reference/README.md):
  a page for every module, type and function of a site's Rust code, read
  from the code as it is now, with links to it checked.
- [**Comments**](https://devpage.ai/docs/boxes/viewer/comments/README.md):
  threads on selected text, kept as files in `.devpage/` that an agent can
  read and resolve.
- [**Checked links**](https://devpage.ai/docs/boxes/viewer/checking-links/README.md),
  [**PDF**](https://devpage.ai/docs/boxes/viewer/pdf/README.md) and
  [**video**](https://devpage.ai/docs/boxes/viewer/making-a-video/README.md).
- [**Deployed for a team**](https://devpage.ai/docs/boxes/viewer/deploying/README.md)
  behind the auth proxy you already have, or as a
  [server](https://devpage.ai/docs/boxes/viewer/server/README.md) that keeps
  every published version.

## With an agent

`check`, `png` and the server commands answer in JSON with `--json`, and
every command exits 0 when done, 1 when the document has problems, and 2 when
devpage couldn't run.
[Working with an agent](https://devpage.ai/docs/working-with-an-agent.md) has
a block to paste into your `AGENTS.md` or `CLAUDE.md`: how the agent checks
its links, looks at its designs and slides, and resolves your comments.

## Licence

MIT. See [LICENSE](LICENSE) and the [changelog](CHANGELOG.md).
