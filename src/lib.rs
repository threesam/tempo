//! Two ears for the tempo of what's playing:
//! - `dsp`: onset flux + autocorrelation. Cheap, runs every half second, first to guess.
//! - `neural`: Beat This! (Foscarin, Schlüter & Widmer, ISMIR 2024), a transformer that
//!   marks beats AND downbeats, so it also knows the bar and where the "one" is.
//! - `grid`: the grid finder for a fresh take (beatbox, hits): Beat This! is trained
//!   on recorded music and mostly hears no beat in a mouth; the grid finder does.
//! `eval` (examples/eval.rs) scores them against recordings with a known tempo.

mod dsp;
pub mod grid;
pub mod lock;
pub mod neural;

/// A fresh take's beat from its hits: the grid they sit on, read as 8th notes
/// (the beat = two grid steps, on the half of the lines with more hits: LAB.md
/// E17). Needs ~4 hits. `x` mono at `rate`, ending now.
pub fn grid_beat(x: &[f32], rate: u32) -> Option<Beat> {
    let times: Vec<f64> = grid::detect(x, rate, 0.5).iter().map(|h| h.start as f64 / rate as f64).collect();
    if times.len() < 4 {
        return None;
    }
    let step = grid::infer_step(&times);
    let q = grid::quantize(&times, step);
    let (newest, drift) = *q.last()?;
    // The beat is the half of the lines with more hits (a tie keeps the newest's):
    // the booms and paps, not the breath on the "and" between them.
    let odd = q.iter().filter(|(i, _)| i % 2 == 1).count();
    let half = if 2 * odd == q.len() { newest % 2 } else { (2 * odd > q.len()) as usize };
    let line = times.last()? - drift - if newest % 2 == half { 0.0 } else { step };
    let beat = 2.0 * step;
    let end = x.len() as f64 / rate as f64;
    Some(Beat {
        bpm: (60.0 / beat) as f32,
        conf: 1.0,
        since_ms: ((end - line).rem_euclid(beat) * 1000.0) as f32,
        meter: 4,
        one_ms: None,
    })
}

pub use lock::Lock;
pub use dsp::{nearest_octave, Beat, Onsets, Tempo, FPS};

#[cfg(test)]
mod tests {
    use super::*;

    /// A mouth's take at 48 kHz, `secs` long: a noise burst (a "pap") at each of `times`.
    fn take(times: &[f64], secs: f64) -> Vec<f32> {
        let mut x = vec![0.0f32; (secs * 48_000.0) as usize];
        let mut seed = 1u32;
        for &t in times {
            let at = (t * 48_000.0) as usize;
            for i in 0..4_000 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if let Some(v) = x.get_mut(at + i) {
                    *v += (seed as f32 / u32::MAX as f32 - 0.5) * (-(i as f32) / 600.0).exp();
                }
            }
        }
        x
    }

    #[test]
    fn grid_reads_a_tresillo_take() {
        // the sea-foam pattern, counted from its breath in: hits on 8ths 0, 3, 5, 7 at 110.9 BPM, 8 s
        let step = 60.0 / 110.9 / 2.0;
        let times: Vec<f64> = (0..4).flat_map(|bar| [0, 3, 5, 7].map(|e| (bar * 8 + e) as f64 * step)).collect();
        let b = grid_beat(&take(&times, 8.0), 48_000).unwrap();
        assert!((b.bpm - 110.9).abs() < 1.0, "{}", b.bpm);
    }

    #[test]
    fn the_beat_is_the_half_of_the_grid_with_more_hits() {
        // sea foam from its one: boom, boom, pap on beats 1, 2 and 3, a breath in on the
        // "and" of 3. Ending on a breath, the beat is under the booms and the pap, not
        // under the newest hit (Codex, /rev round 4; LAB.md E17)
        let eighth = 60.0 / 110.9 / 2.0;
        let times: Vec<f64> = (0..4).flat_map(|bar| [0, 2, 4, 5].map(|e| 0.1 + (bar * 8 + e) as f64 * eighth)).collect();
        let end = times.last().unwrap() + 0.1;
        let b = grid_beat(&take(&times, end), 48_000).unwrap();
        let since_pap = (end - times[times.len() - 2]) * 1000.0; // the last beat: bar 4's pap
        assert!((b.since_ms as f64 - since_pap).abs() < 30.0, "{} vs {since_pap:.0}", b.since_ms);
    }

    #[test]
    fn a_sound_on_every_line_keeps_the_newest_on_the_beat() {
        // boots and cats, a sound on every 8th: nothing says which half is the beat,
        // so the newest hit's line stays one, as before E17
        let times: Vec<f64> = (0..16).map(|k| 0.1 + k as f64 * 0.2).collect();
        let end = times.last().unwrap() + 0.1;
        let b = grid_beat(&take(&times, end), 48_000).unwrap();
        assert!((b.since_ms - 100.0).abs() < 30.0, "{}", b.since_ms);
    }
}

