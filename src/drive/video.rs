//! `devpage video`: a deck, narrated, as an mp4. GPT-Live says each slide's
//! notes word for word while Chrome for Testing shows the deck and records its
//! own tab, the voice played into the recording by the page itself
//! (`assets/video.js`).

pub mod live;
pub mod silence;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use axum::Router;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::chrome::{Open, Profile, Session, Tab};
use crate::document::{Document, Kind};
use crate::server;
use silence::{DONE_AFTER_SECONDS, PREBUFFER_SECONDS, RATE, SLIDE_LEAD_SECONDS, Trimmer};

/// The recorder and player, evaluated into the deck's page.
const RECORDER: &str = include_str!("../../assets/video.js");
/// The size of the recording, and of the page it captures.
const WIDTH: i64 = 1920;
const HEIGHT: i64 = 1080;
/// How often new voice is handed to the player while a slide plays.
const PUSH_EVERY: Duration = Duration::from_millis(100);
/// How long the narrator may take to start speaking a slide.
const VOICE_STARTS_WITHIN: Duration = Duration::from_secs(60);
/// The recording comes back in pieces this big, so a long talk doesn't have
/// to fit in one DevTools message.
const CHUNK: usize = 4 << 20;

/// The file `devpage video` writes when `-o` names none: the pdf rule, `.mp4`.
pub fn default_output(title: &str) -> PathBuf {
    crate::pdf::default_output(title).with_extension("mp4")
}

