//! `devpage video` in Chrome for Testing: a deck recorded with a stand-in
//! voice, a slide showing a slide's PNG while it is recorded, and a deck
//! narrated by GPT-Live.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::adapters::chrome::testing;
use crate::drive::video::silence::RATE;
use crate::drive::video::tests::deck;
use crate::drive::video::{Heard, Voice, film, narrator, record, served, told};
use devpage_core::document::Document;
use devpage_core::folder::ServedFolder;

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
            let fixed = |at: usize| u32::from_be_bytes(tkhd[at..at + 4].try_into().unwrap()) >> 16;
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
    let (router, entry) = served(talk);
    // `film` fails unless the deck shows slide 2 after slide 1.
    film(router, &entry, &out, notes, heard, async |heard| {
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

/// A slide showing a slide's PNG while it is recorded: the recording
/// tab isn't one devpage shoots, so the PNG is drawn for it, never
/// refused as if asked for inside a shot. Drives Chrome for Testing and
/// chrome-headless-shell, no GPT-Live; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn a_recorded_slide_showing_a_slides_png_gets_it_drawn() {
    let (dir, talk) = deck(
        "embeds",
        &[
            (
                "one.md",
                "# One\n\n![Slide two](talk.yml/2.png)\n\n<Notes>First.</Notes>\n",
            ),
            ("two.md", "# Two\n\n<Notes>Second.</Notes>\n"),
        ],
    );
    let (_, notes) = told(&talk).unwrap();
    let out = dir.join("Talk.mp4");
    let heard = Arc::new(Mutex::new(Heard::default()));
    let (router, entry) = served(talk);
    let (router, answered) = testing::answering(router);
    // Long enough slides for the slide's PNG to be drawn while they play.
    film(router, &entry, &out, notes, heard, async |heard| {
        Ok(Tone::start(heard, 4.0))
    })
    .await
    .unwrap();
    let answered = answered.lock().unwrap();
    let png: Vec<_> = answered
        .iter()
        .filter(|(path, _)| path == "/talk.yml/2.png")
        .collect();
    assert_eq!(png, [&("/talk.yml/2.png".to_string(), 200)], "{answered:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// Calls GPT-Live, which bills per minute; run with
/// `cargo test -- --ignored` and `DEVPAGE_OPENAI_API_KEY` set.
#[tokio::test]
#[ignore]
async fn a_deck_is_narrated_by_gpt_live() {
    if std::env::var_os(narrator::OPENAI_API_KEY).is_none() {
        eprintln!("skipped: {} is not set", narrator::OPENAI_API_KEY);
        return;
    }
    let config = narrator::Config::from_env().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let talk = Arc::new(
        Document::load(
            &root.join("tests/fixtures/narrated-talk"),
            &ServedFolder::new(root).unwrap(),
        )
        .unwrap(),
    );
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
