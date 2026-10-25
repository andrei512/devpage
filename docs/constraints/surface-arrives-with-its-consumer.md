# Surface area arrives with its consumer

A Markdown extension, a layout or layout option, a CLI command or flag, a
config field, an extension API hook or a CSS hook is added when something needs
it — never speculatively, and never because the implementation makes it easy.

**Why:** every one of them is a contract with the people who write documents
and configure devpage, and their files keep depending on it long after the
change that added it. A contract nobody asked for is one you maintain forever
for free. What devpage supports grows from what documents actually need, so
that what it becomes is decided by its users rather than by what was scaffolded
on day one.

**Check:** a diff adding surface names a consumer that exists — a document,
test or feature in the same diff, or something already in the repo. Surface
whose only justification is the ticket that adds it doesn't ship.
