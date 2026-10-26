# Making a site

How to make a site — a project's documentation as many pages, with a sidebar
built from their Topics sections and, if you want one, a navbar across the
top. It covers the site's document file, its root page, how pages join the
sidebar, and the navbar, logo and theme switch. It is a task article under the
[viewer](../README.md); the document file in general is on
[Writing a document](../writing-a-document/README.md).

## A site

<FileTree>

- docs/
  - devpage.yml — the site: its title, root page, navbar and logo
  - README.md — the root page, where the site opens
  - getting-started.md — a page the root's Topics lists
  - guide/
    - README.md — a page with Topics of its own
  - logo.svg — shown before the title and as the tab's icon

</FileTree>

```yaml
kind: site
title: My project
root: README.md
logo: logo.svg
navbar:
  Docs (book-open): README.md
  Getting started (rocket): getting-started.md
  (github): https://github.com/me/my-project
```

Run it from the top of the repository, so pages can link anywhere in it:

<Terminal>

```text
$ devpage docs
My project → http://localhost:3030
```

</Terminal>

## The root page and the sidebar

The root page is where the site opens and the top of its sidebar. A page joins
the sidebar when a page already in it lists it in a `## Topics` section at its
end — the root's Topics are the first level, their Topics the next:

```markdown
## Topics

- [Getting started](getting-started.md) — install it and run a first page.
- [Guide](guide/README.md) — the tasks, one page each.
```

Links anywhere else in a page work as ordinary links and never change the
sidebar. How Topics shape the tree — groups, order, labels, repeated pages —
is on [Organizing navigation](../organizing-navigation/README.md).

## The navbar, the logo and the theme switch

- **`navbar`** puts links across the top: `Label: target`, a Lucide `(icon)`
  after a label, a nested map for a dropdown, `(icon)` alone for an icon-only
  link ([the navbar](../organizing-navigation/README.md#the-navbar)).
- **`sidebar: false`** hides the sidebar — for a site navigated by its navbar
  alone.
- **`logo`** shows an image before the title and makes it the tab's icon.
- **The theme switch** and the **comment-mode** button sit at the right end of
  the navbar, or in the top-right corner without one
  ([choosing a theme](../themes/README.md#choosing-a-theme),
  [Comments](../comments/README.md)).

## A real site to copy

These docs are a site: [`docs/devpage.yml`](../../../devpage.yml) with a navbar,
a logo, widgets and a sidebar built from Topics, starting at the
[landing page](../../../README.md). Every field is in the
[configuration reference](../configuration-reference/README.md#site).
