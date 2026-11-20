---
page-type: task
---
# Write a part

How to add a [world model part](../README.md#a-world-model-part) to the live
session, from an empty folder to a session that uses it. The example keeps
notes: you say "note that this limit is too low", the note shows in the side
panel, and an agent that later works on that page is told about it.

You need a project you already serve [with the session on](../../use/turn-it-on/README.md).

## Steps

<Steps>

1. **Make the package.** A part ships in an extension package: a folder with
   a manifest. The part's name is the package's `id`.

   <FileTree>

   - notes/
     - extension.yml
     - part.ts
     - fast.md
     - Notes.tsx

   </FileTree>

   ```yaml
   # notes/extension.yml
   id: notes
   version: "1"
   api: 1
   world: part.ts
   ```

2. **Declare the state, one entry kind and one function.** `part.ts` exports
   one function that returns the part.

   ```ts
   // notes/part.ts
   export default ({ z }) => ({
     state: { notes: z.array(z.object({ text: z.string(), page: z.string() })).default([]) },
     entries: {
       noted: { fields: { text: z.string() }, line: (e) => `Noted: "${e.text}"`, wakes: false },
     },
     functions: {
       add: {
         for: "fast",
         description: "Keeps a note the user dictated.",
         input: { text: z.string(), page: z.string() },
         run: ({ text, page }, now) => {
           now.state.notes.push({ text, page });
           now.add("noted", { text });
         },
       },
     },
   });
   ```

   `wakes: false` because Fast made the call itself and has nothing to react
   to.

3. **Tell Fast when to use it.** Fast now has `notes.add` in its list of
   calls, made from the function. When to call it is yours to say, in
   `fast.md`:

   ```md
   # Notes

   When the user asks to note, remember or keep something for later, keep it
   with notes.add, with their words as the text and the page they said it on.
   Say "Noted." and nothing else. A note is never work for an agent.
   ```

   The heading is not one of the core's, so Fast gets a new section.

4. **Give it to the agents.** A note is useful when work on that page starts.
   Add to the object in `part.ts`:

   ```ts
   addToTask: (state, task) =>
     state.notes.filter((n) => n.page === task.page).map((n) => `A note on this page: "${n.text}"`),
   ```

5. **Show it.** Declare the widget and its place in the object in `part.ts`:

   ```ts
   widgets: { Notes: "panel" },
   ```

   The widget is the file of that name beside `part.ts`. It reads the part's
   state with one call.

   ```tsx
   // notes/Notes.tsx
   import { useWorld } from "devpage";

   export default function Notes() {
     const notes = useWorld("notes")?.notes ?? [];
     return <ul>{notes.map((n) => <li>{n.text}</li>)}</ul>;
   }
   ```

6. **Enable it** in the project's `devpage.yml`, and restart `devpage`.

   ```yaml
   extensions:
     notes:
       package: ./notes
   ```

</Steps>

## Check it

1. **Read Fast's prompt.** In devpage's `live/` folder, `bun prompt.ts fast`
   run for your project prints the prompt with your section and your call in
   it, each marked `notes`.

   ```
   ── made from functions: devpage, canvas, notes
   …
   notes.add(text, page)       // Keeps a note the user dictated.
   ── notes: fast.md · Notes
   When the user asks to note, remember or keep something…
   ```

2. **Say it.** Start the session and say "note that this limit is too low".
   You hear "Noted.", and the note is in the side panel.
3. **Give work on that page.** Say "fix this page". The task the agent gets
   ends with `A note on this page: "this limit is too low"`. Open the agent's
   chat from the side panel to see it.

## When it does not work

- **`Extension notes: …` as devpage starts:** the manifest or `part.ts` did
  not load. The message says which.
- **Fast does not call it:** the text in `fast.md` is the thing to change.
  Read the prompt again, then say it again.
- **The widget is empty:** `useWorld("notes")` is `undefined` until the
  session has sent the part's state, and where no session is on.

## Next

- A function an agent can call, or a widget's button:
  [Who may call a function](../functions/README.md).
- For every project, not one: [Where a part is enabled](../enabling/README.md).
- Every field: [Part reference](../part-reference/README.md).
