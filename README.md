# tempo

Hears the tempo, the beat and the bar of whatever is playing. Rust, MacOS-first, built for a drum looper and lifted out so other people can pressure test it.

It is **93% done, not 99%.** The numbers below are the honest ones.

## What it does

Three ears and a lock:

- **`neural`**: [Beat This!](https://github.com/CPJKU/beat_this) (Foscarin, Schlüter & Widmer, ISMIR 2024) through its pure-Rust port. Marks beats and downbeats, so it knows the bar and where the "one" is.
- **`dsp`**: onset flux plus autocorrelation. Cheap, runs every half second, first to guess.
- **`grid`**: for a fresh take of mouth sounds or beatbox, which Beat This! mostly hears no beat in. Finds the hits, then the grid of steps they sit on.
- **`lock`**: settles successive reads into one tempo and keeps the octave from jumping.

## Scores

Holdout sets, sealed until the configuration was frozen and scored once (details and the protocol in [`LAB.md`](LAB.md) and [`eval/RESULTS.md`](eval/RESULTS.md)):

| Set | Tracks | Right at the end | 95% interval |
|---|---|---|---|
| Groove drummer takes | 80 | 80 (100%) | 95.4–100% |
| MacLeod drum songs, raw labels | 60 | 57 (95.0%) | 86.3–98.3% |
| MacLeod drum songs, audited labels | 56 | 55 (98.2%) | 90.6–99.7% |
| free-stock-music.com, every track (a second source) | 133 | 124 (93.2%) | 87.6–96.4% |

"Right" means within 1 BPM of the label, or of double or half it. The goal was 99% within 1 BPM. Drummer takes are consistent with that, but 80 tracks can only prove 95.4%. Songs are not there on raw labels: the misses are mostly disputed labels and meters (waltzes, blues shuffles, swung hip-hop).

## Try it

```sh
scripts/fetch-models.sh                     # Beat This! models, sha256-pinned, ~93 MB
eval/fetch-all.sh                           # the eval audio (not in git)
cargo test
cargo run --release --example eval -- eval/groove-holdout.tsv
```

The eval lists are in `eval/*.tsv`; every scored run is logged in `eval/history.tsv`. Every experiment, kept or killed, is in [`LAB.md`](LAB.md).

## Credits

- Beat This!, MIT: Foscarin, Schlüter & Widmer, ISMIR 2024; the Rust port is [beat-this-rs](https://github.com/danigb/beat-this-rs) by danigb.
- Eval data is fetched, never redistributed here: the [Groove MIDI Dataset](https://magenta.tensorflow.org/datasets/groove) (CC BY 4.0), Kevin MacLeod's music at incompetech.com (CC BY 3.0), and Creative Commons tracks from free-stock-music.com.

## License

MIT. See [LICENSE](LICENSE).