/// Keep a running beat clock in step with a new read (LAB.md E10, E11b).
/// `unit_old` / `unit_new`: ms per counted unit (a beat, or a bar if the clock
/// counts bars) on the running clock and per the read; `off_ms`: the read's mark
/// minus the clock's anchor. Returns how far to move the anchor (ms); the clock
/// then runs at `unit_new` and keeps its count, so nothing double-fires.
/// - In step (same tempo within 1%, drift under 40 ms on the running clock's
///   own period): take the new period, pivoting on the current unit.
/// - Off the beat once: more likely a bad read than a moved beat, so wait.
/// - Off the beat twice the same way (or a new tempo): snap to the read.
pub fn retime(unit_old: f64, unit_new: f64, off_ms: f64, pending: &mut Option<f64>) -> f64 {
    let k = (off_ms / unit_old).round();
    let drift = off_ms - k * unit_old;
    let same = (unit_old / unit_new - 1.0).abs() < 0.01;
    if same && drift.abs() < 40.0 {
        *pending = None;
        k * (unit_old - unit_new)
    } else if same && pending.is_none_or(|p| (drift - p).abs() >= 40.0) {
        *pending = Some(drift);
        k * (unit_old - unit_new)
    } else {
        *pending = None;
        off_ms - k * unit_new
    }
}

#[cfg(test)]
mod clock_tests {
    use super::retime;

    #[test]
    fn takes_the_new_period_without_moving_the_beat() {
        // 120 BPM clock, read says 120.6 (0.5%), its beat lands on beat 40 of the clock
        let (old, new) = (500.0, 60_000.0 / 120.6);
        let mut p = None;
        let shift = retime(old, new, 40.0 * old + 10.0, &mut p);
        // beat 40 stays where it was: anchor + shift + 40·new == anchor + 40·old
        assert!((shift + 40.0 * new - 40.0 * old).abs() < 1e-9);
        assert!(p.is_none());
    }

    #[test]
    fn one_read_off_the_beat_waits_two_snap() {
        let mut p = None;
        let off = 20.0 * 500.0 + 150.0; // 150 ms late
        assert_eq!(retime(500.0, 500.0, off, &mut p), 0.0); // waits
        assert_eq!(p, Some(150.0));
        assert_eq!(retime(500.0, 500.0, off, &mut p), 150.0); // same again: snap, keep the count
        assert!(p.is_none());
    }

    #[test]
    fn a_new_tempo_snaps_at_once() {
        let mut p = None;
        let shift = retime(500.0, 400.0, 10.0 * 500.0 + 100.0, &mut p);
        assert_eq!(shift, 10.0 * 500.0 + 100.0 - 10.0 * 400.0);
    }

    #[test]
    fn a_read_a_little_early_is_in_step_too() {
        // its beat lands 10 ms before beat 40 of the clock: still beat 40
        let (old, new) = (500.0, 60_000.0 / 120.6);
        let mut p = None;
        let shift = retime(old, new, 40.0 * old - 10.0, &mut p);
        assert!((shift + 40.0 * new - 40.0 * old).abs() < 1e-9);
        assert!(p.is_none());
    }

    #[test]
    fn waiting_still_takes_the_new_period() {
        // one read off the beat at a slightly new tempo: the beat waits, the period doesn't
        let (old, new) = (500.0, 60_000.0 / 120.6);
        let mut p = None;
        let shift = retime(old, new, 20.0 * old + 150.0, &mut p);
        assert!((shift + 20.0 * new - 20.0 * old).abs() < 1e-9); // beat 20 stays put
        assert_eq!(p, Some(150.0));
    }

    #[test]
    fn two_misses_that_disagree_both_wait() {
        let mut p = None;
        assert_eq!(retime(500.0, 500.0, 20.0 * 500.0 + 150.0, &mut p), 0.0);
        assert_eq!(retime(500.0, 500.0, 20.0 * 500.0 + 200.0, &mut p), 0.0); // 50 ms from the first
        assert_eq!(p, Some(200.0));
    }

    #[test]
    fn a_read_in_step_forgets_a_miss() {
        let mut p = None;
        let late = 20.0 * 500.0 + 150.0;
        retime(500.0, 500.0, late, &mut p); // waits
        retime(500.0, 500.0, 20.0 * 500.0 + 10.0, &mut p); // in step again
        assert_eq!(retime(500.0, 500.0, late, &mut p), 0.0); // a lone miss again: waits again
    }
}
