# Extensions use the host watch

An extension declares its document inputs before the host filters filesystem events. It receives captured change batches from devpage and does not run another watcher or polling loop. Preparation of a changed instance finishes before the host announces its refresh.

Otherwise an input can be filtered out before its extension hears it, or a page can refresh against old derived state. Input declarations cover additions and removals, not just files present at startup.

Check a declared input excluded by the ordinary site's watch patterns, a newly created nested folder, and a removed source. Each must reach preparation through the host pipeline, followed by at most one existing refresh notification for the batch. No extension-specific watcher is started.
