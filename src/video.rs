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
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chromiumoxide::Page;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::browser::{
    Bounds, GetWindowBoundsParams, GetWindowForTargetParams, SetWindowBoundsParams,
};
use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;

use crate::document::{Document, Kind};
use crate::pdf::{Profile, READY};
use crate::server;
use silence::{DONE_AFTER_SECONDS, PREBUFFER_SECONDS, RATE, SLIDE_LEAD_SECONDS, Trimmer};

/// The recorder and player, evaluated into the deck's page.
const RECORDER: &str = include_str!("../assets/video.js");
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
            let file = slide
                .file
                .strip_prefix(&document.served)
                .unwrap_or(&slide.file);
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
    film(document, output, requests, heard.clone(), async |heard| {
        live::Live::start(config, &instructions, heard).await
    })
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

/// Serves the deck, opens it in Chrome for Testing, starts the voice once the
/// recorder is armed, and records every slide. Chrome is killed however this
/// ends.
async fn film<V: Voice>(
    document: Arc<Document>,
    output: &Path,
    notes: Vec<String>,
    heard: Arc<Mutex<Heard>>,
    voice: impl AsyncFnOnce(Arc<Mutex<Heard>>) -> Result<V>,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}{}", listener.local_addr()?, document.entry());
    let (changes, _) = broadcast::channel(1);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server::router(server::Options::printing(document, changes)),
        )
        .await
        .ok();
    });
    let profile = Profile::new();
    let config = profile
        .chrome_for_testing()
        .await?
        .viewport(None)
        .disable_default_args()
        // Headless as the prototype ran it: chromiumoxide's own headless flags
        // add `--mute-audio`.
        .with_head()
        .arg(("headless", "new"))
        .arg("hide-scrollbars")
        .arg("auto-accept-this-tab-capture")
        .arg(("autoplay-policy", "no-user-gesture-required"))
        .window_size(1920, 1167)
        .build()
        .map_err(anyhow::Error::msg)?;
    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .context("starting Chrome for Testing")?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
    let result = async {
        let page = browser.new_page("about:blank").await?;
        let bytes = narrate(&page, &url, &notes, &heard, voice).await?;
        std::fs::write(output, bytes).with_context(|| format!("writing {}", output.display()))
    }
    .await;
    browser.kill().await;
    browser.wait().await.ok();
    events.abort();
    server.abort();
    result
}

async fn narrate<V: Voice>(
    page: &Page,
    url: &str,
    notes: &[String],
    heard: &Arc<Mutex<Heard>>,
    voice: impl AsyncFnOnce(Arc<Mutex<Heard>>) -> Result<V>,
) -> Result<Vec<u8>> {
    page.goto(url).await?;
    for _ in 0..400 {
        if page.evaluate(READY).await?.into_value::<bool>()? {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run::<()>(page, &format!("{RECORDER}\nundefined")).await?;
    run::<()>(page, &format!("devpageVideo.start({RATE})")).await?;
    fit(page).await?;
    run::<u32>(page, "devpageVideo.arm()").await?;

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
        flush(page, heard, true).await?;
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
            flush(page, heard, false).await?;
        }
        heard.lock().unwrap().trimmer.end_slide();
        flush(page, heard, false).await?;
        if let Some(stuck) = run::<Option<serde_json::Value>>(page, "devpageVideo.drain()").await? {
            eprintln!("slide {n}: the player did not empty: {stuck}");
        }
        if n < total {
            // Change the slide while recording, and pause only once the new
            // slide has been on screen for its lead: the recording resumes on
            // whatever it last saw, so that must be the new slide.
            next(page).await?;
            tokio::time::sleep(Duration::from_secs_f64(SLIDE_LEAD_SECONDS)).await;
            let shown: String = run(page, "devpageVideo.shown()").await?;
            if shown != (n + 1).to_string() {
                bail!(
                    "the deck shows slide {shown} after slide {n}, not slide {}",
                    n + 1
                );
            }
            run::<()>(page, "devpageVideo.pause()").await?;
        }
    }
    voice.close().await;

    let size: usize = run(page, "devpageVideo.stop()").await?;
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let piece: String = run(
            page,
            &format!("devpageVideo.chunk({}, {CHUNK})", bytes.len()),
        )
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
async fn fit(page: &Page) -> Result<()> {
    let window = page
        .execute(GetWindowForTargetParams {
            target_id: Some(page.target_id().clone()),
        })
        .await?
        .result
        .window_id;
    let mut settled = 0;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let [width, height, ..]: [i64; 4] = run(page, "devpageVideo.size()").await?;
        if (width, height) == (WIDTH, HEIGHT) {
            settled += 1;
            if settled == 3 {
                return Ok(());
            }
            continue;
        }
        settled = 0;
        let bounds = page
            .execute(GetWindowBoundsParams::new(window))
            .await?
            .result
            .bounds;
        let grow = |now: Option<i64>, inner: i64, want: i64| now.map(|n| n + want - inner);
        page.execute(SetWindowBoundsParams::new(
            window,
            Bounds {
                width: grow(bounds.width, width, WIDTH),
                height: grow(bounds.height, height, HEIGHT),
                ..Default::default()
            },
        ))
        .await?;
    }
    let size: [i64; 4] = run(page, "devpageVideo.size()").await?;
    bail!(
        "the page never held still at {WIDTH} × {HEIGHT}: it is {} × {}",
        size[0],
        size[1]
    )
}