/// What the narrator is told, checked before anything slow: the session's
/// instructions and one request per slide. A document that isn't a deck, a
/// deck without `audience` or `goal`, or a slide without notes is an error.
pub fn told(document: &Document) -> Result<(String, Vec<String>)> {
    let Kind::Slides { slides, narration } = &document.kind else {
        let kind = match document.kind {
            Kind::Site { .. } => "a site",
            Kind::Doc { .. } => "a doc",
            Kind::Design => "a design",
            Kind::Slides { .. } => unreachable!(),
        };
        bail!("video records slides, not {kind}");
    };
    let blank = |field: &Option<String>| field.as_deref().is_none_or(|v| v.trim().is_empty());
    let unset: Vec<&str> = [("audience", &narration.audience), ("goal", &narration.goal)]
        .into_iter()
        .filter(|(_, value)| blank(value))
        .map(|(name, _)| name)
        .collect();
    if !unset.is_empty() {
        bail!(
            "the narrator needs to know who the talk is for and what it should achieve; set {} in {}",
            unset.join(" and "),
            document.file_name
        );
    }
    let missing: Vec<String> = slides
        .iter()
        .enumerate()
        .filter(|(_, slide)| slide.notes.as_deref().is_none_or(|n| n.trim().is_empty()))
        .map(|(i, slide)| {
            let file = document.served.relative(&slide.file).unwrap_or(&slide.file);
            format!("{} ({})", i + 1, file.display())
        })
        .collect();
    if !missing.is_empty() {
        bail!(
            "every slide needs <Notes> to be narrated; these have none: {}",
            missing.join(", ")
        );
    }
    let count = slides.len();
    let instructions = live::instructions(&document.title, count, narration);
    let requests = slides
        .iter()
        .enumerate()
        .map(|(i, slide)| {
            live::request(
                i + 1,
                count,
                slide.delivery.as_deref(),
                slide.notes.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    Ok((instructions, requests))
}

/// Records `document`, narrated by GPT-Live per `config`, to `output`.
pub async fn record(document: Arc<Document>, output: &Path, config: &live::Config) -> Result<()> {
    let (instructions, requests) = told(&document)?;
    let heard = Arc::new(Mutex::new(Heard::default()));
    let (router, deck) = served(document);
    film(
        router,
        &deck,
        output,
        requests,
        heard.clone(),
        async |heard| live::Live::start(config, &instructions, heard).await,
    )
    .await
}

/// The voice that says each slide's notes, its audio arriving in [`Heard`].
pub(crate) trait Voice {
    /// Asks for slide `n` with its [`live::request`].
    fn say(&self, n: usize, request: &str);
    async fn close(self);
}

impl Voice for live::Live {
    fn say(&self, n: usize, request: &str) {
        live::Live::say(self, n, request);
    }
    async fn close(self) {
        live::Live::close(self).await;
    }
}

/// The voice as it has arrived: trimmed of long silences, the moment speech
/// was last heard, and why the voice failed, if it did.
#[derive(Default)]
pub struct Heard {
    trimmer: Trimmer,
    last_loud: Option<Instant>,
    failed: Option<String>,
}

impl Heard {
    pub fn take(&mut self, pcm: &[i16]) {
        if self.trimmer.take(pcm) {
            self.last_loud = Some(Instant::now());
        }
    }

    pub fn fail(&mut self, why: String) {
        self.failed.get_or_insert(why);
    }

    fn check(&self) -> Result<()> {
        match &self.failed {
            Some(why) => bail!("{why}"),
            None => Ok(()),
        }
    }
}

/// The router `document` is recorded from, as `devpage pdf` serves it, and
/// its entry.
pub(super) fn served(document: Arc<Document>) -> (Router, String) {
    let entry = document.entry();
    let router = server::router(server::App::printing(document));
    (router, entry)
}

/// Serves the deck from `router`, opens it at `deck` in a recording
/// session, starts the voice once the recorder is armed, and records every
/// slide. Chrome is killed however this ends.
pub(super) async fn film<V: Voice>(
    router: Router,
    deck: &str,
    output: &Path,
    notes: Vec<String>,
    heard: Arc<Mutex<Heard>>,
    voice: impl AsyncFnOnce(Arc<Mutex<Heard>>) -> Result<V>,
) -> Result<()> {
    let session = Session::start(Profile::Recording, router).await?;
    let result = async {
        let tab = session.open(deck, Open::default()).await?;
        let bytes = narrate(&tab, &notes, &heard, voice).await?;
        std::fs::write(output, bytes).with_context(|| format!("writing {}", output.display()))
    }
    .await;
    session.close().await;
    result
}

async fn narrate<V: Voice>(
    tab: &Tab,
    notes: &[String],
    heard: &Arc<Mutex<Heard>>,
    voice: impl AsyncFnOnce(Arc<Mutex<Heard>>) -> Result<V>,
) -> Result<Vec<u8>> {
    tab.eval::<()>(&format!("{RECORDER}\nundefined")).await?;
    tab.eval::<()>(&format!("devpageVideo.start({RATE})"))
        .await?;
    fit(tab).await?;
    tab.eval::<u32>("devpageVideo.arm()").await?;

    let voice = voice(heard.clone()).await?;
    let total = notes.len();
    for (i, slide_notes) in notes.iter().enumerate() {
        let n = i + 1;
        let asked = Instant::now();
        heard.lock().unwrap().trimmer.start_slide();
        voice.say(n, slide_notes);
        // Hold the start until the voice is speaking and enough is buffered.
        loop {
            let ready = {
                let heard = heard.lock().unwrap();
                heard.check()?;
                heard.last_loud.is_some_and(|at| at > asked)
                    && heard.trimmer.buffered() as f64 >= PREBUFFER_SECONDS * RATE as f64
            };
            if ready {
                break;
            }
            if asked.elapsed() > VOICE_STARTS_WITHIN {
                bail!(
                    "the narrator said nothing for slide {n} within {} seconds",
                    VOICE_STARTS_WITHIN.as_secs()
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Resume in the same evaluation the audio lands in, so neither leads.
        flush(tab, heard, true).await?;
        eprintln!("slide {n}/{total}");
        loop {
            let quiet = {
                let heard = heard.lock().unwrap();
                heard.check()?;
                heard.last_loud.map_or(Duration::ZERO, |at| at.elapsed())
            };
            if quiet.as_secs_f64() >= DONE_AFTER_SECONDS {
                break;
            }
            tokio::time::sleep(PUSH_EVERY).await;
            flush(tab, heard, false).await?;
        }
        heard.lock().unwrap().trimmer.end_slide();
        flush(tab, heard, false).await?;
        if let Some(stuck) = tab
            .eval::<Option<serde_json::Value>>("devpageVideo.drain()")
            .await?
        {
            eprintln!("slide {n}: the player did not empty: {stuck}");
        }
        if n < total {
            // Change the slide while recording, and pause only once the new
            // slide has been on screen for its lead: the recording resumes on
            // whatever it last saw, so that must be the new slide.
            tab.press("ArrowRight", 39).await?;
            tokio::time::sleep(Duration::from_secs_f64(SLIDE_LEAD_SECONDS)).await;
            let shown: String = tab.eval("devpageVideo.shown()").await?;
            if shown != (n + 1).to_string() {
                bail!(
                    "the deck shows slide {shown} after slide {n}, not slide {}",
                    n + 1
                );
            }
            tab.eval::<()>("devpageVideo.pause()").await?;
        }
    }
    voice.close().await;

    let size: usize = tab.eval("devpageVideo.stop()").await?;
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let piece: String = tab
            .eval(&format!("devpageVideo.chunk({}, {CHUNK})", bytes.len()))
            .await?;
        let piece = STANDARD.decode(piece).context("reading the recording")?;
        if piece.is_empty() {
            bail!("the recording ended early");
        }
        bytes.extend(piece);
    }
    Ok(bytes)
}

/// Capturing adds a "sharing this tab" bar a moment later, which shrinks the
/// page: keep correcting the window until the page reads 1920 × 1080 three
/// times in a row.
async fn fit(tab: &Tab) -> Result<()> {
    let mut settled = 0;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let [width, height, ..]: [i64; 4] = tab.eval("devpageVideo.size()").await?;
        if (width, height) == (WIDTH, HEIGHT) {
            settled += 1;
            if settled == 3 {
                return Ok(());
            }
            continue;
        }
        settled = 0;
        tab.grow_window(WIDTH - width, HEIGHT - height).await?;
    }
    let size: [i64; 4] = tab.eval("devpageVideo.size()").await?;
    bail!(
        "the page never held still at {WIDTH} × {HEIGHT}: it is {} × {}",
        size[0],
        size[1]
    )
}

/// Hands whatever voice is ready to the player; `play` resumes the recorder
/// in the same evaluation.
async fn flush(tab: &Tab, heard: &Arc<Mutex<Heard>>, play: bool) -> Result<()> {
    let pcm = heard.lock().unwrap().trimmer.drain();
    if pcm.is_empty() {
        return Ok(());
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    let resume = if play { "devpageVideo.play();" } else { "" };
    tab.eval::<()>(&format!(
        "{resume}devpageVideo.push(\"{}\")",
        STANDARD.encode(bytes)
    ))
    .await
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::folder::ServedFolder;

    /// A folder holding `talk.yml`, a deck of `slides`, each `(file, text)`.
    pub(in crate::drive) fn deck(name: &str, slides: &[(&str, &str)]) -> (PathBuf, Arc<Document>) {
        let dir = std::env::temp_dir().join(format!("devpage-video-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        for (file, text) in slides {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let files: Vec<&str> = slides.iter().map(|(file, _)| *file).collect();
        std::fs::write(
            dir.join("talk.yml"),
            format!(
                "kind: slides\ntitle: Talk\naudience: Testers.\ngoal: They hear it.\nslides: [{}]\n",
                files.join(", ")
            ),
        )
        .unwrap();
        let document = Arc::new(
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap(),
        );
        (dir, document)
    }

    #[test]
    fn the_default_file_name_is_the_title_made_safe_as_mp4() {
        assert_eq!(default_output("Launch"), Path::new("Launch.mp4"));
        assert_eq!(default_output("A/B: v1.2"), Path::new("A-B- v1.2.mp4"));
    }

    #[test]
    fn every_slide_needs_notes_and_the_error_names_the_ones_without() {
        let (dir, talk) = deck(
            "notes",
            &[
                ("one.md", "# One\n\n<Notes>Hello.</Notes>\n"),
                ("two.md", "# Two\n"),
                ("three.md", "# Three\n\n<Notes>\n\n</Notes>\n"),
            ],
        );
        let error = told(&talk).unwrap_err().to_string();
        assert_eq!(
            error,
            "every slide needs <Notes> to be narrated; these have none: 2 (two.md), 3 (three.md)"
        );
        std::fs::write(dir.join("two.md"), "# Two\n\n<Notes>Two.</Notes>\n").unwrap();
        std::fs::write(dir.join("three.md"), "# Three\n\n<Notes>Three.</Notes>\n").unwrap();
        let talk =
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
        let (instructions, requests) = told(&talk).unwrap();
        assert!(instructions.contains("Audience: Testers.\nGoal: They hear it.\n"));
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("Slide 1 of 3. Delivery: in the talk's tone.\n"));
        assert!(requests[2].ends_with("\n\nThree."));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_deck_without_audience_or_goal_is_refused_naming_them() {
        let (dir, _) = deck(
            "narration",
            &[("one.md", "# One\n\n<Notes>Hello.</Notes>\n")],
        );
        std::fs::write(
            dir.join("talk.yml"),
            "kind: slides\ntitle: Talk\nslides: [one.md]\n",
        )
        .unwrap();
        let talk =
            Document::load(&dir.join("talk.yml"), &ServedFolder::new(&dir).unwrap()).unwrap();
        assert_eq!(
            told(&talk).unwrap_err().to_string(),
            "the narrator needs to know who the talk is for and what it should achieve; set audience and goal in talk.yml"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn anything_but_a_deck_is_refused() {
        let dir = std::env::temp_dir().join(format!("devpage-video-doc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
        for (yml, kind) in [
            ("kind: site\ntitle: H\nroot: README.md\n", "a site"),
            ("kind: doc\ntitle: H\nchapters: [README.md]\n", "a doc"),
        ] {
            std::fs::write(dir.join("devpage.yml"), yml).unwrap();
            let document =
                Arc::new(Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap());
            let config = live::Config {
                provider: live::Provider::OpenAi {
                    key: "never".into(),
                },
                model: live::DEFAULT_MODEL.into(),
            };
            let out = dir.join("never.mp4");
            let error = record(document, &out, &config).await.unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("video records slides, not {kind}")
            );
            assert!(!out.exists());
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
