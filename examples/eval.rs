//! Score the tempo ears against recordings with a known tempo.
//!
//!   cargo run --release --example eval -- eval/groove.tsv eval/macleod.tsv
//!     [--window 10] [--listens 3] [--only dsp,small,full,grid]
//!     [--speeds 0.8,1,1.25] [--check eval/floors.tsv]
//!
//! Each manifest line: file (relative to the manifest), bpm, time signature
//! (`4/4`, `6/8`, … or 0 = unknown), label. A compound meter (6/8, 9/8, 12/8) is
//! felt in dotted quarters, so its beat is the quarter-note bpm × 2/3 and it has
//! numerator / 3 beats to a bar. Every file is heard `--listens` times, a `--window`-second
//! listen each, spread across the track, and every listen is scored:
//!   exact   within 1 BPM
//!   acc1    within 4% (the MIR standard)
//!   acc2    within 4% of x1/3, x1/2, x1, x2 or x3 (the right pulse, maybe another octave)
//!   meter   beats per bar right (only where the manifest knows the time signature)
//! --speeds replays every file faster/slower (tempo × speed, exactly), so a
//! handful of takes become many known tempos. --check fails (exit 1) when any
//! `ear set metric floor` line of the file scores below its floor: a regression gate.
//!
//! Ears: dsp (the first onset tracker, now the baseline), small / full (Beat
//! This!), grid (chop's grid finder for a fresh take: hits → grid step, read as
//! 8th notes). drumkit listens to the Mac with full and to the mic with grid.

use std::path::Path;
use std::time::Instant;
use tempo::neural::{model_dir, Neural};
use tempo::{Onsets, Tempo};

const SR: u32 = 22_050;

struct Row {
    file: String,
    bpm: f32,
    meter: u8,
    sig: String,
    label: String,
    set: String,
}

#[derive(Default)]
struct Score {
    n: usize,
    exact: usize,
    acc1: usize,
    acc2: usize,
    meter_n: usize,
    meter_ok: usize,
    none: usize,
    ms: f64,
}

fn close(est: f32, truth: f32, tol: f32) -> bool {
    (est - truth).abs() <= tol
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |k: &str, d: &str| -> String {
        args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()).unwrap_or(d.into())
    };
    let window: f32 = opt("--window", "10").parse().unwrap();
    let listens: usize = opt("--listens", "3").parse().unwrap();
    let only = opt("--only", "dsp,small,full,grid");
    let ears: Vec<&str> = only.split(',').collect();
    let speeds: Vec<f32> = opt("--speeds", "1").split(',').map(|v| v.parse().unwrap()).collect();
    let check = args.iter().position(|a| a == "--check").and_then(|i| args.get(i + 1)).cloned();
    let manifests: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| a.ends_with(".tsv") && (*i == 0 || args[i - 1] != "--check"))
        .map(|(_, a)| a)
        .collect();

    let mut rows = Vec::new();
    for m in &manifests {
        let dir = Path::new(m).parent().unwrap();
        let set = Path::new(m).file_stem().unwrap().to_string_lossy().to_string();
        for line in std::fs::read_to_string(m).unwrap().lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            let sig = f.get(2).unwrap_or(&"0").to_string();
            let (num, den) = sig.split_once('/').map_or((0, 4), |(a, b)| (a.parse().unwrap_or(0), b.parse().unwrap_or(4)));
            let compound = den == 8 && num % 3 == 0;
            let bpm: f32 = f[1].parse().unwrap();
            rows.push(Row {
                file: dir.join(f[0]).to_string_lossy().into(),
                bpm: if compound { bpm * 2.0 / 3.0 } else { bpm },
                meter: if compound { num / 3 } else { num } as u8,
                sig,
                label: f.get(3).unwrap_or(&"").to_string(),
                set: set.clone(),
            });
        }
    }

    let mut small = ears.contains(&"small").then(|| Neural::new(&model_dir(), false).unwrap());
    let mut full = ears.contains(&"full").then(|| Neural::new(&model_dir(), true).unwrap());
    let mut scores: std::collections::BTreeMap<(String, String), Score> = Default::default();

    println!("truth  meter | {:<28} | file", ears.iter().map(|e| format!("{e:>6}")).collect::<Vec<_>>().join(" "));
    for r in &rows {
        let original = match beat_this::load_audio(Path::new(&r.file), SR) {
            Ok(a) => a.samples,
            Err(e) => {
                eprintln!("skip {}: {e}", r.file);
                continue;
            }
        };
        for &speed in &speeds {
        let audio = faster(&original, speed);
        let r = &Row { bpm: r.bpm * speed, file: r.file.clone(), meter: r.meter, sig: r.sig.clone(), label: r.label.clone(), set: r.set.clone() };
        let w = (window * SR as f32) as usize;
        let lo = (5 * SR as usize).min(audio.len().saturating_sub(w));
        let hi = audio.len().saturating_sub(w).max(lo);
        for k in 0..listens {
            let at = lo + (hi - lo) * (2 * k + 1) / (2 * listens);
            let x = &audio[at..(at + w).min(audio.len())];
            let mut cells = Vec::new();
            for &ear in &ears {
                let t0 = Instant::now();
                let got: Option<(f32, u8)> = match ear {
                    "dsp" => {
                        let (mut on, mut t) = (Onsets::new(SR), Tempo::new());
                        x.iter().for_each(|&s| {
                            if let Some(f) = on.push(s) {
                                t.push(f)
                            }
                        });
                        t.estimate().map(|b| (b.bpm, b.meter))
                    }
                    "small" => small.as_mut().unwrap().listen(x, SR).map(|b| (b.bpm, b.meter)),
                    "full" => full.as_mut().unwrap().listen(x, SR).map(|b| (b.bpm, b.meter)),
                    "grid" => tempo::grid_beat(x, SR).map(|b| (b.bpm, 0)),
                    _ => panic!("unknown ear {ear}"),
                };
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                for key in [(ear.to_string(), "all".to_string()), (ear.to_string(), r.set.clone())] {
                    let s = scores.entry(key).or_default();
                    s.n += 1;
                    s.ms += ms;
                    match got {
                        None => s.none += 1,
                        Some((bpm, meter)) => {
                            s.exact += close(bpm, r.bpm, 1.0) as usize;
                            s.acc1 += close(bpm, r.bpm, 0.04 * r.bpm) as usize;
                            s.acc2 += [1.0 / 3.0, 0.5, 1.0, 2.0, 3.0]
                                .iter()
                                .any(|m| close(bpm, r.bpm * m, 0.04 * r.bpm * m)) as usize;
                            if r.meter > 0 && meter > 0 {
                                s.meter_n += 1;
                                s.meter_ok += (meter == r.meter) as usize;
                            }
                        }
                    }
                }
                cells.push(got.map_or("    --".into(), |(b, m)| {
                    let mark = if close(b, r.bpm, 0.04 * r.bpm) { ' ' } else { '*' };
                    if m > 0 { format!("{b:5.1}{mark}{m}") } else { format!("{b:6.1}{mark}") }
                }));
            }
            let name = Path::new(&r.file).file_name().unwrap().to_string_lossy();
            println!("{:5.1} {:>5} | {} | {} ({}){}", r.bpm, r.sig, cells.join(" "), name, r.label, if speed != 1.0 { format!(" x{speed}") } else { String::new() });
        }
        }
    }

    let failed = match &check {
        Some(path) => floors_broken(&std::fs::read_to_string(path).unwrap(), &scores),
        None => Vec::new(),
    };
    println!("\near    set        listens  exact   acc1   acc2  meter  no-beat  ms/listen");
    for ((ear, set), s) in &scores {
        let pct = |a: usize, n: usize| if n == 0 { "    -".to_string() } else { format!("{:5.0}%", 100.0 * a as f64 / n as f64) };
        println!(
            "{ear:<6} {set:<10} {:>7} {} {} {} {} {:>8} {:>10.0}",
            s.n,
            pct(s.exact, s.n),
            pct(s.acc1, s.n),
            pct(s.acc2, s.n),
            pct(s.meter_ok, s.meter_n),
            s.none,
            s.ms / s.n as f64
        );
    }
    if check.is_some() {
        if failed.is_empty() {
            println!("\ncheck: every floor held");
        } else {
            println!("\ncheck FAILED:\n  {}", failed.join("\n  "));
            std::process::exit(1);
        }
    }
}

