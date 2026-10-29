//! A page, rendered in two phases. The content phase turns one file's
//! Markdown into [`Rendered`]: its HTML and every link it holds, by file and
//! line. The preview and `devpage check` judge that, and the frame phase puts
//! it inside its document's frame — a site's page, a doc, a deck — with the
//! problems found listed above it. [`ids`] answers which ids a link into
//! another page can land on, from that page's content phase, without
//! building its frame. Also the canvas
//! page, a design alone in the window, and the page of a document file that
//! can't be loaded.

mod canvas;
mod content;
mod frame;
pub(crate) mod markdown;
pub(crate) mod navbar;
mod sidebar;

/// The one HTML escape, for the binary's own HTML.
pub use crate::html::escape;
pub use canvas::canvas;
pub use content::{Link, Rendered, content, ids, read, shown_in};
pub use frame::{doc, invalid, page, slides};

#[cfg(test)]
mod tests;
