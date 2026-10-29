//! Markdown to a full page: the rendered file inside the document's frame —
//! and a design's canvas page.

mod canvas;
mod frame;
mod markdown;
mod sidebar;

pub use crate::html::escape;
pub use crate::markup::title;
pub use canvas::canvas;
pub use frame::{doc, html_open, invalid, page, slides, tab_title};
pub(crate) use markdown::options;
pub use markdown::{FRAME_IDS, front_matter, markdown, markdown_in};

#[cfg(test)]
mod tests;
