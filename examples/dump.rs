//! Run Beat This! once per listen and save what it saw, so experiments on what
//! comes after the model take seconds, not minutes (eval/lab.py reads these).
//!
//!   cargo run --release --example dump -- eval/groove-dev.tsv … [--model full|small] [--causal]
//!
//! Per file: one 30 s stretch (from 25% in, at least 5 s, so a song's intro is
//! skipped), heard the way drumkit hears it — a 10 s listen every 2 s (11 of
//! them) — plus one 30 s listen of the whole stretch, plus the onset curve of the
//! first tracker. Written to eval/cache/<model>/<file>.json; existing files are kept.
//!
//! --causal (Codex R2, E7): the live replay instead. From the track's very start
//! for 60 s: every 1 s a read of the trailing 10 s (from 5 s in, the live
//! listener's first guess), and every 5 s a long read of the trailing min(t, 30) s.
//! Each read is stamped with `end` (seconds since the start). eval/cache/<model>-causal/.

use beat_this::{BeatThis, Runtime, RtenRuntime};
use std::fmt::Write as _;
use std::path::Path;
use tempo::neural::model_dir;
use tempo::Onsets;

const SR: u32 = 22_050;
const STRETCH: f32 = 30.0;
const WINDOW: f32 = 10.0;
const HOP: f32 = 2.0;

