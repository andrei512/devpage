//! `devpage png`: a design, or one slide of a deck, drawn to a PNG with no
//! server running — asked of the same routes a server answers
//! `<design>.png` and `<deck>/<n>.png` with, so the picture is the one the
//! browser would fetch.

use std::sync::Arc;

use anyhow::{Result, bail};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::drive::http;
use devpage_core::document::{Document, Kind};

/// What drawing gave: the picture, or a design whose problems stop it —
/// `devpage check` names them.
pub enum Drawn {
    /// The PNG's bytes.
    Png(Vec<u8>),
    /// A design with problems, not drawn.
    Broken,
}

/// `document` drawn: a design in `theme` (light when none), or slide
/// `slide` (from 1) of a deck.
pub async fn draw(
    document: Arc<Document>,
    slide: Option<usize>,
    theme: Option<&str>,
) -> Result<Drawn> {
    let entry = document.entry();
    let path = match (&document.kind, slide) {
        (Kind::Design, None) => match theme {
            Some(theme) => format!("{entry}.png?theme={theme}"),
            None => format!("{entry}.png"),
        },
        (Kind::Design, Some(_)) => bail!("--slide is for a deck; a design is one picture"),
        (Kind::Slides { .. }, Some(n)) if theme.is_none() => format!("{entry}/{n}.png"),
        (Kind::Slides { .. }, Some(_)) => {
            bail!("--theme is for a design; a slide is drawn as the deck is")
        }
        (Kind::Slides { .. }, None) => bail!("which slide? --slide <n>, from 1"),
        _ => bail!("png draws a design or a slide of a deck; print a doc with `devpage pdf`"),
    };
    let app = http::App::printing(document);
    let shots = app.shots();
    let answered = http::router(app)
        .oneshot(Request::get(&path).body(Body::empty())?)
        .await;
    // The Chrome a slide was drawn with closes whatever the answer.
    shots.close().await;
    let response = answered?;
    let status = response.status();
    let timed_out = response.headers().contains_key("x-devpage-ready");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    match status {
        StatusCode::OK => {
            if timed_out {
                eprintln!("warning: the slide wasn't ready after 20 s; drawn as it was");
            }
            Ok(Drawn::Png(bytes.to_vec()))
        }
        StatusCode::UNPROCESSABLE_ENTITY => Ok(Drawn::Broken),
        _ => bail!("{}", String::from_utf8_lossy(&bytes).trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devpage_core::folder::ServedFolder;

    /// `files` in a folder of their own, and `entry` loaded from it.
    fn loaded(name: &str, files: &[(&str, &str)], entry: &str) -> Arc<Document> {
        let dir = std::env::temp_dir().join(format!("devpage-png-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let dir = dir.canonicalize().unwrap();
        Arc::new(Document::load(&dir.join(entry), &ServedFolder::new(&dir).unwrap()).unwrap())
    }

    #[tokio::test]
    async fn a_design_is_drawn_in_either_theme_without_a_server() {
        let design = "<Frame width={40} height={30} fill=\"#2563eb\" />";
        let document = loaded("design", &[("box.design", design)], "box.design");
        for theme in [None, Some("dark")] {
            let Drawn::Png(png) = draw(document.clone(), None, theme).await.unwrap() else {
                panic!("not drawn in {theme:?}")
            };
            assert!(png.starts_with(b"\x89PNG"), "{theme:?}");
        }
    }

    #[tokio::test]
    async fn a_broken_design_is_not_drawn() {
        let document = loaded(
            "broken",
            &[("bad.design", "<Frame width={40}")],
            "bad.design",
        );
        assert!(matches!(
            draw(document, None, None).await.unwrap(),
            Drawn::Broken
        ));
    }

    #[tokio::test]
    async fn what_png_cannot_draw_is_refused_with_what_to_do() {
        let files = [
            ("box.design", "<Frame width={40} height={30} />"),
            ("one.md", "# One\n"),
            ("talk.yml", "kind: slides\ntitle: Talk\nslides: [one.md]\n"),
        ];
        let error = |entry: &'static str, slide: Option<usize>, theme: Option<&'static str>| {
            let document = loaded(&format!("refused-{entry}"), &files, entry);
            async move {
                match draw(document, slide, theme).await {
                    Ok(_) => panic!("{entry} drawn"),
                    Err(error) => error.to_string(),
                }
            }
        };
        assert!(error("one.md", None, None).await.contains("devpage pdf"));
        assert!(error("talk.yml", None, None).await.contains("--slide"));
        assert!(
            error("talk.yml", Some(1), Some("dark"))
                .await
                .contains("--theme")
        );
        assert!(error("box.design", Some(1), None).await.contains("--slide"));
        assert!(
            error("talk.yml", Some(2), None)
                .await
                .contains("has 1 slide"),
        );
    }
}
