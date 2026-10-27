//! Tempo by ear: hold TEMPO and drumkit listens (mic + the Mac's sound) and
//! names the BPM of whatever's playing, beatboxing included.
//!
//! Two halves. `Onsets` runs per sample (cheap enough for the audio thread):
//! every 5 ms it measures how much louder the lows and the highs just got —
//! a kick, a snare, a "pf" or "ts" all show up as a jump. `Tempo` keeps the
//! last 8 s of those jumps and asks which beat period they repeat at
//! (autocorrelation), leaning toward ~110 BPM when two octaves fit equally.

/// Onset frames per second (5 ms hops).
pub const FPS: f32 = 200.0;
const WINDOW: usize = 8 * FPS as usize; // 8 s of history
const MIN_FRAMES: usize = (3.5 * FPS) as usize; // ~6 beats before guessing
const BPM_LO: f32 = 60.0;
const BPM_HI: f32 = 180.0;
const PRIOR_BPM: f32 = 110.0;
const PRIOR_OCTAVES: f32 = 0.6; // how hard the 110 lean pulls; tight enough to beat 3:2 mix-ups
/// Synthetic grooves clear this; real mixes sit lower, so the engine locks on
/// agreement between guesses instead.
#[cfg(test)]
pub(crate) const MIN_CONF: f32 = 0.15;

pub struct Onsets {
    hop: usize,
    n: usize,
    lp: f32,
    a: f32,
    lo: f32,
    hi: f32,
    prev: [f32; 2],
}

impl Onsets {
    pub fn new(rate: u32) -> Self {
        let a = 1.0 - (-2.0 * std::f32::consts::PI * 200.0 / rate as f32).exp(); // 200 Hz split
        Onsets { hop: (rate as f32 / FPS) as usize, n: 0, lp: 0.0, a, lo: 0.0, hi: 0.0, prev: [0.0; 2] }
    }

    /// Feed one mono sample; every hop returns that hop's onset strength.
    pub fn push(&mut self, x: f32) -> Option<f32> {
        self.lp += self.a * (x - self.lp);
        let h = x - self.lp;
        self.lo += self.lp * self.lp;
        self.hi += h * h;
        self.n += 1;
        if self.n < self.hop {
            return None;
        }
        let e = [self.lo, self.hi].map(|v| (1.0 + 1000.0 * v / self.n as f32).ln());
        let flux = (e[0] - self.prev[0]).max(0.0) + (e[1] - self.prev[1]).max(0.0);
        self.prev = e;
        (self.n, self.lo, self.hi) = (0, 0.0, 0.0);
        Some(flux)
    }
}

/// What a listen hears: the tempo, how sure, where the beat is, and the meter.
#[derive(Clone, Copy, Debug)]
pub struct Beat {
    pub bpm: f32,
    pub conf: f32,
    /// ms since the last beat, as of the newest sound heard
    pub since_ms: f32,
    /// beats per bar
    pub meter: u8,
    /// ms since the last downbeat (the "one"), when the ear can tell
    pub one_ms: Option<f32>,
}

pub struct Tempo {
    env: std::collections::VecDeque<f32>,
}

impl Tempo {
    pub fn new() -> Self {
        Tempo { env: std::collections::VecDeque::with_capacity(WINDOW) }
    }

    pub fn clear(&mut self) {
        self.env.clear();
    }

    pub fn push(&mut self, flux: f32) {
        if self.env.len() == WINDOW {
            self.env.pop_front();
        }
        self.env.push_back(flux);
    }

