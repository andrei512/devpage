---
page-type: task
---
# Turn it on

How to get [the live session](../../README.md) on a project: three steps,
once. This page also says exactly when the session is on, how to turn it off
for one project, and how to run its models on Azure.

## Steps

<Steps>

1. Install the session's program. The install script does it. After
   `cargo install`, or when devpage says so, run:

   ```sh
   devpage setup
   ```

2. Give devpage a key for the session's models, in its environment:

   ```sh
   export DEVPAGE_OPENAI_API_KEY=…
   ```

3. Serve your project as always: `devpage`. An orb is in the bottom right
   corner of every page. Press it to start the session, and again to stop it.

</Steps>

The slow agents are the coding agents installed on your machine, Claude Code
and Codex, each on the login it already has. devpage holds no key of theirs.

Step 1 is repeated for each new version of devpage. An updated devpage that
finds the older program says `No live session: the program in <folder> is
another devpage's: run `devpage setup`` and serves without the session.

## When it is on

The session is on when all three hold:

- **devpage serves on a loopback address.** A deployed devpage, and `devpage
  server`, never have it. This is the condition for starting any
  [extension's program](../../../extensions/README.md#a-program-beside-devpage).
- **The environment names where the models run:** `DEVPAGE_OPENAI_API_KEY` is
  set, or [for Azure](#on-azure) both `DEVPAGE_AZURE_OPENAI_ENDPOINT` and
  `DEVPAGE_AZURE_OPENAI_API_KEY` are. An empty one counts as not set.
- **The program is installed:** the one `devpage setup` unpacked, or the
  folder the [`program` setting](../settings/README.md#devpages-own-setting)
  names.

With the key set and no program, devpage says so once as it starts, `No live
session: its program is not installed: run `devpage setup``, and serves
without the session. Without the key it says nothing: the session is off.
`devpage check`, `pdf`, `png` and `video` never have it.

When it is on, every document in the served folder has it without naming it,
a Markdown file served alone included; a design's own page does not. There is
one session for the served folder, whichever document's page asks.

## Turn it off for one project

In the project's `devpage.yml`:

```yaml
extensions:
  live:
    config:
      enabled: false
```

The project's pages then show none of the session's widgets and load none of
their code. The same `config` holds the project's own
[settings](../settings/README.md).

An `extensions.live` entry with a `package` replaces the built-in session
with that package, as for any
[extension](../../../extensions/README.md#enable-a-package).

## On Azure

Voice's model and Fast's model run on an Azure OpenAI resource when devpage's
environment names one, with the variables
[the video narrator](../../../share/making-a-video/README.md#the-narrator)
reads:

```sh
export DEVPAGE_AZURE_OPENAI_ENDPOINT=https://<resource>.openai.azure.com/
export DEVPAGE_AZURE_OPENAI_API_KEY=…
```

- **The endpoint being set selects Azure,** also beside an OpenAI key. With
  the endpoint and no Azure key there is no session.
- **On Azure a model is named by its deployment.** `DEVPAGE_LIVE_MODEL` and
  the `fast` setting name deployments. Without them the session asks for
  deployments named `gpt-live-1` and `gpt-6-luna`, and for one named
  `gpt-6.1-sol` when the resource refuses Fast's.
- **The slow agents are not affected.** Claude Code and Codex keep their own
  logins.
