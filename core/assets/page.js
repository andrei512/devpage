// The page module: what a widget imports as `devpage`. The page script made
// it; this hands it on, so a widget reads and steers the page it is on.
import { useSyncExternalStore } from "react";

const { page, base } = window.devpagePage;

// The live session's world, as this page knows it: each part's state as the
// session last sent it. The session's own module, which holds the page's
// connection, puts each part here and gives `send`; where no session is on,
// every part is undefined and a call does nothing.
const world = (window.devpageWorld ??= {
  parts: {},
  watchers: new Set(),
  send: null,
  /** A part's state arrived. */
  put(part, state) {
    this.parts[part] = state;
    for (const watcher of this.watchers) watcher();
  },
  /** Calls a function of the world, such as `counter.add`. */
  call(name, input = {}) {
    this.send?.({ type: "call", name, input });
  },
});

const watch = (watcher) => {
  world.watchers.add(watcher);
  return () => world.watchers.delete(watcher);
};

/** A part's state, in a widget: the widget is drawn again when it changes. */
function useWorld(part) {
  return useSyncExternalStore(watch, () => world.parts[part]);
}

export { page, base, world, useWorld };