    /// The beat, or None while there's too little sound or no beat in it.
    pub fn estimate(&self) -> Option<Beat> {
        let n = self.env.len();
        if n < MIN_FRAMES {
            return None;
        }
        // Subtract a 0.25 s moving average so only the jumps remain, not loudness.
        let raw: Vec<f32> = self.env.iter().copied().collect();
        let w = (FPS / 4.0) as usize;
        let mut sum: f32 = raw[..w.min(n)].iter().sum();
        let mut x = vec![0.0f32; n];
        for i in 0..n {
            let (lo, hi) = (i.saturating_sub(w / 2), (i + w / 2).min(n - 1));
            if i > 0 {
                sum = raw[lo..=hi].iter().sum(); // ponytail: O(n·w) per estimate, ~400k adds — fine at 4 Hz
            }
            x[i] = (raw[i] - sum / (hi - lo + 1) as f32).max(0.0);
        }
        // Widen each 5 ms spike to ~25 ms so a beat that falls between frames still lines up.
        let x: Vec<f32> = (0..n)
            .map(|i| {
                [1.0, 2.0, 3.0, 2.0, 1.0]
                    .iter()
                    .enumerate()
                    .filter_map(|(j, w)| (i + j).checked_sub(2).and_then(|t| x.get(t)).map(|v| v * w))
                    .sum::<f32>()
                    / 9.0
            })
            .collect();
        let ac = |lag: usize| -> f32 {
            if lag >= n {
                return 0.0;
            }
            (lag..n).map(|i| x[i] * x[i - lag]).sum::<f32>() / (n - lag) as f32
        };
        let zero = ac(0);
        if zero < 1e-6 {
            return None;
        }
        let lag_of = |bpm: f32| 60.0 * FPS / bpm;
        let (l_min, l_max) = (lag_of(BPM_HI).floor() as usize, lag_of(BPM_LO).ceil() as usize);
        let mut best = (0usize, f32::MIN);
        for l in l_min..=l_max {
            // A real beat repeats at 1, 2, 3 and 4 beats (the bar); a syncopated
            // gap (a dotted quarter) repeats at one or two of those, not all.
            // Only lags the window holds at least twice count, averaged, so a
            // short listen doesn't favour fast tempos that fit more multiples.
            let ks: Vec<usize> = (1..=4).filter(|k| k * l * 2 <= n).collect();
            let s = ks
                .iter()
                .map(|&k| (k * l - k / 2..=k * l + k / 2).map(&ac).fold(0.0, f32::max))
                .sum::<f32>()
                / ks.len().max(1) as f32;
            let oct = (lag_of(1.0) / l as f32 / PRIOR_BPM).log2();
            let s = s * (-0.5 * (oct / PRIOR_OCTAVES).powi(2)).exp();
            if s > best.1 {
                best = (l, s);
            }
        }
        let l = best.0;
        // Precision: find the same peak at 4 beats (or 2) and divide back down.
        let k = if 4 * l + 2 < n / 2 { 4 } else if 2 * l + 2 < n * 2 / 3 { 2 } else { 1 };
        let c = k * l;
        let peak = (c - 2..=c + 2).max_by(|&a, &b| ac(a).total_cmp(&ac(b))).unwrap();
        let (y0, y1, y2) = (ac(peak - 1), ac(peak), ac(peak + 1));
        let d = y0 - 2.0 * y1 + y2;
        let off = if d < 0.0 { 0.5 * (y0 - y2) / d } else { 0.0 };
        let lag = (peak as f32 + off) / k as f32;
        // Confidence: how far the beat's peak stands above the typical lag (hiss is flat).
        let typical = (l_min..=l_max).map(&ac).sum::<f32>() / (l_max - l_min + 1) as f32;
        let conf = ((ac(l) - typical) / (zero - typical).max(1e-9)).clamp(0.0, 1.0);

        // Where the beat falls: the offset back from the newest frame whose
        // grid of beats (recent ones weighted more) collects the most onsets.
        let since = (0..lag.ceil() as usize)
            .max_by(|&a, &b| {
                let score = |phi: usize| {
                    (0..)
                        .map(|k| n as f32 - 1.0 - phi as f32 - k as f32 * lag)
                        .take_while(|&t| t >= 0.0)
                        .enumerate()
                        .map(|(k, t)| x[t.round() as usize] * 0.9f32.powi(k as i32))
                        .sum::<f32>()
                };
                score(a).total_cmp(&score(b))
            })
            .unwrap_or(0);

        // Three or four to a bar: which bar length the onsets repeat at more.
        let bar = |m: f32| {
            let ls: Vec<usize> = [m, 2.0 * m]
                .iter()
                .map(|j| (j * lag).round() as usize)
                .filter(|&b| b * 10 <= n * 6)
                .collect();
            ls.iter().map(|&b| (b - 2..=b + 2).map(&ac).fold(0.0, f32::max)).sum::<f32>() / ls.len().max(1) as f32
        };
        let meter = if bar(3.0) > bar(4.0) * 1.15 { 3 } else { 4 }; // 4 unless 3 clearly wins

        Some(Beat { bpm: 60.0 * FPS / lag, conf, since_ms: since as f32 / FPS * 1000.0, meter, one_ms: None })
    }
}

