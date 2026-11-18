---
id: "04"
blocked-by: []
boxes: ["viewer/live-session/agents/world/extending", "viewer/extensions"]
---

# Load world modules and place widgets from ordinary packages

- A package can declare a world module, widget modules including their relative imports, and widget places. Multiple panel sections coexist, including the session’s own widgets.
- Local serving combines project and global extensions by id. A project replaces or disables a global package. Local-only packages stay out of deployed servers, publishing, checking and PDF generation.
- Previously supported document configurations continue to load. Existing prototype settings have a documented compatibility path.
