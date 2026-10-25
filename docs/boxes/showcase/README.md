# The showcase deck

The design of devpage's own presentation, `docs/talks/devpage/`: what it is
for, the slides it has and what each one proves, and the rules every slide
follows. The deck is devpage demonstrating itself, so it is held to the same
bar as the code: when devpage gains a capability a user sees, the deck shows
it ([the rule](../../constraints/features-arrive-on-the-deck.md)). It belongs
to the [system catalog](../README.md).

## Three examples, one per kind

The deck is one of three documents devpage keeps as live examples of its
kinds, all real and all in this repository, reached from the navbar's
**Examples** menu:

| Kind | Example | Why it is real |
| --- | --- | --- |
| site | these docs, `docs/devpage.yml` | devpage's documentation |
| doc | the feature catalog, `docs/features/devpage.yml` | a quick tour of every feature, one short chapter per area, printable |
| slides | this deck, `docs/talks/devpage/devpage.yml` | devpage's presentation |

## What it is for

Someone who has never seen devpage watches the deck — or clicks through it —
and leaves knowing what devpage does, why it beats the way they write docs
today, and how to start. It is a pitch, not a manual: the feature catalog doc
explains each feature, and the docs site teaches them.
Every slide **shows** the capability it talks about, built with that
capability: the slide about widgets is made of widgets, the slide about
diagrams is a clickable diagram, the slide about PDF is in the PDF.

## The slides

One idea per slide, in this order. Each row names the capability a slide
proves; a capability with no row isn't on the deck yet.

| # | Slide | Proves | Built with |
| --- | --- | --- | --- |
| 1 | devpage | what it is, in one line | the logo, big type |
| 2 | Documents are scattered | the problem | a picture of wikis, slide tools and chat |
| 3 | One repository, three kinds | site, doc and slides | `KindCards` |
| 4 | Live | a save updates every open page | a diagram of the live-update loop |
| 5 | Extended Markdown | GFM, code, raw HTML | `Example`, source beside result |
| 6 | Diagrams are navigation | clickable Mermaid | a clickable diagram |
| 7 | Widgets | React components in Markdown | `Example` of a widget, `Tabs` |
| 8 | Your own widgets | project widgets in TSX | `FileTree` of `docs/widgets/`, the component's source |
| 9 | Comments, with agents | comment mode on, comment, tell the agent, resolved | a diagram of the loop, the thread file |
| 10 | Checked links | the problems bar, `devpage check` | `Terminal` with real output |
| 11 | Paper | `devpage pdf` matches the screen | `Terminal` with the real command, and the pages it prints, drawn |
| 12 | Navigation | navbar, sidebar, light and dark | the site's frame drawn in light and dark: navbar, open menu, theme switch, sidebar |
| 13 | Deployed | behind your auth proxy | a diagram of proxy → devpage → volume |
| 14 | Start | `cargo install devpage` | `Terminal` |

## Rules for every slide

- **Show, don't list.** Every slide carries a visual — a widget, a diagram, a
  terminal, an example — and at most a headline and one line of text. No slide
  is a bullet list.
- **One idea.** If a slide needs two headlines, it is two slides.
- **Built with what it shows**, as above; never a screenshot of devpage where
  devpage itself can draw the thing.
- **Fits the stage.** Nothing scrolls or overflows at 1920 × 1080, and every
  slide reads in light and dark and in the PDF.
- **True.** Commands, output and file names on a slide are real; a terminal
  shows what the command really prints.

## Constraints

- [A feature a user sees arrives on the deck](../../constraints/features-arrive-on-the-deck.md)

## Topics

### Constraints

- [A feature a user sees arrives on the deck](../../constraints/features-arrive-on-the-deck.md)
  — the change that adds it adds or remakes its slide.
