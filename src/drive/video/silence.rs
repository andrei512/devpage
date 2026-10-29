//! The silence rules of a recorded deck: the narrator's voice streams in
//! continuously, silence included, and what reaches the recording has no
//! silence longer than [`MAX_SILENCE_SECONDS`]. Every cut fades out and back
//! in, so it can't click.
//!
//! The voice comes in as 24 kHz mono PCM16, judged in 10 ms windows: a window
//! whose peak is over [`LOUD`] is speech. Silent windows are held back until
//! it is known what follows them — speech (they are kept, up to the cap) or
//! the slide's end (the old slide keeps its share of the gap,
//! `MAX_SILENCE_SECONDS − SLIDE_LEAD_SECONDS`, and the rest is dropped).
//! Whatever silence comes before the next slide's voice is dropped too.

/// The longest silence a recording holds, inside a slide or across a change.
pub const MAX_SILENCE_SECONDS: f64 = 1.0;
/// Of the silence at a slide change, how long the new slide shows before its
/// voice starts; the rest stays on the old slide.
pub const SLIDE_LEAD_SECONDS: f64 = 0.4;
/// Samples per second of the voice, in and out.
pub const RATE: usize = 24_000;
/// A 10 ms window whose peak is above this is speech.
pub const LOUD: i32 = 500;

/// 10 ms.
const WINDOW: usize = RATE / 100;
/// 5 ms of fade on each side of a cut.
const FADE: usize = RATE / 200;

fn samples(seconds: f64) -> usize {
    (seconds * RATE as f64).round() as usize
}

/// Turns the raw voice into what is played and recorded.
#[derive(Debug)]
pub struct Trimmer {
    /// Samples of silence in a row, across slides; the start counts as silent.
    silent_run: usize,
    /// True while silence is being dropped: the next speech fades in.
    cutting: bool,
    /// Silent windows waiting to learn what follows them.
    held: Vec<Vec<i16>>,
    /// Windows ready to play.
    outbox: Vec<Vec<i16>>,
}

impl Default for Trimmer {
    fn default() -> Self {
        Self {
            silent_run: samples(MAX_SILENCE_SECONDS),
            cutting: true,
            held: Vec::new(),
            outbox: Vec::new(),
        }
    }
}

impl Trimmer {
    /// Takes the next stretch of the voice; true when it holds speech.
    pub fn take(&mut self, pcm: &[i16]) -> bool {
        let mut heard = false;
        for window in pcm.chunks(WINDOW) {
            let mut window = window.to_vec();
            let peak = window.iter().map(|s| i32::from(*s).abs()).max();
            if peak.unwrap_or(0) > LOUD {
                heard = true;
                self.silent_run = 0;
                if self.cutting {
                    fade(&mut window, true);
                    self.cutting = false;
                }
                self.outbox.append(&mut self.held);
                self.outbox.push(window);
                continue;
            }
            self.silent_run += window.len();
            if self.silent_run <= samples(MAX_SILENCE_SECONDS) {
                self.held.push(window);
                continue;
            }
            if !self.cutting
                && let Some(last) = self.held.last_mut().or(self.outbox.last_mut())
            {
                fade(last, false);
            }
            self.cutting = true;
        }
        heard
    }

    /// A new slide's voice is about to be asked for: whatever arrived while
    /// waiting is not part of it.
    pub fn start_slide(&mut self) {
        self.outbox.clear();
    }

    /// The slide's voice is done: keep the old slide's share of the gap, drop
    /// the rest, and drop whatever silence comes before the next slide's
    /// voice.
    pub fn end_slide(&mut self) {
        let mut room = samples(MAX_SILENCE_SECONDS - SLIDE_LEAD_SECONDS);
        for window in std::mem::take(&mut self.held) {
            if room >= window.len() {
                room -= window.len();
                self.outbox.push(window);
            }
        }
        if let Some(last) = self.outbox.last_mut() {
            fade(last, false);
        }
        self.silent_run = samples(MAX_SILENCE_SECONDS);
        self.cutting = true;
    }

    /// Samples ready to play.
    pub fn buffered(&self) -> usize {
        self.outbox.iter().map(Vec::len).sum()
    }

    /// Hands over the samples ready to play.
    pub fn drain(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.outbox).concat()
    }
}

