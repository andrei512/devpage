# Links between pages and documents

What a link opens: a page, a document of another kind, or a heading — and why
relative links work exactly as they do on disk. It is a concept article under
[Writing a document](../README.md).

## Addresses are file paths

Because addresses are file paths, a relative link between files works
exactly as it does when the files are read on disk — including one
that climbs out of the docs folder to the repository's `README.md`:

```markdown
See [live updates](../live-updates/README.md#the-page-script).
Read the [sync design](../../design/sync/devpage.yml) or watch the
[launch talk](../../talks/launch/devpage.yml#3).
```

## What each link opens

| A link to | Opens |
| --- | --- |
| a Markdown file | that page, in the document it belongs to: inside a site's folder it shows in that site, with its sidebar; a page that belongs to no site shows as a one-chapter doc |
| a document file | that document as its kind — a site at its root page, a doc whole, a deck at its first slide, or at slide *n* with `#n` |
| a heading, `#two-words` | that section — every heading gets an id from its text (`## Two words` → `#two-words`); within a doc, links between its chapters and to their headings scroll within the doc |

A link within the same document opens in place, without a full reload. Which
links appear in a site's sidebar is decided only by its pages' `## Topics`
sections — see [Organizing navigation](../../organizing-navigation/README.md).
Every link is checked: a broken one is marked on the page and listed at its
top while you write ([Checking links](../../checking-links/README.md)).
