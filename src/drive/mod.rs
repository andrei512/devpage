//! The driving adapters: the HTTP server that answers the browser, and the
//! Chrome that prints PDFs, takes slide PNGs and records videos.

pub mod pdf;
pub mod server;
pub mod server_mode;
pub mod slide_png;
pub mod validators;
pub mod video;

#[cfg(test)]
mod server_mode_browser;
