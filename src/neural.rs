//! Beat This! on a rolling window: hand it the last ~10 s, get the beat back.

use crate::Beat;
use beat_this::{BeatThis, Runtime, RtenRuntime};
use std::cell::Cell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// How much it hears per listen: 10 s scored best (eval: 5 s loses ~15 points, 15 s gains none).
pub const LISTEN_S: f32 = 10.0;
/// First guess after this much sound.
pub const FIRST_S: f32 = 5.0;
/// The long read: the trailing 30 s, every 15 s once 15 s are in. It's the
/// authority once it agrees with the lock (LAB.md E9): a fill or a break that
/// fools one 10 s read averages out over 30. Every 15 s scores as well as every
/// 5 (E13) at a third of the cost — a 30 s read is ~8× a 10 s one.
pub const LONG_S: f32 = 30.0;
pub const LONG_MIN_S: f32 = 15.0;
const LONG_EVERY_S: f32 = 15.0;
/// Mean square below this is silence (drumkit's quiet Mac): nothing to hear, so
/// the model doesn't run.
const SILENT: f32 = 1e-6;

type Model = <RtenRuntime as Runtime>::Model;

/// Where scripts/fetch-models.sh puts the models.
pub fn model_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/tempo/models")
}

pub struct Neural {
    bt: BeatThis<Model>,
}

impl Neural {
    /// `full`: the 83 MB model (best) or the 10 MB small one (faster).
    pub fn new(dir: &Path, full: bool) -> Result<Self, String> {
        let beat = dir.join(if full { "beat_this.onnx" } else { "beat_this_small.onnx" });
        BeatThis::new(&RtenRuntime, &dir.join("mel_spectrogram.onnx"), &beat)
            .map(|bt| Neural { bt })
            .map_err(|e| format!("{e} (run maschine/scripts/fetch-models.sh)"))
    }

    /// The beat in `mono` (any rate), as of its last sample.
    pub fn listen(&mut self, mono: &[f32], rate: u32) -> Option<Beat> {
        let a = self.bt.analyze_audio(mono, rate).ok()?;
        from_marks(&a.beats, &a.downbeats, mono.len() as f32 / rate as f32)
    }
}

/// When the listener reads, in audio time: a short read per second of new sound
/// once `FIRST_S` are in, a long one per `LONG_EVERY_S` of it once `LONG_MIN_S`
/// are. With no new sound (TEMPO off) nothing is due.
pub struct Pace {
    rate: usize,
    new: usize,      // samples since the last short read
    new_long: usize, // samples since the last long read
}

impl Pace {
    pub fn new(rate: u32) -> Pace {
        Pace { rate: rate as usize, new: 0, new_long: 0 }
    }

    /// `n` new samples arrived.
    pub fn heard(&mut self, n: usize) {
        (self.new, self.new_long) = (self.new + n, self.new_long + n);
    }

    /// Is a short read due over a ring of `ring` samples? (Taking it.)
    pub fn short(&mut self, ring: usize) -> bool {
        let due = ring >= (FIRST_S * self.rate as f32) as usize && self.new >= self.rate;
        if due {
            self.new = 0;
        }
        due
    }

    /// Is a long read due? (Taking it.)
    fn long(&mut self, ring: usize) -> bool {
        let due = ring >= (LONG_MIN_S * self.rate as f32) as usize && self.new_long >= (LONG_EVERY_S * self.rate as f32) as usize;
        if due {
            self.new_long = 0;
        }
        due
    }
}

/// Beat This! on its own thread: feed it audio as it arrives, and once a
/// second it hears the last `LISTEN_S` and reports the beat, stamped with the
/// instant `hear` got its last sample; every 15 s it also hears the last `LONG_S`
/// (`latest_long`). The model loads on the first listen.
pub struct Listener {
    tx: Sender<(Vec<f32>, Instant)>,
    rx: Receiver<(Beat, Instant, u64)>,
    rx_long: Receiver<(Beat, Instant, u64)>,
    /// resets sent; the thread tags each read with the resets it had applied, so a
    /// read of audio from before a reset is dropped
    gen: Cell<u64>,
}