/// The floors (`ear set metric floor%` lines) the scores fall below.
fn floors_broken(floors: &str, scores: &std::collections::BTreeMap<(String, String), Score>) -> Vec<String> {
    let mut failed = Vec::new();
    for line in floors.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (ear, set, metric, floor): (&str, &str, &str, f64) = (f[0], f[1], f[2], f[3].parse().unwrap());
        // a floor nothing was scored for fails: else renaming a manifest switches
        // its floors off without a word (found by /rev)
        let Some(s) = scores.get(&(ear.to_string(), set.to_string())) else {
            failed.push(format!("{ear} {set} {metric}: not scored (no such ear and set in this run)"));
            continue;
        };
        let (hit, n) = match metric {
            "exact" => (s.exact, s.n),
            "acc1" => (s.acc1, s.n),
            "acc2" => (s.acc2, s.n),
            "meter" => (s.meter_ok, s.meter_n),
            m => panic!("unknown metric {m}"),
        };
        let got = if n == 0 { 0.0 } else { 100.0 * hit as f64 / n as f64 };
        if got < floor {
            failed.push(format!("{ear} {set} {metric}: {got:.0}% < floor {floor}%"));
        }
    }
    failed
}

/// Play `x` `speed` times as fast (linear resampling): tempo × speed, pitch too.
fn faster(x: &[f32], speed: f32) -> Vec<f32> {
    if speed == 1.0 {
        return x.to_vec();
    }
    let n = (x.len() as f32 / speed) as usize;
    (0..n)
        .map(|i| {
            let p = i as f32 * speed;
            let (j, f) = (p as usize, p.fract());
            x[j] + (x.get(j + 1).copied().unwrap_or(x[j]) - x[j]) * f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored(ear: &str, set: &str, exact: usize, n: usize) -> std::collections::BTreeMap<(String, String), Score> {
        [((ear.to_string(), set.to_string()), Score { n, exact, ..Default::default() })].into_iter().collect()
    }

    #[test]
    fn a_floor_for_a_set_not_scored_fails() {
        // floors.tsv named groove/macleod after the manifests became groove-dev/
        // macleod-dev: those floors were skipped and the gate said "every floor held"
        let failed = floors_broken("full groove exact 88", &scored("full", "groove-dev", 10, 10));
        assert_eq!(failed, ["full groove exact: not scored (no such ear and set in this run)"]);
    }

    #[test]
    fn a_score_under_its_floor_fails() {
        let failed = floors_broken("full groove-dev exact 94", &scored("full", "groove-dev", 9, 10));
        assert_eq!(failed, ["full groove-dev exact: 90% < floor 94%"]);
    }
}