/// Fades the start of `window` in, or its end out, over [`FADE`] samples.
fn fade(window: &mut [i16], fade_in: bool) {
    let n = FADE.min(window.len());
    let len = window.len();
    for i in 0..n {
        let (at, gain) = if fade_in {
            (i, i as f64 / n as f64)
        } else {
            (len - n + i, 1.0 - i as f64 / n as f64)
        };
        window[at] = (f64::from(window[at]) * gain).round() as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of speech: a square wave well over [`LOUD`].
    fn speech(seconds: f64) -> Vec<i16> {
        (0..samples(seconds))
            .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
            .collect()
    }

    fn quiet(seconds: f64) -> Vec<i16> {
        vec![0; samples(seconds)]
    }

    /// Lengths, in seconds, of the runs of speech and silence in `pcm`, a run
    /// of silence negative. Faded samples count as what they fade.
    fn runs(pcm: &[i16]) -> Vec<f64> {
        let mut runs: Vec<(bool, usize)> = Vec::new();
        for window in pcm.chunks(WINDOW) {
            let loud = window.iter().any(|s| i32::from(*s).abs() > LOUD);
            match runs.last_mut() {
                Some((was, n)) if *was == loud => *n += window.len(),
                _ => runs.push((loud, window.len())),
            }
        }
        runs.into_iter()
            .map(|(loud, n)| {
                let seconds = n as f64 / RATE as f64;
                if loud { seconds } else { -seconds }
            })
            .collect()
    }

    #[test]
    fn a_silence_inside_a_slide_is_capped() {
        let mut trimmer = Trimmer::default();
        assert!(trimmer.take(&speech(0.5)));
        assert!(!trimmer.take(&quiet(3.0)));
        assert!(trimmer.take(&speech(0.5)));
        assert_eq!(runs(&trimmer.drain()), [0.5, -MAX_SILENCE_SECONDS, 0.5]);
    }

    #[test]
    fn a_silence_under_the_cap_is_kept_whole() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&speech(0.5));
        trimmer.take(&quiet(0.7));
        trimmer.take(&speech(0.5));
        assert_eq!(runs(&trimmer.drain()), [0.5, -0.7, 0.5]);
    }

    #[test]
    fn silence_waits_until_it_is_known_what_follows() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&speech(0.5));
        trimmer.take(&quiet(0.5));
        assert_eq!(trimmer.buffered(), samples(0.5));
    }

    #[test]
    fn a_slides_end_keeps_its_share_of_the_gap_and_the_next_slide_starts_on_speech() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&speech(0.5));
        trimmer.take(&quiet(0.9));
        trimmer.end_slide();
        let old = trimmer.drain();
        let share = MAX_SILENCE_SECONDS - SLIDE_LEAD_SECONDS;
        assert_eq!(runs(&old), [0.5, -share]);

        trimmer.start_slide();
        trimmer.take(&quiet(2.0));
        assert_eq!(trimmer.buffered(), 0);
        trimmer.take(&speech(0.5));
        let new = trimmer.drain();
        assert_eq!(runs(&new), [0.5]);
    }

    #[test]
    fn a_short_gap_at_a_slides_end_is_kept_whole() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&speech(0.5));
        trimmer.take(&quiet(0.3));
        trimmer.end_slide();
        assert_eq!(runs(&trimmer.drain()), [0.5, -0.3]);
    }

    #[test]
    fn the_voice_starts_with_no_silence_before_it() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&quiet(0.4));
        trimmer.take(&speech(0.5));
        assert_eq!(runs(&trimmer.drain()), [0.5]);
    }

    #[test]
    fn whatever_arrived_before_a_slide_is_asked_for_is_not_part_of_it() {
        let mut trimmer = Trimmer::default();
        trimmer.take(&speech(0.5));
        trimmer.start_slide();
        assert_eq!(trimmer.buffered(), 0);
    }

    #[test]
    fn every_cut_fades_out_and_back_in() {
        let mut trimmer = Trimmer::default();
        // Speech straight into a long silence: the cut lands on the last
        // held window, which is silent, so the speech before it is untouched.
        trimmer.take(&speech(0.1));
        trimmer.take(&quiet(2.0));
        // Speech after the cut fades in from zero.
        trimmer.take(&speech(0.1));
        // The slide ends on speech: its last samples fade out to near zero.
        trimmer.end_slide();
        let pcm = trimmer.drain();
        let resumed = samples(0.1) + samples(MAX_SILENCE_SECONDS);
        assert_eq!(pcm[resumed], 0);
        assert!(i32::from(pcm[resumed + FADE / 2]).abs() < 8000);
        assert_eq!(i32::from(pcm[resumed + FADE]).abs(), 8000);
        let end = pcm.len();
        assert!(i32::from(pcm[end - 1]).abs() < 100);
        assert!(i32::from(pcm[end - FADE / 2]).abs() < 8000);
        assert_eq!(i32::from(pcm[end - FADE - 1]).abs(), 8000);
        // The very first speech fades in too: the start counts as a cut.
        assert_eq!(pcm[0], 0);
    }
}
