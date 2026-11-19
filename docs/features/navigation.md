# Navigation and themes

A site gets a sidebar from its pages' Topics sections and, optionally, a
navbar of links, dropdowns and icons; every page has a light/dark switch, and a
`logo` puts an image before the title and in the browser tab. Links between
pages open in place, and back and forward return to where you were on each
page. The sidebar opens on the page shown, however far down the tree it is,
and its right edge is dragged to set its width, which the browser remembers. An address with nothing at it shows a page in the site's frame offering
the page it most likely meant — or the site's own `404.md`. On a phone one
menu button at the top left slides in the navbar's links, then the sidebar,
or a doc's outline.

```yaml
logo: logo.svg
navbar:
  Docs (book-open): README.md
  (github): https://github.com/andrei512/devpage
```

Teaches it: [Making a site](../boxes/viewer/write/making-a-site/README.md),
[A page for missing addresses](../boxes/viewer/write/making-a-site/README.md#a-page-for-missing-addresses),
[Organizing navigation](../boxes/viewer/write/organizing-navigation/README.md),
[Choosing a theme](../boxes/viewer/on-the-page/themes/README.md#choosing-a-theme).
