//! The TEMPO lock (LAB.md E9, E9b), the same rules as eval/live.py `anchored()`:
//! short reads (10 s) acquire it when two in a row agree; a long read (30 s)
//! within 1% of the last five short reads anchors it, and from then on short
//! reads only move the phase — unless five in a row agree somewhere else, a
//! real tempo change, which drops the anchor and re-locks there. Tempos are in
//! the ear's units (drumkit applies its knob and loop octave on top).

use crate::{nearest_octave, Beat};

pub struct Lock {
    need: usize,
    reads: Vec<f32>,
    recent: Vec<f32>,
    elsewhere: Vec<f32>,
    meters: Vec<u8>,
    pub anchored: bool,
    /// The locked tempo (ear units). The owner may move it by octaves (a loop's).
    pub locked: Option<f32>,
}

const GATE: f32 = 0.03; // anchored: a short read this close only moves the phase
const AGREE: f32 = 0.015; // two reads agree
const MOVED: usize = 5; // anchored: this many agreeing reads outside the gate = a new tempo
const ANCHOR: f32 = 0.01; // a long read anchors this close to the short reads' median
const METERS: usize = 16; // the meter is the vote of this many recent reads, so a new song's wins

impl Lock {
    /// `need`: short reads that must agree to lock (the Mac 2, the mic 3).
    pub fn new(need: usize) -> Lock {
        Lock { need, reads: vec![], recent: vec![], elsewhere: vec![], meters: vec![], anchored: false, locked: None }
    }

    /// A short read. Returns the beat to set the clock by (its tempo = the lock), if any.
    pub fn short(&mut self, mut b: Beat) -> Option<Beat> {
        if let Some(l) = self.locked {
            b.bpm = nearest_octave(b.bpm, l); // once locked, the octave sticks
        }
        push(&mut self.recent, b.bpm, 5);
        push(&mut self.meters, b.meter.min(7), METERS);
        b.meter = self.meter();
        let was_anchored = self.anchored;
        let lock = match self.locked.filter(|_| self.anchored) {
            Some(l) if (b.bpm / l - 1.0).abs() < GATE => {
                self.elsewhere.clear();
                Some(l) // the long read's tempo; this read only moves the phase
            }
            Some(_) => {
                push(&mut self.elsewhere, b.bpm, MOVED);
                let moved = self.elsewhere.len() == MOVED && spread(&self.elsewhere) < AGREE;
                if moved {
                    (self.anchored, self.elsewhere) = (false, vec![]);
                }
                moved.then_some(b.bpm)
            }
            None => {
                push(&mut self.reads, b.bpm, self.need);
                (self.reads.len() == self.need && spread(&self.reads) < AGREE).then_some(b.bpm)
            }
        };
        if was_anchored {
            self.reads = vec![b.bpm]; // un-anchored, the next read agrees with this one
        }
        lock.map(|l| {
            self.locked = Some(l);
            Beat { bpm: l, ..b }
        })
    }

    /// A long read (≥ LONG_MIN_S). Returns the beat to set the clock by if it anchors.
    pub fn long(&mut self, mut b: Beat) -> Option<Beat> {
        let l = self.locked?;
        let f = nearest_octave(b.bpm, l);
        let mut now: Vec<f32> = self.recent.iter().map(|&x| nearest_octave(x, l)).collect();
        now.sort_by(f32::total_cmp);
        let &refr = now.get(now.len() / 2)?; // no short reads: nothing to agree with
        if (f / refr - 1.0).abs() >= ANCHOR {
            return None;
        }
        (self.anchored, self.locked) = (true, Some(f));
        (b.bpm, b.meter) = (f, self.meter());
        Some(b)
    }

    fn meter(&self) -> u8 {
        (2..8).max_by_key(|&m| self.meters.iter().filter(|&&x| x == m).count()).unwrap_or(4) // the recent reads' vote
    }
}

fn push<T>(v: &mut Vec<T>, x: T, cap: usize) {
    v.push(x);
    if v.len() > cap {
        v.remove(0);
    }
}

