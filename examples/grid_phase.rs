//! Which half of the grid is the beat, for the mic's ear (LAB.md E17; Codex, /rev round 4).
//!
//! grid_beat reads a take as 8ths: the beat is every other grid step. Which half?
//!   newest   the newest hit's line (grid_beat before E17)
//!   count    the half with more hits, the newest's on a tie (grid_beat since E17)
//!   loud     the half whose hits carry more energy in their first 50 ms
//!   peak     the half holding the loudest hit
//!   shipped  what tempo::grid_beat says
//! Scored two ways:
//!   cargo run --release --example grid_phase -- eval/groove-dev.tsv
//!     10 s windows of the Groove dev takes (4/4, 3/4) against the drummer's click.
//!     A window counts when its beat is the click's quarter or 8th (×1, ×2), and is
//!     on when its newest beat line is within a quarter beat of a click line.
//!   cargo run --release --example grid_phase -- eval/data/takes/sea-foam-1-beat.m4a 0,2,4,5 110.9
//!     a take whose bar of 8ths and tempo are known (sea foam: boom, boom, pap, breath
//!     on 0, 2, 4, 5). The bar's shape says where it starts, so which half is the beat.
//!     The windows are a fresh take's: from its start, read at 5 s, then every 0.5 s.
use std::path::Path;

const SR: u32 = 48_000; // a mic's rate
const WIN: f64 = 10.0; // tempo::neural::LISTEN_S, what the mic's ear holds
const RULES: [&str; 5] = ["newest", "count", "loud", "peak", "shipped"];

/// A window's grid: its step, each hit's step from the first, the newest hit's line
/// (s into the window) and the half (0 even, 1 odd steps) each rule takes for the beat.
struct Grid {
    step: f64,
    steps: Vec<usize>,
    line: f64,
    halves: [usize; 4],
}

impl Grid {
    fn of(w: &[f32]) -> Option<Grid> {
        let hits = tempo::grid::detect(w, SR, 0.5);
        let times: Vec<f64> = hits.iter().map(|h| h.start as f64 / SR as f64).collect();
        if times.len() < 4 {
            return None;
        }
        let step = tempo::grid::infer_step(&times);
        let q = tempo::grid::quantize(&times, step);
        let (mut count, mut energy, mut peak) = ([0usize; 2], [0f32; 2], [0f32; 2]);
        for (h, &(i, _)) in hits.iter().zip(&q) {
            let e = w[h.start..h.end.min(h.start + SR as usize / 20).min(w.len())].iter().map(|v| v * v).sum::<f32>();
            count[i % 2] += 1;
            energy[i % 2] += e;
            peak[i % 2] = peak[i % 2].max(e);
        }
        let (newest, drift) = *q.last()?;
        let n = newest % 2;
        let more = if count[0] == count[1] { n } else { (count[1] > count[0]) as usize };
        let halves = [n, more, (energy[1] > energy[0]) as usize, (peak[1] > peak[0]) as usize];
        Some(Grid { step, steps: q.iter().map(|&(i, _)| i).collect(), line: times.last()? - drift, halves })
    }

    /// The newest beat line (s into the window) if the beat is on `half`.
    fn beat_line(&self, half: usize) -> f64 {
        self.line - if self.halves[0] == half { 0.0 } else { self.step }
    }

    /// Each rule's newest beat line, then grid_beat's, s into the window.
    fn picks(&self, w: &[f32]) -> [f64; 5] {
        let shipped = tempo::grid_beat(w, SR).map_or(f64::NAN, |b| w.len() as f64 / SR as f64 - b.since_ms as f64 / 1000.0);
        let h = self.halves;
        [self.beat_line(h[0]), self.beat_line(h[1]), self.beat_line(h[2]), self.beat_line(h[3]), shipped]
    }
}

fn audio(path: &Path) -> Vec<f32> {
    beat_this::load_audio(path, SR).unwrap_or_else(|e| panic!("{}: {e}", path.display())).samples
}

fn pct(n: usize, of: usize) -> f64 {
    100.0 * n as f64 / of.max(1) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [list] if list.ends_with(".tsv") => groove(Path::new(list)),
        [take, bar, bpm] => known_bar(Path::new(take), &bar.split(',').map(|v| v.parse().unwrap()).collect::<Vec<usize>>(), bpm.parse().unwrap()),
        _ => panic!("usage: grid_phase eval/groove-dev.tsv | grid_phase <take> <8ths of its bar, e.g. 0,2,4,5> <bpm>"),
    }
}

