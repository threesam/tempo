# Tempo ears: scores

The newest results first. The working notes behind every number (hypotheses,
what was kept and killed, the Codex rounds) are in `tempo/LAB.md`. Every run is
logged in `history.tsv` (one read at a time, `lab.py`) and `live-history.tsv`
(the live replay, `live.py`).

## 2026-10-18: the mic's beat, which half of the grid (E17)

The mic's ear reads a take as 8ths, with the beat on every other line. That
line was the newest hit's, so a take ending on an "and" ticked on the "and".
Now it's the half with more hits (`examples/grid_phase.rs`):

| Set | Before | Now |
|---|---|---|
| Groove dev, on the drummer's click (4,884 windows) | 66.4% | **86.1%** |
| Groove holdout, scored once (3,089 windows) | 69.2% | **89.0%** |
| Sea foam, Sam's take, on its beat (9 windows) | 3 of 9 | **9 of 9** |

The tempo is unchanged; only where the ticks fall moved. The rules that lost
are in tempo/LAB.md, E17.

## 2026-10-18: the holdouts, scored once

Sealed until the configuration was frozen (tempo/LAB.md, holdout protocol),
then scored in one logged run.

| Set | Tracks | Right at the end | 95% interval |
|---|---|---|---|
| Groove drummer takes | 80 | **80 (100%)** | 95.4–100% |
| MacLeod drum songs, raw labels | 60 | 57 (95.0%) | 86.3–98.3% |
| MacLeod drum songs, audited labels | 56 | 55 (98.2%) | 90.6–99.7% |
| free-stock-music.com, every track (a second source) | 133 | 124 (93.2%) | 87.6–96.4% |

- The holdouts land where dev did, so the dev work didn't fit the dev sets.
- Drummer takes are consistent with 99%; 80 tracks can prove 95.4%.
- Songs are not at 99% on raw labels. The misses are mostly labels (2 of
  MacLeod's 3 are disputed by the audit) and meters the score doesn't forgive
  (×3, ×1.5, ×⅔ on a waltz, blues shuffles and swung hip-hop). They go to the
  blind A/B before any claim rests on them.

## 2026-10-18: live, as the looper hears it (dev sets)

`live.py` re-hears each track from its first second, the way TEMPO does:
- a 10 s read every second;
- a 30 s read every 15 s;
- nothing for 3.4 s while a long read runs.

The Rust code the looper ships replays identically (`examples/replay.rs`, 0
mismatches).

| Set | Tracks | Right at the end | Time on a correct tempo (from 15 s) | Ticks on the beat |
|---|---|---|---|---|
| Groove (drummer takes) | 120 | **120 (100%)** | 99.9% | 97.4% of 116 |
| MacLeod drum songs, labels nobody disputes | 77 | **74 (96.1%)** | – | – (no beat truth) |
| MacLeod, every song (incl. no drums, free tempo) | 119 | 100 (84.0%) | 79.9% | – |

- **Right** means within 1 BPM of the label, or of double or half it (the knob
  sets the octave).
- **Ticks on the beat** means the median tick is within 0.1 beat of the
  drummer's MIDI grid, and none strays past 0.25 beat, over the last 20 s.
- **The 3 drum-song misses** are all steady reads far from the label
  (Waunobe March: label 102, every read 112.0; Fireflies and Stardust: 170 vs
  140.0; Circus Tent: 144 vs 142.0). They're in the blind A/B (`data/ab/`)
  for Sam's ear, and count as misses until it's judged.
- **The holdouts** (groove 80, MacLeod 80, free-stock-music 133) were scored
  once with this configuration: see above.

How it got there (one line each; details in LAB.md):
- E1: number each beat from the one before it.
- E9 / E9b: the 30 s read is the authority, once it agrees within 1% with
  the short reads.
- E10: a clock bug fixed: it ran on a stale period, in the looper too.
- E11: phase from the last 8 beats' line, and re-phase only on two agreeing
  reads.
- E12: drop split beats before numbering.


## The 20 ms grid (2026-10-18)

Beat This! marks beats on 20 ms frames. What that costs:

- **Tempo.** A line through every beat reads the period between frames.
  - Swept tempos 60–200 at five frame offsets: worst error from the grid alone
    is **0.44 BPM on a 10 s read, 0.14 BPM on a 30 s read**, inside the 1 BPM
    target.
  - No remaining miss is a resolution miss.
- **Ticks.** Typical tick error against the drummer's click is 0.034 beat
  (~15 ms at 120 BPM), and the drummers themselves sit 0.034 beat off their
  click. Against the audio, our ticks land on the drummer's actual hits on
  average.
  - The dataset's audio trails its MIDI by ~11 ms, the same ~11 ms by which
    our ticks trail the MIDI hits.
- **Tried and killed:**
  - E15: interpolating between frames using the model's beat confidence. No
    gain, because the model is trained not to care about a frame or two.
  - E16: snapping beats to the audio's attacks at 1 ms. Worse, because it
    chases each hit's human wobble.
- **Scale:** the looper's LEDs redraw every 33 ms and the tempo widget every 17 ms.

## 2026-10-17: one read at a time (`examples/eval.rs`)

How close each ear gets to a known tempo. A listen is 10 s, three per file,
spread across it.

- **exact**: within 1 BPM
- **acc1**: within 4%
- **acc2**: within 4% of ×⅓, ×½, ×1, ×2 or ×3, so the right pulse even if the octave is off
- **meter**: beats per bar right

## 2026-10-17

| ear | set | listens | exact | acc1 | acc2 | meter | ms per listen |
|---|---|---|---|---|---|---|---|
| Beat This! full | groove (40 drummer takes) | 120 | 95% | 96% | 97% | 88% | 346 |
| Beat This! small | groove | 120 | 92% | 97% | 97% | 87% | 237 |
| first DSP tracker | groove | 120 | 55% | 64% | 85% | 72% | 5 |
| Beat This! full | macleod (40 mixed songs) | 120 | 63% | 72% | 88% | – | 346 |
| Beat This! small | macleod | 120 | 55% | 64% | 84% | – | 240 |
| first DSP tracker | macleod | 120 | 41% | 50% | 57% | – | 5 |
| grid | takes (4 of Sam's beatbox takes × 5 speeds) | 20 | 40% | 40% | 95% | – | 2 |
| Beat This! full | takes | 20 | 5% | 15% | 30% | 33% | 283 |

What it showed:

- Beat This! needed one fix to be precise: it marks beats on a 20 ms grid, so the
  median gap is quantised (90.9, 88.2…). A line fitted through every beat took the
  drummer set from 46% to 95% within 1 BPM.
- Its misses on songs are mostly the octave (108 for a "54", 176 for a "90"),
  which listeners argue over too. The TEMPO knob halves or doubles.
- 6/8 is felt in dotted quarters: Beat This! hears 46.9 for a "70" 6/8 take, and
  that's right. The eval counts compound meters that way.
- On a mouth, Beat This! (trained on recorded music) mostly hears no beat. the grid finder's
  grid finder hears the pulse in 95% of takes, so the mic listens with that.
- Listen length (full model): 5 s → 65% exact, 7 s → 74%, 10 s → 79%, 15 s → 78%.

## Test data

- `groove.tsv`: Groove MIDI Dataset (Google, CC BY 4.0), fetched by `fetch-groove.py`
- `macleod.tsv`: Kevin MacLeod, incompetech.com (CC BY 3.0), fetched by `fetch-macleod.py`
- `takes.tsv`: Sam's mouth-machine takes, copied in by hand (not in git)

Audio lives in `eval/data/` (gitignored). Models: `scripts/fetch-models.sh`.