/// Hands whatever voice is ready to the player; `play` resumes the recorder
/// in the same evaluation.
async fn flush(page: &Page, heard: &Arc<Mutex<Heard>>, play: bool) -> Result<()> {
    let pcm = heard.lock().unwrap().trimmer.drain();
    if pcm.is_empty() {
        return Ok(());
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    let resume = if play { "devpageVideo.play();" } else { "" };
    run::<()>(
        page,
        &format!("{resume}devpageVideo.push(\"{}\")", STANDARD.encode(bytes)),
    )
    .await
}

/// Presses → on the deck.
async fn next(page: &Page) -> Result<()> {
    for kind in [DispatchKeyEventType::KeyDown, DispatchKeyEventType::KeyUp] {
        page.execute(
            DispatchKeyEventParams::builder()
                .r#type(kind)
                .key("ArrowRight")
                .code("ArrowRight")
                .windows_virtual_key_code(39)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await?;
    }
    Ok(())
}

/// Evaluates `expression` in the page as a user gesture, awaiting a promise,
/// and reads its value.
async fn run<T: DeserializeOwned>(page: &Page, expression: &str) -> Result<T> {
    let params = EvaluateParams::builder()
        .expression(expression)
        .user_gesture(true)
        .await_promise(true)
        .return_by_value(true)
        .build()
        .map_err(anyhow::Error::msg)?;
    let result = page.evaluate_expression(params).await?;
    match result.value() {
        Some(_) => Ok(result.into_value()?),
        None => Ok(serde_json::from_value(serde_json::Value::Null)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder holding `talk.yml`, a deck of `slides`, each `(file, text)`.
    fn deck(name: &str, slides: &[(&str, &str)]) -> (PathBuf, Arc<Document>) {
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
        let document = Arc::new(Document::load(&dir.join("talk.yml"), &dir).unwrap());
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
        let talk = Document::load(&dir.join("talk.yml"), &dir).unwrap();
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
        let talk = Document::load(&dir.join("talk.yml"), &dir).unwrap();
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
            let document = Arc::new(Document::load(&dir, &dir).unwrap());
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

    /// A stand-in for GPT-Live: streams 20 ms of audio every 20 ms, as
    /// GPT-Live does, silence included, and answers each slide's notes with a tone.
    struct Tone {
        left: Arc<AtomicUsize>,
        seconds: f64,
        task: tokio::task::JoinHandle<()>,
    }

    impl Tone {
        fn start(heard: Arc<Mutex<Heard>>, seconds: f64) -> Self {
            let left = Arc::new(AtomicUsize::new(0));
            let tone = left.clone();
            let task = tokio::spawn(async move {
                let mut every = tokio::time::interval(Duration::from_millis(20));
                let mut at = 0usize;
                loop {
                    every.tick().await;
                    let pcm: Vec<i16> = (0..RATE / 50)
                        .map(|_| {
                            at += 1;
                            let on = tone
                                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                                    n.checked_sub(1)
                                })
                                .is_ok();
                            let phase = at as f64 * 440.0 / RATE as f64 * std::f64::consts::TAU;
                            if on { (phase.sin() * 8000.0) as i16 } else { 0 }
                        })
                        .collect();
                    heard.lock().unwrap().take(&pcm);
                }
            });
            Self {
                left,
                seconds,
                task,
            }
        }
    }

    impl Voice for Tone {
        fn say(&self, _: usize, _: &str) {
            self.left
                .store((self.seconds * RATE as f64) as usize, Ordering::Relaxed);
        }
        async fn close(self) {
            self.task.abort();
        }
    }

    /// The boxes inside `data`, as (type, payload).
    fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let mut found = Vec::new();
        let mut at = 0;
        while at + 8 <= data.len() {
            let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = data[at + 4..at + 8].try_into().unwrap();
            let (start, end) = match size {
                0 => (at + 8, data.len()),
                1 => {
                    let large = u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap());
                    (at + 16, at + large as usize)
                }
                n => (at + 8, at + n),
            };
            found.push((kind, &data[start..end.min(data.len())]));
            at = end;
        }
        found
    }

    fn inside<'a>(data: &'a [u8], kind: &[u8; 4]) -> Vec<&'a [u8]> {
        boxes(data)
            .into_iter()
            .filter(|(k, _)| k == kind)
            .map(|(_, payload)| payload)
            .collect()
    }

    /// Each track of an mp4: its handler (`vide`, `soun`) and its `tkhd`
    /// width and height.
    fn tracks(mp4: &[u8]) -> Vec<(String, u32, u32)> {
        let moov = inside(mp4, b"moov");
        moov.iter()
            .flat_map(|moov| inside(moov, b"trak"))
            .map(|trak| {
                let tkhd = inside(trak, b"tkhd")[0];
                let fixed =
                    |at: usize| u32::from_be_bytes(tkhd[at..at + 4].try_into().unwrap()) >> 16;
                let hdlr = inside(trak, b"mdia")
                    .into_iter()
                    .flat_map(|mdia| inside(mdia, b"hdlr"))
                    .next()
                    .unwrap();
                (
                    String::from_utf8_lossy(&hdlr[8..12]).into_owned(),
                    fixed(tkhd.len() - 8),
                    fixed(tkhd.len() - 4),
                )
            })
            .collect()
    }

    /// Drives Chrome for Testing, no GPT-Live; run with
    /// `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_deck_is_recorded_to_a_1920_by_1080_mp4_with_its_voice() {
        let (dir, talk) = deck(
            "film",
            &[
                ("one.md", "# One\n\n<Notes>First.</Notes>\n"),
                ("two.md", "# Two\n\n<Notes>Second.</Notes>\n"),
            ],
        );
        let (_, notes) = told(&talk).unwrap();
        let out = dir.join("Talk.mp4");
        let heard = Arc::new(Mutex::new(Heard::default()));
        // `film` fails unless the deck shows slide 2 after slide 1.
        film(talk, &out, notes, heard, async |heard| {
            Ok(Tone::start(heard, 2.5))
        })
        .await
        .unwrap();
        let mp4 = std::fs::read(&out).unwrap();
        assert_eq!(&mp4[4..8], b"ftyp");
        let mut tracks = tracks(&mp4);
        tracks.sort();
        assert_eq!(
            tracks,
            [("soun".to_string(), 0, 0), ("vide".to_string(), 1920, 1080)]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Calls GPT-Live, which bills per minute; run with
    /// `cargo test -- --ignored` and `DEVPAGE_OPENAI_API_KEY` set.
    #[tokio::test]
    #[ignore]
    async fn a_deck_is_narrated_by_gpt_live() {
        if std::env::var_os(live::OPENAI_API_KEY).is_none() {
            eprintln!("skipped: {} is not set", live::OPENAI_API_KEY);
            return;
        }
        let config = live::Config::from_env().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let talk =
            Arc::new(Document::load(&root.join("tests/fixtures/narrated-talk"), root).unwrap());
        let dir = std::env::temp_dir().join(format!("devpage-video-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("Narrated talk.mp4");
        record(talk, &out, &config).await.unwrap();
        let mut tracks = tracks(&std::fs::read(&out).unwrap());
        tracks.sort();
        assert_eq!(
            tracks,
            [("soun".to_string(), 0, 0), ("vide".to_string(), 1920, 1080)]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