impl Listener {
    pub fn spawn(rate: u32, full: bool) -> Listener {
        Listener::spawn_with(rate, move || Neural::new(&model_dir(), full).map(|mut n| move |x: &[f32]| n.listen(x, rate)))
    }

    /// `spawn` with the ear `load` makes on the first listen (a test's stands in for Beat This!).
    fn spawn_with<E: FnMut(&[f32]) -> Option<Beat>>(rate: u32, mut load: impl FnMut() -> Result<E, String> + Send + 'static) -> Listener {
        let (tx, audio) = channel::<(Vec<f32>, Instant)>();
        let (out, rx) = channel();
        let (out_long, rx_long) = channel();
        std::thread::spawn(move || {
            let mut resets = 0u64;
            let mut ear: Option<E> = None;
            let mut ring: VecDeque<f32> = VecDeque::new();
            let (cap, short) = ((LONG_S * rate as f32) as usize, (LISTEN_S * rate as f32) as usize);
            let mut pace = Pace::new(rate);
            let mut heard_at = Instant::now();
            loop {
                // Take everything queued (inference takes up to seconds and audio
                // keeps arriving), so the read is of the newest sound (Codex R3). It's
                // stamped with when that came, not now: if main stalled while the
                // worker listened, now is a stall late (Codex, /rev round 7).
                let first = match audio.recv_timeout(Duration::from_millis(100)) {
                    Ok(chunk) => Some(chunk),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => return, // the engine dropped its Listener
                };
                for (chunk, came) in first.into_iter().chain(audio.try_iter()) {
                    if chunk.is_empty() {
                        ring.clear(); // reset
                        resets += 1;
                    } else {
                        pace.heard(chunk.len());
                        ring.extend(chunk);
                        heard_at = came;
                    }
                }
                let over = ring.len().saturating_sub(cap);
                ring.drain(..over);
                if !pace.short(ring.len()) {
                    continue;
                }
                if ear.is_none() {
                    match load() {
                        Ok(n) => ear = Some(n),
                        Err(e) => {
                            eprintln!("tempo: Beat This! unavailable: {e}");
                            return;
                        }
                    }
                }
                let (at, g) = (heard_at, resets);
                let x: Vec<f32> = ring.iter().skip(ring.len().saturating_sub(short)).copied().collect();
                if x.iter().map(|v| v * v).sum::<f32>() / (x.len() as f32) < SILENT {
                    continue; // ponytail: silence costs nothing; the long read waits for sound too
                }
                if let Some(b) = ear.as_mut().unwrap()(&x) {
                    let _ = out.send((b, at, g));
                }
                if pace.long(ring.len()) {
                    let x: Vec<f32> = ring.iter().copied().collect();
                    if let Some(b) = ear.as_mut().unwrap()(&x) {
                        let _ = out_long.send((b, at, g)); // as of the same last sample
                    }
                }
            }
        });
        Listener { tx, rx, rx_long, gen: Cell::new(0) }
    }

    pub fn hear(&self, chunk: Vec<f32>) {
        let _ = self.tx.send((chunk, Instant::now()));
    }

    /// Forget what it heard (a new listen starts from silence).
    /// Returns whether the thread is still there to hear it: it ends when the model won't
    /// load, and then the caller needs a new Listener.
    pub fn reset(&self) -> bool {
        self.gen.set(self.gen.get() + 1);
        self.tx.send((Vec::new(), Instant::now())).is_ok()
    }

    /// The newest report since the last call, if any.
    pub fn latest(&self) -> Option<(Beat, Instant)> {
        let g = self.gen.get();
        self.rx.try_iter().filter(|r| r.2 == g).last().map(|(b, at, _)| (b, at))
    }

    /// The newest long read (≥ `LONG_MIN_S`) since the last call, if any.
    pub fn latest_long(&self) -> Option<(Beat, Instant)> {
        let g = self.gen.get();
        self.rx_long.try_iter().filter(|r| r.2 == g).last().map(|(b, at, _)| (b, at))
    }
}

