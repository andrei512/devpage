# A render uses one extension revision

A page contribution, embed, file view, its reference resolution, and its data requests identify the extension revision they use. A response cannot silently combine derived facts from one capture with source or targets from another. Failed capture or preparation is visible and cannot relabel old state as current.

This prevents an engineering page from showing requirements from one revision beside a diagram or assessment context from another. Publication is atomic at the prepared-state boundary; it does not claim that an editor's separate filesystem writes are transactional.

Change a source between preparation and rendering, request data for an unavailable revision, and fail a refresh. Require one coherent response or an explicit changed/stale/error result. Check and PDF must not report success using silently substituted old content.