fn arr(v: &[f32]) -> String {
    let mut s = String::from("[");
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "{x:.4}").unwrap();
    }
    s.push(']');
    s
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model = args.iter().position(|a| a == "--model").and_then(|i| args.get(i + 1)).map_or("full", |s| s.as_str()).to_string();
    let causal = args.iter().any(|a| a == "--causal");
    // --long <s>: the causal long reads only, of <s> seconds, without logits — to
    // try a cheaper long read (LAB.md E13) against the same short reads
    let long_only: Option<f32> = args.iter().position(|a| a == "--long").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok());
    let causal = causal || long_only.is_some();
    // --speed <f>: the audio played f× faster (tempo and pitch, as eval.rs --speeds),
    // the label × f — a perturbed copy of the set (Codex R3: gains must survive it)
    let speed: f32 = args.iter().position(|a| a == "--speed").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let dir = model_dir();
    let beat = dir.join(if model == "full" { "beat_this.onnx" } else { "beat_this_small.onnx" });
    let mut bt = BeatThis::<<RtenRuntime as Runtime>::Model>::new(&RtenRuntime, &dir.join("mel_spectrogram.onnx"), &beat).unwrap();
    for m in args.iter().filter(|a| a.ends_with(".tsv")) {
        let root = Path::new(m).parent().unwrap();
        let out = root.join("cache").join(match long_only {
            Some(span) => format!("{model}-causal-long{span}"),
            None if causal && speed != 1.0 => format!("{model}-causal-x{speed}"),
            None if causal => format!("{model}-causal"),
            None => model.clone(),
        });
        std::fs::create_dir_all(&out).unwrap();
        for line in std::fs::read_to_string(m).unwrap().lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            let name = Path::new(f[0]).file_name().unwrap().to_string_lossy().to_string();
            let dst = out.join(format!("{name}.json"));
            if dst.exists() {
                continue;
            }
            let t0 = std::time::Instant::now();
            let audio = match beat_this::load_audio(&root.join(f[0]), SR) {
                Ok(a) => faster(&a.samples, speed),
                Err(e) => {
                    eprintln!("skip {name}: {e}");
                    continue;
                }
            };
            let len = audio.len() as f32 / SR as f32;
            if causal {
                let bpm = if speed == 1.0 { f[1].to_string() } else { format!("{:.4}", f[1].parse::<f32>().unwrap() * speed) };
                let mut json = format!("{{\"file\":\"{}\",\"bpm\":{bpm},\"sig\":\"{}\",\"label\":\"{}\",\"duration\":{len:.3},\"reads\":[", f[0], f.get(2).unwrap_or(&"0"), f.get(3).unwrap_or(&"").replace('"', "'"));
                let mut first = true;
                let mut end = 5.0f32;
                while end <= len.min(60.0) + 1e-3 {
                    let e = ((end * SR as f32) as usize).min(audio.len());
                    let mut kinds = if long_only.is_some() { vec![] } else { vec![("short", 10.0f32)] };
                    if (end.round() as u32) % 5 == 0 && end >= 10.0 {
                        kinds.push(("long", long_only.unwrap_or(30.0)));
                    }
                    for (kind, span) in kinds {
                        let s0 = e.saturating_sub((span * SR as f32) as usize);
                        let a = bt.analyze_audio(&audio[s0..e], SR).unwrap();
                        if !std::mem::take(&mut first) {
                            json.push(',');
                        }
                        write!(json, "{{\"kind\":\"{kind}\",\"end\":{end},\"len\":{:.3},\"beats\":{},\"downbeats\":{}", (e - s0) as f32 / SR as f32, arr(&a.beats), arr(&a.downbeats)).unwrap();
                        if long_only.is_none() {
                            write!(json, ",\"beat_logits\":{},\"downbeat_logits\":{}", arr(&a.beat_logits), arr(&a.downbeat_logits)).unwrap();
                        }
                        json.push('}');
                    }
                    end += 1.0;
                }
                json.push_str("]}");
                std::fs::write(&dst, json).unwrap();
                println!("{:5.1}s {name} (causal)", t0.elapsed().as_secs_f32());
                continue;
            }
            let start = (len * 0.25).max(5.0).min((len - STRETCH).max(0.0));
            let stretch = &audio[(start * SR as f32) as usize..((start + STRETCH) * SR as f32).min(audio.len() as f32) as usize];
            let mut json = format!("{{\"file\":\"{}\",\"bpm\":{},\"sig\":\"{}\",\"label\":\"{}\",\"start\":{start:.3},\"listens\":[", f[0], f[1], f.get(2).unwrap_or(&"0"), f.get(3).unwrap_or(&"").replace('"', "'"));
            let mut first = true;
            let mut add = |at: f32, x: &[f32], json: &mut String| {
                let a = bt.analyze_audio(x, SR).unwrap();
                if !std::mem::take(&mut first) {
                    json.push(',');
                }
                write!(json, "{{\"at\":{at},\"len\":{:.3},\"beats\":{},\"downbeats\":{},\"beat_logits\":{},\"downbeat_logits\":{}}}",
                    x.len() as f32 / SR as f32, arr(&a.beats), arr(&a.downbeats), arr(&a.beat_logits), arr(&a.downbeat_logits)).unwrap();
            };
            let mut at = 0.0;
            while at + WINDOW <= stretch.len() as f32 / SR as f32 + 1e-3 {
                let x = &stretch[(at * SR as f32) as usize..(((at + WINDOW) * SR as f32) as usize).min(stretch.len())];
                add(at, x, &mut json);
                at += HOP;
            }
            add(0.0, stretch, &mut json); // the whole stretch in one listen
            let mut on = Onsets::new(SR);
            let flux: Vec<f32> = stretch.iter().filter_map(|&s| on.push(s)).collect();
            // the hop is a whole number of samples, so the true rate is SR / hop, not FPS
            let fps = SR as f32 / (SR as f32 / tempo::FPS) as u32 as f32;
            write!(json, "],\"onset_fps\":{fps},\"onsets\":{}}}", arr(&flux)).unwrap();
            std::fs::write(&dst, json).unwrap();
            println!("{:5.1}s {name}", t0.elapsed().as_secs_f32());
        }
    }
}

/// x played `speed`× faster (linear interpolation; as eval.rs).
fn faster(x: &[f32], speed: f32) -> Vec<f32> {
    if speed == 1.0 {
        return x.to_vec();
    }
    (0..(x.len() as f32 / speed) as usize)
        .map(|i| {
            let p = i as f32 * speed;
            let (j, f) = (p as usize, p.fract());
            x[j] + (x.get(j + 1).copied().unwrap_or(x[j]) - x[j]) * f
        })
        .collect()
}