fn groove(list: &Path) {
    let dir = list.parent().unwrap();
    let mut on = [[0usize; 5]; 2]; // [×1, ×2][rule]
    let (mut windows, mut read, mut level, mut ties) = (0, 0, [0usize; 2], 0);
    for row in std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = row.split('\t').collect();
        if f[2].ends_with("/8") {
            continue; // a compound meter's beat isn't the quarter the click marks
        }
        let file = dir.join(f[0]);
        let name = file.file_name().unwrap().to_string_lossy();
        let numbers = |ext: &str| -> Vec<f64> {
            let p = dir.join("data/groove-beats").join(format!("{name}.{ext}"));
            std::fs::read_to_string(p).unwrap_or_default().split_whitespace().filter_map(|v| v.parse().ok()).collect()
        };
        let (click, span) = (numbers("tsv"), numbers("span"));
        if click.len() < 2 || span.len() < 2 {
            continue;
        }
        let x = audio(&file);
        let quarter = 60.0 / f[1].parse::<f64>().unwrap();
        let eighths: Vec<f64> = click.windows(2).flat_map(|w| [w[0], (w[0] + w[1]) / 2.0]).chain(click.last().copied()).collect();
        let mut t0 = span[0];
        while t0 + WIN <= span[1].min(x.len() as f64 / SR as f64) {
            let w = &x[(t0 * SR as f64) as usize..((t0 + WIN) * SR as f64) as usize];
            let at = t0;
            t0 += 2.0;
            windows += 1;
            let Some(g) = Grid::of(w) else { continue };
            read += 1;
            let (lv, lines, unit) = match quarter / (2.0 * g.step) {
                r if (r - 1.0).abs() < 0.04 => (0, &click, quarter),
                r if (r - 2.0).abs() < 0.08 => (1, &eighths, quarter / 2.0),
                _ => continue,
            };
            level[lv] += 1;
            let n = g.steps.iter().filter(|&&i| i % 2 == 1).count();
            ties += (2 * n == g.steps.len()) as usize;
            for (k, l) in g.picks(w).iter().enumerate() {
                let d = lines.iter().map(|c| (at + l - c).abs()).fold(f64::INFINITY, f64::min) / unit;
                on[lv][k] += (d < 0.25) as usize;
            }
        }
    }
    println!("{windows} windows, {read} with 4+ hits; the beat is the click's quarter in {}, its 8th in {}; counts tie in {ties}", level[0], level[1]);
    for (k, rule) in RULES.iter().enumerate() {
        let (a, b) = (on[0][k], on[1][k]);
        println!("{rule:>8}: on the click {:5.1}% at ×1, {:5.1}% at ×2, {:5.1}% in all", pct(a, level[0]), pct(b, level[1]), pct(a + b, level[0] + level[1]));
    }
}

fn known_bar(take: &Path, bar: &[usize], bpm: f64) {
    let x = audio(take);
    let secs = x.len() as f64 / SR as f64;
    let (mut windows, mut fits, mut right) = (0, 0, [0usize; 5]);
    let mut end = 5.0;
    while end <= secs {
        let w = &x[((end - WIN).max(0.0) * SR as f64) as usize..(end * SR as f64) as usize];
        end += 0.5;
        windows += 1;
        let Some(g) = Grid::of(w).filter(|g| (60.0 / (2.0 * g.step) / bpm - 1.0).abs() < 0.04) else {
            continue; // no read, or not as its 8ths (16ths on 0, 4, 8, 10 fit a bar of 8ths too)
        };
        // where the bar starts: the offset that puts the most hits on it (8 8ths a bar)
        let (o, fit) = (0..8).map(|o| (o, g.steps.iter().filter(|&&i| bar.contains(&((i + o) % 8))).count())).max_by_key(|&(_, f)| f).unwrap();
        if fit * 10 < g.steps.len() * 8 {
            continue; // other sounds than the bar's (a voice over it)
        }
        fits += 1;
        let beat = g.beat_line(o % 2); // step i is on a beat when i + o is even
        for (k, l) in g.picks(w).iter().enumerate() {
            let d = ((l - beat) / (2.0 * g.step)).rem_euclid(1.0);
            right[k] += (d.min(1.0 - d) < 0.25) as usize;
        }
    }
    println!("{}: {secs:.1} s, {windows} windows, {fits} read as its bar at its tempo", take.display());
    for (k, rule) in RULES.iter().enumerate() {
        println!("{rule:>8}: the beat in {} of {fits}", right[k]);
    }
}
