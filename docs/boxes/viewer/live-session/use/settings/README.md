---
page-type: reference
---
# Settings

Every setting of [the live session](../../README.md), where each is written,
where a session keeps its data, and what recording saves.

## Where a setting is written

- **For every project:** under `live` in `~/.devpage/config.yml`, the file
  that also holds the
  [server contexts](../../../share/server/reference/README.md#cli).
- **For one project:** under `extensions.live.config` in the project's
  `devpage.yml`. A setting given there replaces the one for every project.

```yaml
# ~/.devpage/config.yml
live:
  harnesses:
    claude-code: [fable, haiku]
    codex: []
  fast: gpt-6-luna
  record: true
  fullFilesystemAccess: false
```

A change takes effect when the session restarts.

## The settings

- **`harnesses`:** the coding agents the session may use, each with the
  models of it that may be used. An empty list is the agent's own default
  model. The first is the default. Without the setting it is Claude Code on
  its default model. See [Routing](../routing/README.md).
- **`fast`:** the model Fast runs on. `gpt-6-luna` when not set.
- **`record`:** `true` saves your voice. See [Recording](#recording). `false`
  when not set.
- **`fullFilesystemAccess`:** `false` when not set, and then the slow agents
  may read and write the project's files and the session's scratch pads.
  `true` lets them read and write anywhere your account may. It applies to
  new agents and to the next turn of the ones there are.
- **`enabled`:** only for one project. `false`
  [turns the session off](../turn-it-on/README.md#turn-it-off-for-one-project)
  there.

Which parts are added to the world model is not a setting of the session: a
part is an extension, [enabled as one](../../extend/enabling/README.md).

## In the environment

- **`DEVPAGE_OPENAI_API_KEY`:** the key for Voice's and Fast's models. It
  reaches the program through devpage's environment and never a page.
- **`DEVPAGE_AZURE_OPENAI_ENDPOINT`, `DEVPAGE_AZURE_OPENAI_API_KEY`:** the
  same models [on Azure](../turn-it-on/README.md#on-azure).
- **`DEVPAGE_LIVE_MODEL`:** Voice's model. `gpt-live-1` when not set.

## Routing rules

With more than one harness or model, `~/.devpage/routing.md` says in plain
words which kind of work goes where. A `routing.md` at the top of a project
replaces it whole for that project. See [Routing](../routing/README.md).

## devpage's own setting

devpage itself reads one key under `live`, `program`: a folder that takes the
installed program's place, to work on the program itself.

```yaml
live:
  program: ~/code/devpage/live
```

It is an absolute path, or one from `~`, and its `extension.yml` must name
the extension `live`. It is run as it is, with no version stamp looked for,
still by devpage's own bun. Every other key under `live` is the program's,
and devpage keeps it when it writes the file.

## Where a session is kept

Nothing of a session is written into the project. Each project's session has
a folder of its own:

<FileTree>

- ~/.devpage/live/projects/&lt;the project's path&gt;/
  - world.sqlite — the world model: its state and its full log
  - pads/ — the scratch pads, one Markdown file each
  - voice/ — recordings, only when `record` is on

</FileTree>

The project's path is written with a `-` for every character that is not a
letter or a digit. Only the program reads and writes the folder; devpage does
not. The log holds words: what was said and done.

## Recording

Recording is off until `record` is `true`. With it on, each voice session
writes two files into `voice/`, named by the time the session started:

- **A WAV file:** your microphone's sound as it was sent to the voice model,
  mono, 16-bit, 24 kHz. The assistant's voice is not in it.
- **A JSON file** of the same name: the session's id, how long the recording
  is, how much of it is you speaking, and each thing you said with where it
  is in the WAV. The times are the voice model's: close, not exact.

Both are written as the session goes, so a session cut short keeps what it
had. Nothing sends them anywhere, and nothing deletes them.