/// Beat and downbeat times (s) → tempo (median gap), meter (most common bar
/// length), and how long ago the last beat and the last "one" were.
pub fn from_marks(beats: &[f32], downbeats: &[f32], end: f32) -> Option<Beat> {
    // 0.21 s, not 0.2: off the model's 20 ms grid, so f32 and f64 agree on the edge
    let mut gaps: Vec<f32> = beats.windows(2).map(|w| w[1] - w[0]).filter(|g| (0.21..2.0).contains(g)).collect();
    if gaps.len() < 3 {
        return None;
    }
    gaps.sort_by(f32::total_cmp);
    let gap = gaps[gaps.len() / 2];
    let kept = dedup(beats, gap);
    // The model marks beats on a 20 ms grid, so the median gap is quantised
    // (90.9, 88.2, …). A straight line through every beat — beat number against
    // time — reads the period between the grid lines.
    let pts = numbered(&kept, gap);
    if pts.len() < 4 {
        return None;
    }
    let (period, _) = line(&pts).unwrap_or((gap, 0.0));
    // Where the last beat fell: the line through the last 8, not the last mark
    // alone — the window's edge has no future to judge it by (LAB.md E11a).
    let tail = &pts[pts.len().saturating_sub(8)..];
    let fit = line(tail);
    let last = fit.map_or(tail[tail.len() - 1].1, |(p, c)| c + p * tail[tail.len() - 1].0);
    // The "one" on that same line: the downbeat says which beat, the fit says when
    // (a raw downbeat mark is as edge-noisy as a raw beat, and drumkit anchors its
    // clock on it).
    let one = downbeats.last().map(|&d| fit.map_or(d, |(p, c)| c + p * ((d - c) / p).round()));
    let mut bars = [0usize; 8];
    for w in downbeats.windows(2) {
        let n = kept.iter().filter(|&&b| b >= w[0] - 0.01 && b < w[1] - 0.01).count();
        if (2..8).contains(&n) {
            bars[n] += 1;
        }
    }
    let meter = (2..8).max_by_key(|&n| bars[n]).filter(|&n| bars[n] > 0).unwrap_or(4) as u8;
    Some(Beat {
        bpm: 60.0 / period,
        conf: 1.0, // not graded: the Lock judges a read by whether the next ones agree
        since_ms: (end - last) * 1000.0,
        meter,
        one_ms: one.map(|d| (end - d) * 1000.0),
    })
}

/// A beat less than half a gap after the one before is the same beat heard
/// twice (the model splits one, 0.83 + 0.17): numbered, it would put every
/// later beat one off (LAB.md E12). Marks sit on a 20 ms grid, so exact
/// half-gap ties are common: they're kept, decided a quarter frame (5 ms) out.
fn dedup(beats: &[f32], gap: f32) -> Vec<f32> {
    let mut kept: Vec<f32> = Vec::with_capacity(beats.len());
    for &t in beats {
        if kept.last().is_none_or(|&p| t - p >= gap / 2.0 - 0.005) {
            kept.push(t);
        }
    }
    kept
}

/// (beat number, time) for each beat, numbered from the one before it (a gap
/// of ~2 is a missed beat): a rounding error in `gap` can't pile up over a long
/// listen (LAB.md E1).
fn numbered(kept: &[f32], gap: f32) -> Vec<(f32, f32)> {
    let mut k = 0.0f32;
    kept.iter()
        .enumerate()
        .map(|(i, &t)| {
            if i > 0 {
                // an x.5 ratio is a tie (common on the 20 ms grid): up, a hair past the half
                k += ((t - kept[i - 1]) / gap + 0.501).floor();
            }
            (k, t)
        })
        .collect()
}

