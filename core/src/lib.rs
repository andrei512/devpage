//! devpage's core: what a document is, how its pages render, what is wrong
//! with them, who comments on them and how its designs resolve — with no I/O
//! runtime. The crate depends on no tokio, axum, notify, tonic, reqwest or
//! chromiumoxide, so the compiler refuses a core that reaches for one. What
//! it needs from outside — the network for external links, somewhere to run
//! in the background, where changes go, where comments are kept, where
//! problems are reported — it asks for through a port, and the `devpage`
//! binary plugs in the adapters.

pub mod addresses;
pub mod answer;
pub mod cache;
pub mod changes;
pub mod check;
pub mod comments;
pub mod designs;
pub mod diagrams;
pub mod document;
pub mod external;
pub mod folder;
pub mod page;
pub mod problem;
pub mod search;
pub mod watch;
pub mod widgets;

// The page's own parts, for the core alone: highlighting, icons, reading
// Markdown, the navigation tree and HTML itself.
pub(crate) mod highlight;
pub(crate) mod html;
pub(crate) mod icons;
pub(crate) mod markup;
pub(crate) mod nav;
