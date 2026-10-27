//! The shipped TEMPO logic (neural::from_marks + Lock + retime) over the causal
//! cache, one line per read: what eval/live.py `e9_anchor` should print too
//! (Codex R3: prove the port, not just the Python).
//!
//!   cargo run --release --example replay -- eval/cache/full-causal/*.json > rust-live.tsv
//!
//! Output: file, read end (s), locked bpm or -, clock beat (s) or -, clock anchor (s) or -.

use tempo::{neural::from_marks, retime, Lock};

fn field<'a>(json: &'a str, key: &str, from: usize) -> (&'a str, usize) {
    let at = json[from..].find(&format!("\"{key}\":")).unwrap() + from + key.len() + 3;
    let end = at + json[at..].find([',', '}']).unwrap();
    (json[at..end].trim_matches('"'), end)
}

fn nums(json: &str, key: &str, from: usize) -> (Vec<f32>, usize) {
    let at = json[from..].find(&format!("\"{key}\":[")).unwrap() + from + key.len() + 4;
    let end = json[at..].find(']').unwrap() + at;
    (json[at..end].split(',').filter(|x| !x.is_empty()).map(|x| x.parse().unwrap()).collect(), end)
}

fn main() {
    for path in std::env::args().skip(1) {
        let json = std::fs::read_to_string(&path).unwrap();
        let name = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().to_string();
        let (mut lock, mut clock, mut pending) = (Lock::new(2), None::<(f64, f64)>, None);
        let mut pos = 0;
        while let Some(off) = json[pos..].find("{\"kind\":") {
            let start = pos + off;
            let (kind, e0) = field(&json, "kind", start);
            let (end, e1) = field(&json, "end", e0);
            let (len, e2) = field(&json, "len", e1);
            let (beats, e3) = nums(&json, "beats", e2);
            let (downs, e4) = nums(&json, "downbeats", e3);
            pos = e4;
            let (end, len): (f64, f32) = (end.parse().unwrap(), len.parse().unwrap());
            let got = from_marks(&beats, &downs, len).and_then(|b| match kind {
                "short" => lock.short(b),
                _ if len >= tempo::neural::LONG_MIN_S => lock.long(b),
                _ => None,
            });
            if let Some(b) = got {
                let (beat, at) = (60.0 / b.bpm as f64, end - b.since_ms as f64 / 1000.0);
                clock = Some(match clock {
                    None => (beat, at),
                    Some((old, anchor)) => {
                        let shift = retime(old * 1000.0, beat * 1000.0, (at - anchor) * 1000.0, &mut pending);
                        (beat, anchor + shift / 1000.0)
                    }
                });
            }
            let f = |x: Option<f64>| x.map_or("-".into(), |v| format!("{v:.4}"));
            println!("{name}\t{end}\t{}\t{}\t{}", f(lock.locked.map(|l| l as f64)), f(clock.map(|c| c.0)), f(clock.map(|c| c.1)));
        }
    }
}
