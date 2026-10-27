//! Parity: the Rust post-processing (neural::from_marks) against eval/lab.py on
//! the same cached reads. Prints one tempo per read; lab.py --parity compares.
//!
//!   cargo run --release --example parity -- eval/cache/full/*.json > rust.tsv

fn nums(json: &str, key: &str, from: usize) -> (Vec<f32>, usize) {
    let at = json[from..].find(&format!("\"{key}\":[")).unwrap() + from + key.len() + 4;
    let end = json[at..].find(']').unwrap() + at;
    let v = json[at..end].split(',').filter(|x| !x.is_empty()).map(|x| x.parse().unwrap()).collect();
    (v, end)
}

fn main() {
    for path in std::env::args().skip(1) {
        let json = std::fs::read_to_string(&path).unwrap();
        let name = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().to_string();
        let mut pos = 0;
        let mut i = 0;
        while let Some(off) = json[pos..].find("{\"at\":") {
            let start = pos + off;
            let len_at = json[start..].find("\"len\":").unwrap() + start + 6;
            let len: f32 = json[len_at..].split(|c| c == ',').next().unwrap().parse().unwrap();
            let (beats, e1) = nums(&json, "beats", start);
            let (downs, e2) = nums(&json, "downbeats", e1);
            let b = tempo::neural::from_marks(&beats, &downs, len);
            println!("{name}\t{i}\t{}", b.map_or("none".into(), |b| format!("{:.4}\t{:.2}", b.bpm, b.since_ms)));
            pos = e2;
            i += 1;
        }
    }
}
