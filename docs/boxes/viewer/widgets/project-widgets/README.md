# Writing a project widget

How a project adds its own widgets: the folder they live in, what a widget
file looks like, how widgets compose and share helpers, and how they are
styled. It is a task article under [Widgets](../README.md).

## The widgets folder

A project adds widgets in one folder, named by its document file's `widgets`
field, relative to that file:

<FileTree>

- my-project/docs/
  - devpage.yml — `widgets: widgets`
  - widgets/
    - Chart.tsx — a widget: `<Chart>`
    - Timeline.tsx — a widget: `<Timeline>`
    - format.ts — a helper the widgets import; names no tag

</FileTree>

```yaml
kind: site
title: devpage
root: README.md
widgets: widgets          # every capitalized .tsx in docs/widgets/ is a widget
```

Every `.tsx` file in the folder whose name starts with a capital letter is a
widget, named by its file name: `KindCards.tsx` is `<KindCards>`. Files whose
names start with a lowercase letter — `format.ts`, `cards.tsx` — are helpers
that widgets import; they name no tag. The field works the same for every kind;
a doc's or deck's widgets are its own.

<Callout kind="warning" title="One name, one widget">
A project widget can't take a system widget's name: `Callout.tsx` in a project
is a problem, so system widgets that use `Callout` keep getting the one they
were written for ([the rule](../../constraints/one-name-one-widget.md)).
`Notes.tsx` is a problem too: `Notes` is devpage's tag for a slide's
[notes](../../making-slides/README.md#speaker-notes).
</Callout>

## A widget file

A widget is a React component — its default export — written in TSX. devpage's
own docs have one, `docs/widgets/KindCards.tsx`; trimmed to its shape:

```tsx
// docs/widgets/KindCards.tsx
const kinds = [
  { name: "Site", kind: "site", href: "/docs/boxes/viewer/making-a-site/README.md" },
  { name: "Doc", kind: "doc", href: "/docs/boxes/viewer/writing-a-doc/README.md" },
  { name: "Slides", kind: "slides", href: "/docs/boxes/viewer/making-slides/README.md" },
];

export const css = `
  .kind-cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr)); gap: 1.25rem; }
  .kind-cards a.kind-card { border: 1px solid var(--border); border-radius: 18px; color: inherit; }
  .kind-cards a.kind-card:hover { border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); }
`;

export default function KindCards() {
  return (
    <div className="kind-cards">
      {kinds.map((kind) => (
        <a className="kind-card" href={kind.href}>
          <div className="kind-sketch" aria-hidden="true"><Sketch kind={kind.kind} /></div>
          <strong>{kind.name}</strong>
          <code className="kind-field">kind: {kind.kind}</code>
        </a>
      ))}
    </div>
  );
}
```

`Sketch` is a second component in the same file, drawing each kind's little
picture. Written as `<KindCards />`, it renders:

<KindCards />

- **It is just React.** Hooks, state, effects and events all work; devpage ships
  React and runs each widget with it.
- **Widgets compose.** A widget can render any other widget — a system widget or
  one of the project's — by name, without importing it.
- **Helpers.** A widget can import `.ts` and `.tsx` files from the same folder
  (`import { format } from "./format"`); they are compiled with it. npm
  packages can't be imported — devpage has no package manager.
- **Images** are referenced by their address from the top of the served folder
  (`/docs/images/logo.svg`), since a widget appears on many pages.

devpage compiles each widget from TSX when a page needs it, and again when its
file changes — saving a widget or a helper updates every open page that uses
it, like saving a page does, whatever a site's `watch` list says. There is no
build step. A page of a site has that site's widgets wherever it is shown,
printed on its own included.

## Styling

A widget is styled the way any React component is, in either way or both:

- **Inline styles** — `style={{ borderColor: "var(--accent)" }}`.
- **Its own CSS**, optionally, exported from the same file as a string — as
  `KindCards` does above, under its own classes (`.kind-cards`, `.kind-card`).
  devpage adds it to the page once.

The theme defines no classes for widgets to borrow; what a widget shares with
the page is the theme's **tokens**. Written with them — `var(--text)`,
`var(--muted)`, `var(--accent)`, `var(--border)`, `var(--surface)`,
`var(--font)`, `var(--font-mono)` — a widget follows the light and dark themes
with nothing else to do; where it must differ beyond the tokens, scope the
rule to `:root[data-theme="dark"]`, which follows the reader's choice in the
theme switch ([Themes](../../themes/README.md#the-tokens)).

What a failing widget looks like is on
[Using widgets](../using-widgets/README.md#when-a-widget-fails).
