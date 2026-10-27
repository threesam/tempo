//! The grid finder for a fresh take (a voice memo of mouth sounds, beatbox): find the hits,
//! then the grid of steps they sit on. Pure: no audio I/O. Lifted from the `chop` crate
//! of the drum looper this was built for.

/// One detected sound, as a sample range in the source recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
}

fn quantile(v: &[f32], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[((q * s.len() as f64) as usize).min(s.len() - 1)] as f64
}

/// Find hits. Two detectors, because a mouth makes two kinds of sound:
///  - transients (boom, pap): a sudden jump in level or in brightness
///  - swells (a breath, a hum): no jump at all, just sound after silence
///
/// `sens` 0..1: higher finds more hits.
pub fn detect(x: &[f32], sr: u32, sens: f64) -> Vec<Hit> {
    let sr = sr as f64;
    let hop = ((sr * 0.003).round() as usize).max(1);
    let win = hop * 3;
    let n = x.len().saturating_sub(win) / hop;
    if n < 20 {
        return Vec::new();
    }
    // level, and level of the first difference (a cheap "how bright")
    let (mut env, mut hf) = (vec![0f32; n], vec![0f32; n]);
    for i in 0..n {
        let (mut a, mut b) = (0f64, 0f64);
        for j in i * hop..i * hop + win {
            a += (x[j] as f64).powi(2);
            if j > 0 {
                b += (x[j] as f64 - x[j - 1] as f64).powi(2);
            }
        }
        env[i] = (a / win as f64).sqrt() as f32;
        hf[i] = (b / win as f64).sqrt() as f32;
    }
    let (floor, peak) = (quantile(&env, 0.1), quantile(&env, 0.999));
    if peak <= 0.0 {
        return Vec::new(); // digital silence: a gate of 0 would call all of it one long swell
    }
    let gate = floor * 2.0 + peak * (0.06 - 0.045 * sens);
    let e = floor + peak * 0.01;
    let eh = quantile(&hf, 0.1) + quantile(&hf, 0.999) * 0.01;
    let ms = |t: f64| ((t / 1000.0 * sr / hop as f64).round() as usize).max(1);
    let k = ms(20.0);
    let rise =
        |v: &[f32], i: usize, e: f64| ((v[i] as f64 + e) / (v[i - k] as f64 + e)).ln().max(0.0);
    let mut nov = vec![0f64; n];
    for (i, v) in nov.iter_mut().enumerate().skip(k) {
        *v = rise(&env, i, e) + rise(&hf, i, eh);
    }
    let loud_after = |i: usize, span: usize| {
        env[i..n.min(i + span)]
            .iter()
            .fold(0f64, |m, &v| m.max(v as f64))
    };

    // transients: local maxima of the novelty curve, strongest first
    let (thr, gap) = (2.6 - 2.0 * sens, ms(90.0));
    let mut cand: Vec<usize> = (k + 1..n - 1)
        .filter(|&i| {
            nov[i] > thr
                && nov[i] >= nov[i - 1]
                && nov[i] > nov[i + 1]
                && loud_after(i, ms(60.0)) > gate
        })
        .collect();
    cand.sort_by(|&a, &b| nov[b].total_cmp(&nov[a]));
    let mut picked: Vec<usize> = Vec::new();
    for c in cand {
        if picked.iter().all(|&p| p.abs_diff(c) >= gap) {
            picked.push(c);
        }
    }
    // back each transient up to where the level actually started to jump
    let mut frames: Vec<usize> = picked
        .iter()
        .map(|&c| {
            let (mut best, mut jump) = (c, -1f64);
            for j in c.saturating_sub(k).max(1)..=c {
                let d = env[j] as f64 - env[j - 1] as f64;
                if d > jump {
                    (jump, best) = (d, j);
                }
            }
            best.saturating_sub(1)
        })
        .collect();

    // swells: the level rises through the gate after a real silence
    let (quiet, hold) = (ms(110.0), ms(50.0));
    let mut below = quiet;
    for i in 0..n {
        if (env[i] as f64) < gate {
            below += 1;
            continue;
        }
        if below >= quiet {
            let stays = env[i..n.min(i + hold)]
                .iter()
                .filter(|&&v| v as f64 >= gate)
                .count();
            let near = frames.iter().any(|&f| f + ms(40.0) > i && f < i + ms(90.0));
            if stays as f64 >= hold as f64 * 0.8 && !near {
                frames.push(i);
            }
        }
        below = 0;
    }
    frames.sort_unstable();

    // each hit runs until the sound dies away or the next hit starts
    let (pre, tail, dead) = ((sr * 0.002).round() as usize, ms(25.0), ms(60.0));
    (0..frames.len())
        .map(|idx| {
            let f = frames[idx];
            let next = frames.get(idx + 1).copied().unwrap_or(n);
            let (mut end, mut run) = (next, 0);
            for (j, &v) in env.iter().enumerate().take(next).skip(f + ms(30.0)) {
                run = if (v as f64) < gate { run + 1 } else { 0 };
                if run >= dead {
                    end = next.min(j + tail - run);
                    break;
                }
            }
            let start = (f * hop).saturating_sub(pre);
            Hit {
                start,
                end: x
                    .len()
                    .min((end * hop).max(start + (sr * 0.04).round() as usize)),
            }
        })
        .collect()
}

/// Phase of a grid of `step` seconds that sits closest to `times` (circular mean).
fn phase(times: &[f64], step: f64) -> f64 {
    let (sy, sx) = times.iter().fold((0.0, 0.0), |(sy, sx), t| {
        let a = std::f64::consts::TAU * t / step;
        (sy + a.sin(), sx + a.cos())
    });
    sy.atan2(sx) / std::f64::consts::TAU * step
}

/// The grid step, in seconds, that the performance sits on: the step (120 to
/// 450 ms) where hits land closest to a grid line, measured as a share of the
/// step so finer grids don't win just by being fine.
///
/// Deliberately not "find the BPM, then assume 16ths": a take on 8ths or
/// triplets has a perfectly good tempo and still lands between 16th lines.
// ponytail: brute-force scan (660 steps x hits), plenty for a voice memo. Needs
// ~4 hits to mean anything; fewer returns a plain 250 ms.
pub fn infer_step(times: &[f64]) -> f64 {
    if times.len() < 4 {
        return 0.25;
    }
    let err = |step: f64| {
        let off = phase(times, step);
        times
            .iter()
            .map(|t| ((t - off) / step).rem_euclid(1.0))
            .map(|u| u.min(1.0 - u))
            .sum::<f64>()
    };
    (0..=660)
        .map(|i| 0.12 + i as f64 * 0.0005)
        .min_by(|&a, &b| err(a).total_cmp(&err(b)))
        .unwrap()
}

/// Snap hit times to grid steps. Each hit keeps `drift`: how far the
/// performance was from the grid, in seconds. The first hit is step 0.
pub fn quantize(times: &[f64], step: f64) -> Vec<(usize, f64)> {
    let off = phase(times, step);
    let steps: Vec<i64> = times
        .iter()
        .map(|t| ((t - off) / step).round() as i64)
        .collect();
    let first = steps.iter().copied().min().unwrap_or(0);
    times
        .iter()
        .zip(&steps)
        .map(|(t, &s)| ((s - first) as usize, t - off - s as f64 * step))
        .collect()
}