/// max / min − 1
fn spread(v: &[f32]) -> f32 {
    let (lo, hi) = v.iter().fold((f32::MAX, 0.0f32), |(l, h), &x| (l.min(x), h.max(x)));
    hi / lo - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(bpm: f32) -> Beat {
        Beat { bpm, conf: 1.0, since_ms: 0.0, meter: 4, one_ms: None }
    }

    #[test]
    fn two_agreeing_reads_lock() {
        let mut l = Lock::new(2);
        assert!(l.short(beat(120.0)).is_none());
        assert_eq!(l.short(beat(120.5)).map(|b| b.bpm), Some(120.5));
    }

    #[test]
    fn anchored_a_fill_cant_move_it_but_a_new_tempo_can() {
        let mut l = Lock::new(2);
        l.short(beat(118.0));
        l.short(beat(118.1));
        assert_eq!(l.long(beat(118.02)).map(|b| b.bpm), Some(118.02));
        // a fill reads 121.5 (inside the gate: only the phase moves), then 122.9
        // (outside: counted, but one read isn't a new tempo)
        assert_eq!(l.short(beat(121.5)).map(|b| b.bpm), Some(118.02));
        assert!(l.short(beat(122.9)).is_none());
        assert!(l.anchored);
        // the song really goes to 130: the fifth agreeing read outside the gate re-locks
        for _ in 0..4 {
            assert!(l.short(beat(130.0)).is_none());
        }
        assert_eq!(l.short(beat(130.2)).map(|b| b.bpm), Some(130.2));
        assert!(!l.anchored);
    }

    #[test]
    fn a_biased_long_read_doesnt_anchor() {
        let mut l = Lock::new(2);
        for _ in 0..5 {
            l.short(beat(120.0));
        }
        assert!(l.long(beat(122.0)).is_none()); // 1.7% off what the short reads say
        assert!(!l.anchored);
    }

    fn anchored_at(bpm: f32) -> Lock {
        let mut l = Lock::new(2);
        l.short(beat(bpm));
        l.short(beat(bpm));
        l.long(beat(bpm)).expect("anchors");
        l
    }

    #[test]
    fn once_locked_a_read_an_octave_off_counts_at_the_lock() {
        let mut l = Lock::new(2);
        l.short(beat(120.0));
        l.short(beat(120.0));
        assert_eq!(l.short(beat(60.1)).map(|b| b.bpm), Some(120.2)); // half time, heard at 120
    }

    #[test]
    fn the_meter_is_the_reads_vote() {
        let mut l = Lock::new(2);
        let three = |bpm| Beat { meter: 3, ..beat(bpm) };
        l.short(three(120.0));
        l.short(beat(120.0));
        assert_eq!(l.short(three(120.0)).map(|b| b.meter), Some(3)); // two 3s to one 4
    }

    #[test]
    fn a_new_meter_takes_over_from_a_long_old_one() {
        // an hour of 4/4 mustn't outvote the next song's 3/4 for another hour (Codex, /rev)
        let mut l = Lock::new(2);
        for _ in 0..3600 {
            l.short(beat(120.0));
        }
        let mut got = None;
        for _ in 0..16 {
            got = l.short(Beat { meter: 3, ..beat(120.0) }).map(|b| b.meter);
        }
        assert_eq!(got, Some(3));
    }

    #[test]
    fn a_long_read_is_judged_by_the_last_five_short_reads() {
        // six reads at 120, then the song goes to 126 (un-anchored, two reads re-lock
        // it): a long read at 126 agrees with the last five, not with all eleven
        let mut l = Lock::new(2);
        for bpm in [120.0; 6].into_iter().chain([126.0; 5]) {
            l.short(beat(bpm));
        }
        assert_eq!(l.long(beat(126.1)).map(|b| b.bpm), Some(126.1));
    }

    #[test]
    fn one_odd_short_read_doesnt_stop_the_long_read() {
        // the median of the last five: one fill read at 114 among 120s
        let mut l = Lock::new(2);
        for bpm in [120.0, 120.0, 114.0, 120.0, 120.0] {
            l.short(beat(bpm));
        }
        assert!(l.long(beat(120.1)).is_some());
    }

    #[test]
    fn a_long_read_an_octave_off_anchors_at_the_lock() {
        let mut l = Lock::new(2);
        l.short(beat(120.0));
        l.short(beat(120.0));
        assert_eq!(l.long(beat(60.05)).map(|b| b.bpm), Some(120.1));
    }

    #[test]
    fn the_short_reads_follow_the_lock_up_an_octave() {
        // drumkit moves the lock to a loop's octave (locked = 240): the short reads
        // heard at 120 still count, an octave up
        let mut l = Lock::new(2);
        l.short(beat(120.0));
        l.short(beat(120.0));
        l.locked = Some(240.0);
        assert_eq!(l.long(beat(240.2)).map(|b| b.bpm), Some(240.2));
    }

    #[test]
    fn a_new_tempo_needs_five_reads_in_a_row() {
        // anchored at 118: four reads at 130, one back at 118, one more at 130 — no move
        let mut l = anchored_at(118.0);
        for _ in 0..4 {
            l.short(beat(130.0));
        }
        l.short(beat(118.0));
        assert!(l.short(beat(130.0)).is_none());
        assert!(l.anchored);
    }

    #[test]
    fn after_a_tempo_change_the_next_read_keeps_the_new_lock() {
        let mut l = anchored_at(118.0);
        for _ in 0..5 {
            l.short(beat(130.0)); // the fifth re-locks at 130, un-anchored
        }
        assert_eq!(l.short(beat(130.1)).map(|b| b.bpm), Some(130.1));
    }

    #[test]
    fn a_long_read_with_no_short_reads_to_agree_with_doesnt_anchor() {
        // E9b anchors on agreement with the short reads; the owner can set `locked`
        // before any (drumkit does on a new ear)
        let mut l = Lock::new(2);
        l.locked = Some(120.0);
        assert!(l.long(beat(120.5)).is_none());
    }
}