/// Least squares through (beat number, time): (seconds per beat, time of beat 0).
fn line(pts: &[(f32, f32)]) -> Option<(f32, f32)> {
    let n = pts.len() as f32;
    let (mk, mt) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
    let (sxy, sxx) = pts.iter().fold((0.0, 0.0), |(a, b), p| (a + (p.0 - mk) * (p.1 - mt), b + (p.0 - mk).powi(2)));
    (sxx > 0.0).then(|| (sxy / sxx, mt - sxy / sxx * mk))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 100 samples a second keeps the arithmetic by hand: FIRST_S 5 s = 500,
    // LONG_MIN_S and LONG_EVERY_S 15 s = 1500.

    #[test]
    fn no_new_sound_no_read() {
        // TEMPO off stops the sound; the old pace re-read the last ring every second
        let mut p = Pace::new(100);
        p.heard(1000);
        assert!(p.short(1000));
        assert!(!p.short(1000), "read again with nothing new");
        p.heard(99);
        assert!(!p.short(1000), "under a second of new sound");
        p.heard(1);
        assert!(p.short(1000));
    }

    #[test]
    fn a_long_read_needs_new_sound_too() {
        let mut p = Pace::new(100);
        p.heard(1500);
        assert!(p.long(1500));
        assert!(!p.long(1500), "read again with nothing new");
        p.heard(1500);
        assert!(p.long(3000));
    }

    #[test]
    fn a_read_is_stamped_when_its_sound_came_not_when_the_worker_got_to_it() {
        // Codex, /rev round 7: the newest sound comes while the worker listens, then
        // main stalls, so the worker reads it a listen later. A read stamped then puts
        // the beat a listen late (and its long read, as late, confirms it).
        let (listening, listens) = channel();
        let l = Listener::spawn_with(100, move || {
            let listening = listening.clone();
            Ok(move |x: &[f32]| {
                let _ = listening.send(());
                std::thread::sleep(Duration::from_millis(200));
                Some(Beat { bpm: x.len() as f32, conf: 1.0, since_ms: 0.0, meter: 4, one_ms: None })
            })
        });
        l.hear(vec![0.5; 500]); // 5 s: the first read
        listens.recv_timeout(Duration::from_secs(5)).unwrap();
        l.hear(vec![0.5; 50]);
        std::thread::sleep(Duration::from_millis(1));
        let sent = Instant::now();
        l.hear(vec![0.5; 50]); // the newest, then nothing
        let came = sent..=Instant::now();
        let deadline = Instant::now() + Duration::from_secs(5);
        let at = loop {
            if let Some((_, at)) = l.latest().filter(|(b, _)| b.bpm == 600.0) {
                break at;
            }
            assert!(Instant::now() < deadline, "no read of the newest sound");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(came.contains(&at), "stamped {:?} after its sound came", at.saturating_duration_since(*came.end()));
    }

    #[test]
    fn marks_become_tempo_meter_and_the_one() {
        // 120 BPM in 3/4: beats every 0.5 s, a downbeat every third.
        let beats: Vec<f32> = (0..18).map(|i| i as f32 * 0.5).collect();
        let downs: Vec<f32> = beats.iter().copied().step_by(3).collect();
        let b = from_marks(&beats, &downs, 8.7).unwrap();
        assert!((b.bpm - 120.0).abs() < 0.01);
        assert_eq!(b.meter, 3);
        assert!((b.since_ms - 200.0).abs() < 1.0);
        assert!((b.one_ms.unwrap() - 1200.0).abs() < 1.0);
    }

    #[test]
    fn reads_the_tempo_between_the_20_ms_grid_lines() {
        // 92 BPM (0.652 s) marked on the model's 20 ms grid: the median gap says 90.9 or 93.75
        let beats: Vec<f32> = (0..15).map(|i| ((i as f32 * 60.0 / 92.0) / 0.02).round() * 0.02).collect();
        let b = from_marks(&beats, &[], 10.0).unwrap();
        assert!((b.bpm - 92.0).abs() < 0.2, "{}", b.bpm);
    }

    #[test]
    fn a_split_beat_is_one_beat() {
        // 126 BPM with one beat heard twice (0.83 + 0.17 of a gap): still 126
        let gap = 60.0 / 126.0;
        let mut beats: Vec<f32> = (0..40).map(|i| i as f32 * gap).collect();
        beats.insert(11, 10.0 * gap + 0.83 * gap);
        let b = from_marks(&beats, &[], 40.0 * gap).unwrap();
        assert!((b.bpm - 126.0).abs() < 0.1, "{}", b.bpm);
    }

    #[test]
    fn the_one_lands_on_the_beat_line() {
        // 120 BPM 4/4, the last downbeat marked 30 ms late: the one is still on the beat
        let beats: Vec<f32> = (0..20).map(|i| i as f32 * 0.5).collect();
        let downs = [0.0, 2.0, 4.0, 6.0, 8.03];
        let b = from_marks(&beats, &downs, 10.0).unwrap();
        assert!((b.one_ms.unwrap() - 2000.0).abs() < 1.0, "{:?}", b.one_ms);
    }

    #[test]
    fn a_stray_mark_anywhere_barely_moves_the_tempo() {
        // Codex R3: a mark at any fraction of a gap, before or after a true beat,
        // in a 30 s read at 120 BPM on the model's 20 ms grid
        for f in 1..10 {
            let gap = 0.5f32;
            let mut beats: Vec<f32> = (0..60).map(|i| i as f32 * gap).collect();
            beats.insert(31, (30.0 + f as f32 / 10.0) * gap);
            let beats: Vec<f32> = beats.iter().map(|t| (t / 0.02).round() * 0.02).collect();
            let b = from_marks(&beats, &[], 30.0).unwrap();
            assert!((b.bpm - 120.0).abs() < 0.5, "stray at {f}/10: {}", b.bpm);
        }
    }

    #[test]
    fn missed_beats_dont_move_the_tempo() {
        let mut beats: Vec<f32> = (0..60).map(|i| i as f32 * 60.0 / 97.0).collect();
        for i in [50, 33, 32, 12] {
            beats.remove(i);
        }
        let b = from_marks(&beats, &[], 37.2).unwrap();
        assert!((b.bpm - 97.0).abs() < 0.05, "{}", b.bpm);
    }

    #[test]
    fn every_tempo_and_grid_offset_reads_within_a_bpm() {
        // Codex R3: sweep tempo × where the beats fall between the model's 20 ms
        // frames, for a 10 s read (the short one) and a 30 s read (the long one)
        let mut worst = [(0.0f32, 0.0f32, 0.0f32); 2];
        for (w, span) in [10.0f32, 30.0].iter().enumerate() {
            for tenth in 600..=2000 {
                let bpm = tenth as f32 / 10.0;
                for off in [0.0f32, 0.004, 0.009, 0.013, 0.017] {
                    let beats: Vec<f32> = (0..)
                        .map(|i| off + i as f32 * 60.0 / bpm)
                        .take_while(|&t| t < *span)
                        .map(|t| (t / 0.02).round() * 0.02)
                        .collect();
                    let b = from_marks(&beats, &[], *span).unwrap();
                    let e = (b.bpm - bpm).abs();
                    if e > worst[w].0 {
                        worst[w] = (e, bpm, off);
                    }
                }
            }
        }
        println!("worst error: 10 s {:?}, 30 s {:?} (BPM error, at BPM, offset s)", worst[0], worst[1]);
        assert!(worst[0].0 < 1.0 && worst[1].0 < 1.0, "{worst:?}");
    }

    #[test]
    fn too_few_beats_is_no_beat() {
        assert!(from_marks(&[0.0, 0.5, 1.0], &[], 2.0).is_none());
    }

    #[test]
    fn a_mark_exactly_half_a_gap_on_is_kept() {
        // exact half-gap ties are common on the 20 ms grid: kept, here and in lab.py
        // (LAB.md, parity and ties)
        assert_eq!(dedup(&[0.0, 0.18, 0.36], 0.36), [0.0, 0.18, 0.36]);
    }

    #[test]
    fn a_beat_heard_twice_is_kept_once() {
        // the model splits a beat 0.83 + 0.17 of a gap: the second mark goes (E12)
        assert_eq!(dedup(&[0.0, 0.5, 0.915, 1.0, 1.5], 0.5), [0.0, 0.5, 0.915, 1.5]);
    }

    #[test]
    fn a_beat_and_a_half_on_counts_up() {
        // 0.45 / 0.30 is 1.4999999 in f32: the tie goes up all the same, as in lab.py
        assert_eq!(numbered(&[0.0, 0.45, 0.75], 0.3), [(0.0, 0.0), (2.0, 0.45), (3.0, 0.75)]);
    }

    #[test]
    fn a_mark_just_under_half_a_gap_on_doesnt_push_the_count() {
        // off the 20 ms grid dedup can keep one (0.247 of 0.5): the beat after it is
        // still beat 1, or every beat after would be one off
        assert_eq!(numbered(&[0.0, 0.247, 0.5], 0.5)[2].0, 1.0);
    }

    #[test]
    fn faster_than_285_bpm_is_not_a_tempo() {
        // 0.2 s gaps (300 BPM, on the model's grid) are split beats and fills
        let beats: Vec<f32> = (0..30).map(|i| i as f32 * 0.2).collect();
        assert!(from_marks(&beats, &[], 6.0).is_none());
    }

    #[test]
    fn slower_than_30_bpm_is_not_a_tempo() {
        let beats: Vec<f32> = (0..10).map(|i| i as f32 * 2.4).collect();
        assert!(from_marks(&beats, &[], 24.0).is_none());
    }

    #[test]
    fn the_last_beat_is_read_off_the_line_not_its_mark() {
        // 120 BPM, the last mark 40 ms late (the window's edge): the true last beat is
        // 500 ms before the end: the mark alone says 460, the line through the last 8 483
        let mut beats: Vec<f32> = (0..20).map(|i| i as f32 * 0.5).collect();
        *beats.last_mut().unwrap() += 0.04;
        let b = from_marks(&beats, &[], 10.0).unwrap();
        assert!((b.since_ms - 500.0).abs() < 25.0, "{}", b.since_ms);
    }

    #[test]
    fn no_downbeats_means_four_four() {
        let beats: Vec<f32> = (0..20).map(|i| i as f32 * 0.5).collect();
        assert_eq!(from_marks(&beats, &[], 10.0).unwrap().meter, 4);
    }

    fn listener() -> (Listener, Sender<(Beat, Instant, u64)>, Sender<(Beat, Instant, u64)>) {
        let (tx, _) = channel();
        let ((out, rx), (out_long, rx_long)) = (channel(), channel());
        (Listener { tx, rx, rx_long, gen: Cell::new(0) }, out, out_long)
    }

    fn heard(bpm: f32) -> (Beat, Instant, u64) {
        (Beat { bpm, conf: 1.0, since_ms: 0.0, meter: 4, one_ms: None }, Instant::now(), 0)
    }

    #[test]
    fn a_reset_drops_what_was_heard_before_it() {
        let (l, out, out_long) = listener();
        out.send(heard(120.0)).unwrap();
        out_long.send(heard(120.0)).unwrap();
        l.reset();
        assert!(l.latest().is_none());
        assert!(l.latest_long().is_none());
    }

    #[test]
    fn a_reset_says_whether_anyone_is_listening() {
        // the thread ends when the model won't load: drumkit must make a new listener
        // then, not keep the dead one until it restarts (Codex, /rev)
        let (dead, _, _) = listener();
        assert!(!dead.reset());
        let (tx, _audio) = channel();
        let ((_, rx), (_, rx_long)) = (channel(), channel());
        assert!(Listener { tx, rx, rx_long, gen: Cell::new(0) }.reset());
    }

    #[test]
    fn the_newest_read_wins() {
        let (l, out, _) = listener();
        out.send(heard(120.0)).unwrap();
        out.send(heard(121.0)).unwrap();
        assert_eq!(l.latest().map(|(b, _)| b.bpm), Some(121.0));
    }
}
