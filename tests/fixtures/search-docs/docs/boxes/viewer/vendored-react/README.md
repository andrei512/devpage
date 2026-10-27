# The vendored React bundle

What React is in devpage — React and react-dom bundled into one file that
widgets run with in the browser — where it came from, its version and licence,
how devpage serves it, and how to upgrade it. It is a mechanism page under the
[viewer](../README.md), for someone about to replace the bundle; how to write a
widget is in [Widgets](../widgets/README.md).

## What is in the repository

| File | What it is |
| --- | --- |
| `assets/vendor/react.js` | React and react-dom **19.3.0**, production builds, bundled into one minified ES module (about 222 KB). |
| `assets/vendor/react.LICENSE` | Their licence: MIT, copyright Meta Platforms, Inc. and affiliates. |

The bundle is built from the npm packages `react@19.3.0` and
`react-dom@19.3.0` (published 2026-09-09) with this entry module:

```js
import * as React from "react";
export * from "react";
export default React;
export { jsx, jsxs, Fragment } from "react/jsx-runtime";
export { createRoot, hydrateRoot } from "react-dom/client";
export { flushSync } from "react-dom";
```

`hydrateRoot` and `flushSync` are there for a site's sidebar, which React
takes over from the server's markup and updates within a page swap's commit
([The page script](../page-script/README.md#the-sidebar)).

## How devpage uses it

`build.rs` builds the file into the binary, and devpage serves it at its
content address, `/__devpage/react.<hash>.js` — see
[built-in assets](../serving-pages/README.md#built-in-assets). Every page's
import map points `react`, `react/jsx-runtime` and `react-dom/client` at it,
so compiled widgets — whose JSX becomes `react/jsx-runtime` calls — and the page
script's mounting code share one copy of React
([Widgets](../widgets/how-widgets-run/README.md)). Only a page with a widget —
a site's sidebar is one — loads it, preloaded from that page's head
([the load set](../rendering-a-page/README.md#the-load-set)).

## Upgrading it

<Steps>

1. Build the new bundle
   In a scratch folder outside the repository, with [Bun](https://bun.sh),
   using `entry.js` as above. Building 19.3.0 this way with Bun 1.4.2
   reproduces the vendored file byte for byte.

   $ npm install react@<version> react-dom@<version>
   $ bun build entry.js --minify --target browser --format esm --define 'process.env.NODE_ENV="production"' --outfile react.js

2. Replace the files
   `react.js`, and `react.LICENSE` from the package's `LICENSE`.

3. Rebuild devpage and run the widget tests
   Including the ones that drive Chrome.

   $ cargo test
   $ cargo test -- --ignored

4. Update this page's version and size

</Steps>