/// The octave of `heard` closest to `near`, but never more than one from `heard`
/// (a 170 heard against a 90 loop is 85). So a read of the true tempo lands within
/// an octave of it even under a lock two octaves off: the full fold kept two dev
/// takes there, 102 of the replay's points (LAB.md, /rev round 9).
pub fn nearest_octave(heard: f32, near: f32) -> f32 {
    [heard / 2.0, heard, heard * 2.0]
        .into_iter()
        .min_by(|a, b| (a / near).ln().abs().total_cmp(&(b / near).ln().abs()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// A beatbox-ish bar: boom on 1 and the "and" of 2, snare noise on 2 and 4,
    /// hats on every eighth, a little background hiss — at `bpm` for `secs`.
    fn groove(bpm: f32, secs: f32, swing: f32) -> Vec<f32> {
        let beat = 60.0 / bpm * SR as f32;
        let mut out = vec![0.0f32; (secs * SR as f32) as usize];
        let mut seed = 7u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        for v in out.iter_mut() {
            *v = 0.01 * noise();
        }
        let mut hit = |out: &mut Vec<f32>, at: f32, kind: u8| {
            let at = at as usize;
            for i in 0..(0.15 * SR as f32) as usize {
                let Some(o) = out.get_mut(at + i) else { break };
                let t = i as f32 / SR as f32;
                *o += match kind {
                    0 => 0.8 * (2.0 * std::f32::consts::PI * 60.0 * t).sin() * (-t * 25.0).exp(),
                    1 => 0.4 * noise() * (-t * 30.0).exp(),
                    _ => 0.15 * noise() * (-t * 120.0).exp(),
                };
            }
        };
        let mut b = 0.0;
        while b * beat < out.len() as f32 {
            let bar = b % 4.0;
            if bar == 0.0 || bar == 1.5 {
                hit(&mut out, b * beat, 0);
            }
            if bar == 1.0 || bar == 3.0 {
                hit(&mut out, b * beat, 1);
            }
            let half = if b.fract() == 0.5 { swing * beat } else { 0.0 };
            hit(&mut out, b * beat + half, 2);
            b += 0.5;
        }
        out
    }

    fn listen(x: &[f32]) -> Option<Beat> {
        let (mut on, mut t) = (Onsets::new(SR), Tempo::new());
        for &s in x {
            if let Some(f) = on.push(s) {
                t.push(f);
            }
        }
        t.estimate()
    }

    fn hear(x: &[f32]) -> Option<(f32, f32)> {
        listen(x).map(|b| (b.bpm, b.conf))
    }

    /// Boom on the one, chick on two and three: a waltz.
    fn waltz(bpm: f32, secs: f32) -> Vec<f32> {
        let beat = (60.0 / bpm * SR as f32) as usize;
        let mut out = vec![0.0f32; (secs * SR as f32) as usize];
        for (b, at) in (0..out.len()).step_by(beat).enumerate() {
            for i in 0..(0.12 * SR as f32) as usize {
                let Some(o) = out.get_mut(at + i) else { break };
                let t = i as f32 / SR as f32;
                *o += if b % 3 == 0 {
                    0.9 * (2.0 * std::f32::consts::PI * 60.0 * t).sin() * (-t * 20.0).exp()
                } else {
                    0.25 * ((i * 7919 % 113) as f32 / 56.0 - 1.0) * (-t * 60.0).exp()
                };
            }
        }
        out
    }

    #[test]
    fn finds_where_the_beat_falls() {
        // 120 BPM from sample 0 for 8.2 s: the newest beat landed 200 ms ago.
        let b = listen(&groove(120.0, 8.2, 0.0)).unwrap();
        let err = (b.since_ms - 200.0).rem_euclid(500.0);
        assert!(err.min(500.0 - err) < 20.0, "since {:.0} ms", b.since_ms);
    }

    #[test]
    fn tells_three_from_four() {
        assert_eq!(listen(&groove(110.0, 8.0, 0.0)).unwrap().meter, 4);
        let w = listen(&waltz(150.0, 8.0)).unwrap();
        assert_eq!(w.meter, 3, "waltz heard at {:.1} in {}", w.bpm, w.meter);
    }

    #[test]
    fn names_the_tempo_of_a_groove() {
        for bpm in [72.0, 88.0, 95.0, 110.0, 120.0, 128.0, 140.0, 170.0] {
            let (got, conf) = hear(&groove(bpm, 8.0, 0.0)).expect("a beat");
            let got = nearest_octave(got, bpm);
            assert!((got - bpm).abs() < 0.6, "{bpm} BPM heard as {got:.2} (conf {conf:.2})");
            assert!(conf > MIN_CONF, "{bpm} BPM too unsure ({conf:.2})");
        }
    }

    #[test]
    fn swing_and_a_short_listen_still_land() {
        let (got, _) = hear(&groove(92.0, 4.0, 0.08)).expect("a beat");
        assert!((nearest_octave(got, 92.0) - 92.0).abs() < 1.5, "heard {got:.2}");
    }

    #[test]
    fn hip_hop_tempos_are_not_doubled() {
        for bpm in [80.0, 90.0, 100.0] {
            let (got, _) = hear(&groove(bpm, 8.0, 0.0)).unwrap();
            assert!((got - bpm).abs() < 0.6, "{bpm} heard as {got:.2}");
        }
    }

    #[test]
    fn silence_and_hiss_have_no_beat() {
        assert!(hear(&vec![0.0; SR as usize * 4]).is_none());
        let mut s = 1u32;
        let hiss: Vec<f32> = (0..SR * 4)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.05 * (s as f32 / u32::MAX as f32 - 0.5)
            })
            .collect();
        if let Some((_, conf)) = hear(&hiss) {
            assert!(conf < MIN_CONF, "hiss looked like a beat ({conf:.2})");
        }
    }

    #[test]
    fn octave_follows_the_loop() {
        assert_eq!(nearest_octave(170.0, 90.0), 85.0);
        assert_eq!(nearest_octave(60.0, 118.0), 120.0);
        assert_eq!(nearest_octave(121.0, 118.0), 121.0);
    }

    #[test]
    fn a_read_moves_one_octave_at_most() {
        // not the full fold: a 50 BPM take first locked at 149 heard its 50 and came to
        // 100, not 200; a 130 song locked at 32.5 heard 130, came to 65 (dev, /rev round 9)
        assert_eq!(nearest_octave(50.0, 149.0), 100.0);
        assert_eq!(nearest_octave(130.0, 32.5), 65.0);
    }
}
