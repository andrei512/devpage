//! The driving adapters: the HTTP server that answers the browser, and the
//! Chrome that prints PDFs, takes slide PNGs and records videos, and the
//! command line that talks to a devpage server, over the API JSON both share.

pub mod api_types;
pub mod client;
pub mod pdf;
pub mod server;
pub mod server_mode;
pub mod slide_png;
pub mod validators;
pub mod video;

#[cfg(test)]
mod server_mode_browser;
#[cfg(test)]
mod server_mode_comments_browser;
#[cfg(test)]
mod server_mode_pages_browser;
