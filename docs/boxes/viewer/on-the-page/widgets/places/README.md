---
page-type: concept
---
# Places

A place shows a widget outside a page's text, on every page of a document: a button beside the theme selector, a panel beside the page, a control floating over it. This box belongs to [widgets](../README.md).

## Name a place's widgets

A site, doc or deck lists them under `places`, by place, in the order they show:

```yaml
kind: site
title: Garden
root: README.md
widgets: widgets
places:
  navbar: [Clock]
  panel: [Checklist]
  float: [Timer]
```

| Place | Where it is |
| --- | --- |
| `navbar` | Before the theme selector: at the end of a site's navbar, or in the top-right corner of a page that has none. |
| `panel` | The window's right edge, 300 pixels wide. |
| `float` | Over the page, at the middle of the window's bottom edge. |

Each name is a widget of the document: a [system widget](../system-widgets/README.md), a [project widget](../project-widgets/README.md) or an extension's. It is rendered with no props and no content. A name that is no widget is a problem, shown on every page and reported by `devpage check`: `places: <place>: no widget is named <Name>`. A place devpage doesn't have makes the document invalid.

## Widgets devpage places

An enabled package can list widgets under `places` in its [manifest](../../../extensions/reference/README.md#bundle). The page appends those widgets to the document’s own lists. Each widget in `panel` occupies a section in their shared panel.

Where [the live session](../../../live-session/README.md) is on, devpage adds
its widgets to a document's places, after the document's own: the tasks
button to `navbar`, the task panel to `panel`, the canvas and the orb to
`float`. The document names none of them, and a document that
[turns the session off](../../../live-session/use/turn-it-on/README.md#when-it-is-on) gets
none. The orb moves its place to the window's bottom right corner.

## How long a placed widget lives

A placed widget is mounted once and stays mounted while the tab shows the document: a link to another page, a live update and Back leave it and its state as they are. It is mounted again when its module changed, or one it renders by name. A page loaded whole mounts it anew.

Places are never printed, and a deck hides them in fullscreen. A design's own page has none.

## The panel

On a window at least 1100 pixels wide that shows no deck, the panel is beside the page: the page gives it the room, and [comment threads](../../../share/comments/README.md) become the list their button opens instead of cards beside the text. On a narrower window and on a deck the panel is closed until a widget opens it, and then lies over the page's right edge; under 860 pixels it takes the window's width.

A widget opens and closes it with [`page.panel`](../page-module/README.md#the-panel): a button in the navbar's place is the usual way to give the reader the choice.
