//! `devpage pdf`: Chrome prints the page devpage serves, in print mode, so the
//! PDF is the page the browser shows.

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::chrome::{Open, PrintToPdfParams, Profile, Session};
use crate::document::{Document, Kind, Paper, Pdf};
use crate::server;

/// `document` printed, as a PDF's bytes.
pub async fn print(document: Arc<Document>) -> Result<Vec<u8>> {
    let params = match &document.kind {
        Kind::Doc { pdf, .. } => doc_params(pdf),
        Kind::Slides { .. } => PrintToPdfParams {
            print_background: Some(true),
            prefer_css_page_size: Some(true),
            paper_width: Some(20.0),
            paper_height: Some(11.25),
            margin_top: Some(0.0),
            margin_bottom: Some(0.0),
            margin_left: Some(0.0),
            margin_right: Some(0.0),
            ..Default::default()
        },
        Kind::Site { .. } => bail!("pdf prints a doc or slides, not a site"),
        Kind::Design => bail!("pdf prints a doc or slides, not a design"),
    };
    let path = format!("{}?print", document.entry());
    let router = server::router(server::App::printing(document));
    let session = Session::start(Profile::Shot, router).await?;
    // The page is laid out as paper from the start, not only when Chrome
    // prints it: every slide of a deck is shown in print, so an embedded
    // design on any slide is fitted and painted where it will print.
    let printed = async {
        let tab = session
            .open(
                &path,
                Open {
                    shot: true,
                    paper: true,
                    ..Open::default()
                },
            )
            .await?;
        let bytes = tab.print(params).await;
        tab.close().await;
        bytes
    }
    .await;
    session.close().await;
    let bytes = printed?;
    if bytes.is_empty() {
        bail!("Chrome printed nothing");
    }
    Ok(bytes)
}

/// The file `devpage pdf` writes when `-o` names none: the document's title,
/// with the characters a file name can't hold replaced by `-`.
pub fn default_output(title: &str) -> std::path::PathBuf {
    let name: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    let name = if name.is_empty() { "document" } else { name };
    std::path::PathBuf::from(format!("{name}.pdf"))
}

/// The print call for a doc: its paper, and its header and footer, whose
/// `{page}` and `{pages}` become Chrome's page-number fields.
pub(super) fn doc_params(pdf: &Pdf) -> PrintToPdfParams {
    let template = |text: &str| {
        let text = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace("{page}", "<span class=\"pageNumber\"></span>")
            .replace("{pages}", "<span class=\"totalPages\"></span>");
        format!(
            "<div style=\"font: 9px -apple-system, Helvetica, sans-serif; color: #6e6e73; width: 100%; text-align: center;\">{text}</div>"
        )
    };
    let (width, height) = match pdf.paper {
        Paper::A4 => (8.27, 11.69),
        Paper::Letter => (8.5, 11.0),
    };
    PrintToPdfParams {
        display_header_footer: Some(true),
        header_template: Some(template(pdf.header.as_deref().unwrap_or(""))),
        footer_template: Some(template(&pdf.footer)),
        print_background: Some(true),
        prefer_css_page_size: Some(true),
        paper_width: Some(width),
        paper_height: Some(height),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::folder::ServedFolder;

    #[test]
    fn the_default_file_name_is_the_title_made_safe() {
        assert_eq!(
            default_output("Offline sync"),
            Path::new("Offline sync.pdf")
        );
        assert_eq!(default_output("A/B: why?"), Path::new("A-B- why-.pdf"));
        assert_eq!(default_output("..."), Path::new("document.pdf"));
    }

    #[tokio::test]
    async fn a_site_is_refused() {
        let dir = std::env::temp_dir().join(format!("devpage-pdf-site-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: Home\nroot: README.md\n",
        )
        .unwrap();
        let site = Arc::new(Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap());
        let error = print(site).await.unwrap_err();
        assert_eq!(error.to_string(), "pdf prints a doc or slides, not a site");
    }
}
