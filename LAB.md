# Tempo lab

The working notebook for the tempo ear: what the problem is, what we believe,
what we tried, what the numbers said. Every experiment gets an entry, kept or
not. `eval/RESULTS.md` holds the headline scores.

> Written in the private drum-looper repo this was lifted from. The per-run logs
> (`history.tsv`, `live-history.tsv`), the Rust-vs-Python parity example and the
> download-script tests it mentions, and the commit ids it cites, stayed there.
> `lab.py` and `live.py` start fresh logs when run.

## The goal

Sam (2026-10-17): get the tempo locked and correct, with the beats ticking along.
The raw tempo matters most. Meter (3/4 vs 4/4) matters less. **Target: 99%
within 1 BPM on known tracks.** After that, test on fresh tracks to check we
didn't overfit, then use the same method for key.

## How we score (decided up front, so we can't move the goalposts)

- **Dev sets** (iterate freely): `groove-dev.tsv` (120 drummer takes, Groove MIDI
  Dataset, CC BY 4.0) and `macleod-dev.tsv` (120 songs, Kevin MacLeod, CC BY 3.0;
  90 with a drum kit, 30 without).
- **Holdout sets** (sealed): `groove-holdout.tsv` (80) and `macleod-holdout.tsv` (80).
  Every file belongs to one split forever, decided by a hash of its name. The
  holdout is scored **only at milestones** and is never used to choose anything.
  If a holdout score drives a change, that holdout is spent and we fetch a new one.
- **Protocols**, from what drumkit actually does:
  - **P1, one listen:** a single 10 s window. This is the cold start.
  - **P2, locked:** a 30 s stretch heard as drumkit hears it, a 10 s listen every
    2 s (11 reads), then the lock rule. This is what Sam sees after ~30 s, and it
    is the **primary protocol**.
  - **P3, one long listen:** the whole 30 s at once (more context per read).
- **Metrics:**
  - **`exact`**: within 1 BPM of the label.
  - **`exact_any`**: within 1 BPM after folding to the label's octave (×½, ×1 or
    ×2). The beats tick along, but at double or half speed. This is the
    **primary metric**: the knob fixes the octave in one turn, and labels
    themselves disagree on octave.
  - **`acc1` / `acc2`**: the MIR standard, within 4% (acc2 also allows ×⅓, ×½, ×2, ×3).
  - **`meter`**: beats per bar correct, where the label knows it.
- **Compound meters** (6/8, 9/8, 12/8) are felt in dotted quarters: the beat is
  the quarter-note label × 2/3.

## The problem, from first principles

A tempo is a period: the time between beats a listener would tap. Getting it
"within 1 BPM" means the period is right to about 0.8% (4 ms at 120 BPM). An
estimate can be wrong in four separate ways, and each has a different fix:

1. **Wrong level.** The right pulse family at the wrong level: ×2 or ×½ (the
   octave), or ×3/2 or ×2/3 (felt in threes vs twos). The model is precise but
   counts the wrong thing. This is a *choice* problem, and partly a labelling
   one, since people disagree.
2. **Imprecise.** The right level, but the period is off by more than 0.8%.
   Sources:
   - The frame grid: Beat This! marks beats on 20 ms frames, so one gap can be
     off by ±10 ms (±2% at 120).
   - Timing jitter: humans play and the model marks.
   - Too few beats in the window.

   Statistics says this should be tiny: a line through N beats with timing noise
   σ has period error ≈ σ·√(12/N³). With 20 beats and σ = 12 ms that is 0.45 ms,
   about 0.1 BPM. So if precision fails, something structural is breaking the
   line: a missed or extra beat throwing off the count, or the tempo drifting.
3. **No beat found.** Sparse or beatless audio (pads, rubato), or a window that
   falls in a break.
4. **The label is not the audio.** The composer's tempo can be a different
   level from the felt pulse, or the track can change tempo. This can't be
   fixed in the ear. We measure it (all ears agree with each other but not with
   the label) and report it. We never relabel to make the score go up.

The order matters. The baseline says most misses on songs are type 1 (octave).
Type 2 should be nearly free after the line fit, and the numbers will say
whether it is.

## Hypotheses

Each gets an experiment. Expected effect in brackets.

- **H1 (type 1):** Most `exact` misses are octave or ×3/2 picks; `exact_any`
  will be far higher than `exact`. *Test: classify every miss by its ratio.*
- **H2 (type 2):** Locking over many reads (P2) beats any single read, because
  read errors are partly independent. *Test: P2 vs P1. Try median, mode, and
  fitting one line through the beats of all reads.*
- **H3 (type 2):** The line fit can still be beaten: sub-frame beat times taken
  from the model's beat activation (a parabolic peak) instead of frame centres.
  *Test: refit with interpolated peaks.*
- **H4 (types 2 and 3):** Reading the period off the whole activation curve
  (autocorrelation or comb of the logits) is more robust than reading it off
  picked beats, because a missed or extra beat doesn't break it. *Test: comb on
  the logits.*
- **H5 (type 1):** The octave can be chosen better than "whatever the model
  marked": from the beat-strength ratio between candidate levels, the downbeat
  spacing, and a prior on tempo. *Test: a level-choice rule fitted on groove dev
  only, then checked on macleod dev.*
- **H6 (type 3):** Longer context (P3, 30 s) recovers windows where 10 s found
  nothing. *Test: P3 vs P1 on misses.*
- **H7 (type 4):** Some labels don't match the audio. *Test: list the tracks
  where every ear and every read agree with each other and not with the label.
  Report them, never fix them.*
- **H8 (ensemble):** The old onset tracker (precise when right) can refine the
  period once Beat This! has picked the level. *Test: refine within ±3% of the
  neural estimate.*

## Debate with Codex

Each round: the problem, the numbers, my proposal. Codex attacks it, and we
settle on the next experiments. Recorded below, newest last.

### Round 1 (2026-10-17): Codex attacks the plan

Codex (`codex exec`, read-only, given this file, the code, and E0 to E1):

1. **"The decomposition confuses symptoms with causes."** The real problem is
   27 song failures on `exact_any`, not octaves: 14 at ×1 but 1 to 3 BPM off,
   5 at other ratios, 2 at ×½, 1 at ×4/3, 5 that never lock. It also
   recounted my miss buckets, which were from `exact`, not `exact_any`. Missing
   causes: extra detections (E1 numbers every extra beat as a new beat),
   changing subdivision, window edges, stale locks.
2. **Ceiling of selection.** With an oracle picking the best of the existing
   reads: groove 120/120, songs 107/120 (108 with the 30 s read). "Choose better
   among today's reads" cannot reach 99%. New candidates are needed.
3. **Agreement is not truth.** Overlapping reads share 80% of their audio, so
   two agreeing reads are weak evidence. Median lock 92/120 vs drumkit's 93.
4. **The onset clock is off:** 22050 / floor(22050/200) = 200.45 fps, a −0.227%
   bias. (Found the same hour; fixed in dump.rs and audit.py.)
5. **H7:** agreement among my own estimators does not prove a label wrong. It
   needs independent, blinded annotation.
6. **Its approach:** jointly infer period, phase and beat assignment over
   *unique* audio time. Align overlapping reads instead of counting them twice.
   Period candidates from both ears, then a dynamic program that assigns
   detections to beat indices, allowing skipped beats and rejected extra
   detections, with a robust fit. Keep competing pulse families until the
   evidence separates them.
7. **Its experiments, in order, with kill criteria:**
   - ① the selection ceiling (done above);
   - ② onset refinement on the corrected clock (kill if net rescues ≤ 0 or groove < 118);
   - ③ robust beat assignment with extra-beat rejection;
   - ④ activation-curve candidates (kill as sufficient if oracle coverage < 119/120);
   - ⑤ causal evidence accumulation (kill if songs ≤ 93/120 or wrong-lock time rises).
8. **Statistics:** 80/80 on a holdout only supports ≥96.3% at 95% confidence.
   Claiming 99% takes about 300 error-free examples. **The holdout must grow to
   ~300 per domain**, and should include music not by MacLeod.
9. **Metric honesty:** `exact_any` folds the label's octave, so at label 120 a
   reading near 240 gets ±2 and one near 60 gets ±0.5. Publish next to it:
   - `exact`, how often the octave was corrected, and lock coverage;
   - wrong-lock time and tick-phase drift.

   Count abstentions as failures. (They already are: `none` is in the denominator.)

**What I take, and what I push back on:**

- **Taking:**
  - Points 1 to 4 and 8 to 9 in full.
  - Experiments ② to ⑤ in Codex's order.
  - A `drift30` metric: how many beats the ticks would drift over 30 s. That is
    the physical version of "the beats tick along"; under ¼ beat is the bar.
- **Pushing back on 5, partly.** The audit's two measurements really are
  independent: a neural net on a mel spectrogram, and a raw onset
  autocorrelation. They agree to 0.02 BPM *on whole numbers* (69.02, 84.00,
  119.99), and DAW songs sit on whole tempos. That is hard to get by chance.
  Codex is still right that it isn't proof, and that scoring on "undisputed"
  labels flatters the ear that did the disputing. So:
  - the **primary score uses raw labels**;
  - the audited score is reported beside it, clearly marked;
  - the disputed tracks go to Sam as blind A/B click tests (label click vs
    measured click over the song), which is the independent annotation Codex asks for.

## Experiments

Format: id, date, hypothesis, change, dev numbers (P2 `exact_any` / `exact`
per set), verdict, keep or drop.

### Bench and parity (2026-10-17)

- `eval/lab.py` scores ideas on cached model output (`tempo/examples/dump.rs`).
  The model always runs in Rust; Python only tries what to do with its output.
- **Parity check** (`tempo/examples/parity.rs`): the Rust `from_marks` against
  `lab.py linefit_seq` on all 2,821 cached dev reads. 2,819 agree to 0.01 BPM.
  The other 2 are junk reads above 230 BPM, where f32 and f64 round a half-beat
  differently. Re-run parity whenever a lab winner is ported.
- Every run is in `eval/history.tsv` (the version-0 runs, before the bench
  upgrade, are in `eval/history-v0.tsv`).

### E0: baselines (dev, P2 locked, model full)

| | groove `exact_any` | songs `exact_any` | songs `exact` |
|---|---|---|---|
| median gap (beat-this-rs `calculate_bpm`) | 65.0% | 50.8% | 38.3% |
| line fit, beats numbered from the first (neural.rs) | 97.5% | 74.2% | 56.7% |

The median gap is quantised by the model's 20 ms frames; the line fit is what
made the drummer takes work.

### E1: number each beat from the one before it. KEPT

The line fit numbered beats as round((t − t₀)/gap). Over a 30 s listen the
rounding error of `gap` piles up into a whole beat, which is why a single 30 s
read scored *worse* than a 10 s one. Numbering each beat from the previous
gap fixes it.

- groove: P2 98.3%, **P3 100%**
- songs: P2 77.5%, P3 75.8% (was 52.5%)

Ported to `neural.rs`.

### E2: the lock skips empty reads, as drumkit does. NO CHANGE

Codex noticed that the Python lock reset on a read with no beat; the live one
doesn't. Matching it changed nothing on dev.

### E3 (Codex ②): onset refinement on the corrected clock. KILLED

Kept the neural level, then re-measured the period on the 200 fps onset curve
(±3%). Songs gained 2 (79.2%), but groove fell to 96.7%, below the 118/120
kill line. The onset curve is precise, but it locks onto whatever repeats
loudest, which is not always the beat.

### E4: one line through every read's beats on one clock. KILLED

Merged all 11 reads' beats into one timeline (each moment once, within 70 ms)
and fitted one line. Groove fell to 85.8% and songs to 50.8%. Overlapping reads
don't all mark the same level, so the merge mixes 8ths in with quarters.
Lesson: **only pool evidence that agrees on the level.**

### E5: the reads pick the level, the long read sets the period. KEPT

The 10 s reads lock the level, and a 30 s read of the same stretch, folded to
that level, sets the period if it agrees within 3%.

- groove: **P2 100%, drift30 100%**
- songs: **80.8%** (88.6% on undisputed labels), drift30 72.5%

In drumkit this means: keep the 10 s reads for a fast lock, then re-listen to
the last 30 s every few seconds to tighten the period.

### Label audit (H7, `eval/audit.py`)

15 of 120 song labels are disputed by the fixed rule: two independent
measurements agree within 0.2 BPM on a whole tempo, and the label misses it.
Examples: Funky One, labelled 121, measures 119.99; Disco Lounge, labelled
109, measures 109.99. All 120 groove labels pass. incompetech's own catalogue
(`pieces.json`, 1,443 tracks) matches the Commons labels on 198 of 200, so it
is the same source and not a second opinion. Codex's objection stands, so
independent blind A/B listening is the resolution (`eval/ab/`, see below).

### Where songs fail after E5 (23 of 120)

| Cause | Count | Notes |
|---|---|---|
| disputed label at ×1 | 7 | dead on a whole number, 1 to 2 BPM off the label |
| no beat found | 5 | 4 beatless (classical, ambient, silent-film), 1 sting |
| other ratio | 6 | 5 without drums |
| 1 to 2 BPM off at ×½ | 3 | |
| ×4/3 | 1 | "We Wish You a Merry Christmas", a 3/4 waltz |

- Songs with a drum kit and an undisputed label: about 96%.
- What's left to solve: beatless or rubato music, the 3/4 level choice, and
  precision when there are no drums.

### Round 2 (2026-10-17): Codex on E5 and what it hides

Codex had E0 to E5, the audit and the miss list. Main points:

1. **E5 is scored after the fact on a stretch that skips the intro.** Live, a
   track starts cold, may open with no beat, may break or change tempo, and the
   30 s read can't help until ~30 s in.
   - The 3% gate is ±3.6 BPM at 120, far looser than the target.
   - `drift30` measures frequency only, not where the ticks land.
   - **Highest priority: replay causally from each track's true start, and
     score the emitted ticks.**
2. **Claim and denominator.**
   - The `drums` tag is a catalogue string match, not proof of an audible kit.
   - Preregister the live claim: "acquires a correct pulse within 30 s of audible
     pulse onset and keeps it 30 s; ±1 BPM after allowed ×½/×2; ticks within
     0.1 beat, slip ≤ 0.25 beat; no wrong lock or dropout."
   - Report raw and audited labels side by side; never delete disputed tracks.
3. **"No beat" is mostly "no lock":** 4 of the 5 produced estimates that never
   agreed. Loping Sting is 4.5 s long: an acquisition-duration case, reported
   separately but left in the legacy benchmark.
4. **A/B test design:** add "both", "neither" and "tempo changes"; take
   unaided taps first; mix in undisputed controls; a second listener would help.
5. **Mechanism for the stubborn songs:** a bounded multi-hypothesis
   period-and-phase tracker. It should:
   - generate candidates from beat logits and onset periodicity;
   - make beat assignment robust (skip and extra beats);
   - treat downbeats as soft accent evidence;
   - choose by predicting *newly arriving* audio, counting overlapping audio once;
   - reset on a tempo change.
6. **Holdout:**
   - a second source (Free-Stock-Music's Alex-Productions catalogue publishes
     per-track BPM; PremiumBeat too, but paid);
   - group by composition and version before splitting;
   - 300/300 error-free per source to claim ≥99% per source (zero-error bound
     is 0.05^(1/n));
   - add separate strata for no-drum and beatless music.
7. **Order:**
   - E6 independent annotation (needs Sam);
   - **E7 causal replay**;
   - E8 bounded candidate coverage (≥8 of 10 stubborn cases, or kill);
   - E9 the tracker (≥3 net rescues, groove stays 120/120, no new wrong locks);
   - E10 abstention, expiry and reacquisition, then one frozen holdout test.

**Taken:** all of it. Sam asked to keep experimenting rather than wait, so E6
(annotation) waits for his ears, and E7 to E9 go ahead.

### Phase truth for the drummer takes (Groove MIDI)

- `eval/fetch-groove-midi.py` pulls each take's MIDI (a few KB) and writes its
  beat grid from the tempo map.
- **Bug found and fixed:** the reader started with MIDI's default tempo (120)
  and appended the file's own. When both sit at tick 0, the sort put the
  default *last*, so every grid was 120 BPM. Only the one take actually at 120
  lined up, which is how it showed.
- After the fix, the model's beats sit **0.022 beat** (median) from the MIDI
  grid, and 1 take of 116 is over 0.1 beat. The grid is a sound phase reference.

### E7: live replay (`eval/live.py` over `cache/full-causal/`)

- `dump.rs --causal` re-hears each track from its start for 60 s: a 10 s read
  every 1 s (from 5 s), and a long read of the trailing ≤30 s every 5 s
  (from 10 s).
- `live.py` runs drumkit's TEMPO logic over that and scores:
  - `end_ok`: locked and correct at 60 s (the primary number);
  - `ever_ok`, `lock_s` and `good_s` (time to first lock and first correct lock);
  - `wrong%` (share of locked time on a wrong tempo);
  - `phase_ok` (groove: ticks within 0.1 beat median, none past 0.25, over the last 20 s).

### E8 (Codex): bounded candidate coverage on the stubborn songs. KILLED

**Setup.** The 12 songs E5 misses whose label is not disputed and which run 10 s
or longer. For each, up to 8 candidates:
- periodicity peaks of the beat activation (sigmoid of the logits, 50 fps);
- periodicity peaks of the onset curve (200.45 fps);
- each short read's tempo;
- each refined sub-frame and de-duplicated within 2%.

**Result:** oracle coverage **6/12**, below the 8/10 kill line. A tracker that
chooses among candidates (E9) can't fix this group from these features. Not built.

**What the 6 uncovered tracks show:**

| Track | Label | What every read says | Likely cause |
|---|---|---|---|
| Waunobe March | 102 | 112, every read and the long one | label |
| The Machine Thinks | 120 | 100 / 200 | label or feel |
| We Wish You a Merry Christmas | 118 | a steady 160, 3/4 | label or level |
| Achilles | 110 | | soundtrack, no drums |
| Mourning Song | 57 | | classical, no drums |
| Villainous Treachery | 153 | | silent-film, no drums, rubato |

Wherever the pulse is steady, the ear is self-consistent and the label
disagrees. The rest is free-tempo music without drums.

**Lesson:** the song gap isn't an algorithm gap. It is
- label truth (blind A/B, which needs Sam), and
- music with no steady pulse, which is outside the 99% claim (Codex R2.1), so
  abstaining there is right.

What's left to engineer is the live behaviour: cold start, intros, phase (E7).

### A second source: free-stock-music.com (sealed holdout)

`eval/fetch-fsm.py`:
- many composers, each track CC-licensed with a published BPM, MacLeod excluded;
- at most 8 tracks per artist;
- 150 tracks, all sealed.

Unknown whether the BPMs come from the composers or from software. If
software, they may share our errors, so a sample gets the A/B treatment before
any claim rests on it.

### E7 results: the live replay (2026-10-18)

All 240 dev tracks re-heard causally (`eval/live-history.tsv`).

**Scoring fix first.** Ticks after a drummer take ended were scored against
the last MIDI grid point, which produced errors of 1–7 beats. No truth exists
past the grid, so `phase_score` now skips those ticks. Applied before any
policy comparison below.

`live.py` also reports `aud` (end_ok on undisputed labels) and `drum_aud`
(undisputed drum tracks: the 99% target).

| Policy (clock v0, phase from the last beat) | Groove end_ok | wrong% | phase_ok | Songs end_ok | ever_ok | drum_aud |
|---|---|---|---|---|---|---|
| E7a drumkit today | 98.3% | 2.7% | 94.0% | 75.0% | 90.8% | 91.0% |
| E7b E5, 3% gate | 98.3% | 2.6% | 93.1% | 79.2% | 90.8% | 91.0% |
| E7c E5, 1% gate | 98.3% | 2.7% | 93.1% | 78.3% | 90.8% | 91.0% |

- Everything locks at 6 s (the second read).
- The two groove misses are short reads hijacking a correct lock late in the
  take: chacarera reads 120–123 at 48–59 s while every 30 s read says 118.0.
- Codex's tighter 1% gate changes nothing.

### E9: the long read is the authority. KEPT

**Hypothesis.** A 30 s read averages away a fill or a break; a 10 s read
doesn't. So short reads should acquire the lock, and once a long read
(≥15 s) agrees within 3%, the lock is anchored. From then on, short reads only
move the phase. The exception is a real tempo change: 5 short reads in a row
that agree with each other (1.5%) and all sit outside the gate drop the anchor.

**Result:**
- Groove end_ok 98.3 → 99.2%, wrong% 2.7 → 1.7.
- Songs end_ok 81.7%, drum_aud 91.0 → 93.6%.
- But phase_ok fell 94.0 → **82.8%**, and that exposed E10.
- Ever_ok on songs dropped 90.8 → 86.7%. All four regressions are songs
  without drums that drift in tempo (rubato, or a steady climb from 138 to
  152). The anchor lags them by the long read's 15 s. That's outside the
  steady-pulse claim, and it's accepted.

### E10: the clock kept a stale period (a bug, in drumkit too). FIXED

`heard_clock` (drumkit `main.rs`) and its replay twin kept the **old** period
whenever a new read was within 1%. The drift check that justified keeping it
was measured on a clock at the **new** period. So the ticks ran at whatever
tempo the first lock heard, for good:
- hip-hop 94: the clock stayed at 94.41 while reads said 94.00;
- error grew to 0.38 beat over 30 s.

Today's drumkit hid this, because its jittery short reads kept tripping
re-anchors. E9's steady tempo stopped those.

**Fix:**
- Measure drift on the running clock's own period.
- When in step, take the new period, pivoting on the current beat so no tick
  moves.
- When off by ≥40 ms, snap to the read's phase.
- Either way, keep the beat count. That also removes a second bug: every
  re-anchor used to jump the drop index, which fired an extra drop.

E9 + E10: phase_ok 82.8 → 92.2%.

### E11: where a read's phase comes from. KEPT (both)

The remaining phase misses were single reads that snapped the clock off the
beat for one second, after which the next read snapped it back.

- **E11a.** Take the phase from a line through the read's last 8 beats instead
  of the last beat alone. The beat at the window's edge has no future context
  and is the least reliable. Phase_ok 92.2 → 94.0%. A fit through all ~20
  beats did slightly worse (93.1%): tempo drift inside the window biases it.
- **E11b.** Hysteresis: re-phase only when two reads in a row agree on the new
  phase (within 40 ms), as the tempo lock already requires. Phase_ok
  92.2 → 95.7%.
- **Both: 96.6%.**

Remaining groove phase misses (4 of 116):
- rock_100 sits a constant 0.27 beat early on every read. The audio and the
  MIDI grid likely disagree (an offset), not the tracker.
- The others are half-beat flips in chacarera and jazz swing.

### E12: drop split beats before numbering them. KEPT

Hard-Boiled's long read holds two gaps of 0.83 + 0.17: Beat This! split one
beat in two. Sequential numbering (E1) counted the fragment as a beat, so every
later beat sat one index off and the slope compressed (126.0 read as 128.2).

**Fix.** Before fitting, drop any beat less than half a gap after the one
before it (`lab.dedup`).

| Bench | Before → after |
|---|---|
| Non-causal groove P1 | 97.0 → 98.0% |
| Non-causal songs P1 | 72.2 → 76.3% (~1,300 reads) |
| Non-causal songs P3 | 75.8 → 76.7% |
| Non-causal songs P2 | 80.8 → 79.2% (−2 tracks) |
| **Live, E9+E10+E11+E12: groove** | end_ok **100%**, wrong% 1.2, phase_ok 95.7% |
| **Live, E9+E10+E11+E12: songs** | end_ok 81.7%, ever_ok 89.2%, drum_aud 93.6% |

The P2 losses come from long reads that E12 moves *closer* to the truth (Blown
Away 185 → 173.5, against a label of 170). That puts them inside the 3% gate
while they're still wrong.

**E12b** (a long read counts only if ≥90% of its gaps are within 15% of
the median): no change at 0.8 or 0.9. KILLED.

### Labels: tempo changes by design, suspects, a better A/B (2026-10-18)

**Tempo changes by design.**
- `macleod-dev.varying.tsv` is rule-based: the composer's own incompetech
  description says the tempo moves.
- The keyword scan ran over dev and holdout before any holdout score. One hit:
  We Wish You a Merry Christmas, "Starts out slowly and accelerates as it
  builds". No single label can be right for it.
- It's excluded from `aud` and `drum_aud` in both benches.

**Suspects.**
- `macleod-dev.suspect.tsv` lists steady reads far from the label, which
  `audit.py`'s 1–3 BPM rule can't catch: Waunobe March (102 vs 112.0),
  Fireflies and Stardust (170 vs 140.0), Circus Tent (144 vs 142.0).
- Incompetech's `pieces.json` carries the same numbers as Commons, so it is no
  second opinion.
- They stay counted as misses until an ear decides.

**A/B rebuilt** (`eval/data/ab/`, 21 pairs; the answers were still blank):
- the 15 disputed tracks and the 3 suspects;
- 3 **controls** (labels every ear agrees with, against a decoy click 1.5% off);
- answers are A / B / both / neither / changes (Codex R2).
- If a decoy ever "stays with the music", a 2 BPM drift isn't audible in 30 s
  and none of the dispute answers count.

**Where the live target stands:** 73/77 undisputed drum songs (94.8%) and
120/120 groove takes. The four misses:
- the 3 suspects;
- Hard-Boiled, whose short reads jitter to 129–130 in the last 5 s.

### Parity, ties and the Rust port (2026-10-18)

Porting E11a and E12 into `from_marks` brought 20 Rust/Python mismatches out
of 2,821 reads. All were float ties on the model's 20 ms grid, which make exact
boundary values common.

Each one, and how it's now settled the same way in f32 and f64:

| Tie | Example | Rule now |
|---|---|---|
| Half-gap dedup | 0.18 vs 0.36/2 | decided 5 ms (a quarter frame) out; ties are **kept** |
| Gap filter | Rust `0.2..` included 0.2 s, Python `0.2 <` excluded it | 0.21 s, off the grid |
| Index rounding | `round(2.5)`: Python's banker's rounding gives 2, Rust gives 3; 1.5 in f64 is 1.4999996 in f32 | `floor(x + 0.501)` |

On ties, keep beat drop: keep is better on songs (live drum_aud 96.1% vs
93.5%, P2 80.0 vs 77.5%) and equal on groove.

**Parity is now exact:** 0 tempo and 0 phase mismatches across 2,821 reads
(`examples/parity.rs` prints bpm and since_ms).

**Ablation (Codex R3):** the 0.21 s gap filter alone changes nothing (songs P1
72.2 vs 72.1%). The whole E12 gain is the dedup.

### Round 3 (2026-10-18): Codex on E9–E12 and the port

Codex accepts E10 as a real bug and calls E9, E11 and E12 dev-selected
heuristics. Its points and what came of each:

**1. Overfitting risks.**
- **A biased long read inside the 3% gate becomes the authority.** Tested by
  biasing every long read +1.7%: E9 collapses (groove end_ok 5.8%, drum_aud
  6.5%). Fixed by **E9b** below.
- **E12 deletes greedily.** A stray mark before the true beat survives, and the
  true beat is dropped.
  - Synthetic test (`a_stray_mark_anywhere_barely_moves_the_tempo`): a stray at
    any tenth of a gap, in a 30 s read on the 20 ms grid, moves the tempo less
    than 0.5 BPM.
  - E12c (keep whichever mark is nearer the due beat): +1 groove phase track,
    −1 drum song. **KILLED**; the simpler rule stays.
- **E11's knobs** (8 beats, 2 confirmations, 40 ms) are not swept yet. Open.
- **The metrics overclaim** (end_ok is one instant; wrong% counted reads, not
  time). Added `held%` (share of time from 15 s to the end on a correct tempo).
  wrong% is now weighted by time.

**2. Phase truth window.**
- The grid runs to the catalogue duration + 1 s, past the performance.
  `fetch-groove-midi.py` now writes each take's first and last note-on time
  (`<take>.span`); phase is scored only inside that span.
- At half tempo, the scorer matched any grid beat, so a parity switch could
  pass. It now picks one parity for the whole stretch.

**3. Ties.** Numerically motivated, not fully principled. Kept as documented;
the comment now says 5 ms is a quarter frame. Known edge: `+0.001` also rounds
ratios in 0.499–0.5 up.

**4. The port was not equivalent.** Seven real bugs, all fixed:

| # | Bug | Fix |
|---|---|---|
| 1 | **Listener** took one queued chunk per loop, then analysed and stamped the read "now". After a 3.4 s long read, the next read's audio ended seconds before its stamp. Today's deployed build has a milder form. | drain everything queued before each analysis |
| 2 | A read already running when TEMPO reset could deliver a result from before the reset | reads are tagged with the resets the thread has applied; stale ones are dropped |
| 3 | `reads` froze while anchored, so after an escape the first read compared against acquisition history | refreshed while anchored |
| 4 | drumkit cleared `elsewhere` when a long read agreed; the replay didn't | removed (matches the replay) |
| 5 | A short and a long read arriving together: the long was gated against the old lock | the short lock is applied first, then the long |
| 6 | The "one" came from the raw downbeat mark, which is as edge-noisy as a raw beat and is what the clock anchors on | snapped to the fitted beat line (test `the_one_lands_on_the_beat_line`) |
| 7 | After a phase snap the drop count can run backwards (4 → 3 → 4) and fire twice | the heard clock ignores a one-bar step back |

Also: the replay's acquisition now uses drumkit's max/min agreement.

**5. Long-read cost.** Codex's provisional pick is full model, 30 s, every 15 s.
See E13.

**6. Next experiments.**
- (a) Replay timestamped reports through both Rust and Python.
- (b) Perturbation runs. Done so far: biased long reads, synthetic marks.
- (c) Deployment bake-off, blind labels, freeze, then holdouts.

### E9b: anchor only on close agreement with the short reads. KEPT

A long read anchors only within **1% of the median of the last 5 short reads**
(folded to the lock), not within 3% of the lock.

| | Groove end_ok | phase_ok | held | Drum songs (drum_aud) |
|---|---|---|---|---|
| E9b, real data | 100% | 96.6% | 100% | 96.1% (same as E9) |
| E9 under +1.7% long-read bias | 5.8% | – | – | 6.5% |
| E9b under +1.7% long-read bias | **94.2%** | – | – | **90.9%** |

A biased long read now simply never anchors, and the policy degrades to the
short-read lock.

### E13: what the long read costs, and what it can be

Measured with Beat This! in rten on the i9, with load ~9 because drumkit was
already busy:

| Read | Wall time | CPU time | 
|---|---|---|
| full, 10 s | 0.45 s | ~1.5 s |
| full, 30 s | **3.4 s** | ~14 s (attention grows faster than the window) |
| small, 20 s | 0.7 s | – |
| small, 30 s | 2.1 s | – |

- **E13a, cadence.** A long read every 15 s instead of 5 scores the same or
  better: groove phase 97.4%, drum_aud 96.1%. That's a third of the cost, so
  `LONG_EVERY` is now 15 s in `neural.rs`.
- **Silence.** The Listener no longer runs the model on silence (mean square
  < 1e-6, drumkit's quiet Mac).
  - Before this, TEMPO left on in a quiet room kept the full model running
    every second.
  - Found at 344% CPU on drumkit while Sam's TEMPO was on, hearing the room.
- Small-20 and full-20 long reads are being dumped
  (`dump --long <s>`, replayed with `LONG=<cache>`).

### E11 plateau check (Codex R3: sweep the knobs, look for a plateau, not a peak)

Groove dev, the production setup (long read every 15 s), phase_ok:

| beats in the phase fit \ tolerance | 25 ms | 40 ms | 60 ms |
|---|---|---|---|
| 6 | 96.6% | 95.7% | 91.4% |
| 8 | 96.6% | **97.4%** | 93.1% |
| 12 | 95.7% | 95.7% | 94.0% |

The plateau is 95.7–97.4% for 6–12 beats at ≤40 ms. Looser than 40 ms costs
3–6 points. held stays at 99.9% everywhere, so tempo is untouched. The
defaults (8, 40 ms) are inside the plateau. Not re-tuned: picking the best
cell would be fitting to dev.

### Rust port = the replay, proven (2026-10-18)

- The lock rules moved out of drumkit's `main.rs` into `tempo::Lock`
  (`src/lock.rs`, with tests), and drumkit calls it.
- `examples/replay.rs` runs the shipped `from_marks` + `Lock` + `retime` over
  the causal cache.
- Against `live.py e9_anchor`: **0 lock and 0 clock mismatches in 14,569
  timeline entries**.
- **Production timing** (E13b): no short reads for 3.4 s after each long one,
  which is what the Listener's single thread does. Costs nothing: groove
  120/120, phase 97.4%, held 99.9%; drum songs 74/77.

**E13c: small model, 20 s window, every 15 s.** Cheaper (0.7 s per read) but
worse on songs:

| | Full model, 30 s (shipped) | Small model, 20 s |
|---|---|---|
| Groove end_ok | 100% | 100% |
| Groove phase_ok | 97.4% | 96.6% |
| Drum songs (drum_aud) | **96.1%** | 93.5% (−2) |
| Songs end_ok | 84.0% | 78.2% |

Running it every 5 s doesn't recover the loss (92.2%). **KILLED**: the full
30 s read at 15 s stays. That's Codex's provisional pick, now measured.
Full-20 is pending.

**E13d: full model, 20 s window, every 15 s.** Measured at 2.6 s per read,
with the machine loaded by dumps; the 30 s read was 3.4 s under similar load,
so it saves little. Results:
- groove 120/120, phase 96.6% (−1 track), held 99.7%;
- **drum songs 94.8%** (−1 track against the 30 s read);
- songs end_ok 83.2%.

**KILLED.** E13 settles on the full model, 30 s, every 15 s.

### E14: the gains on a perturbed copy (Codex R3 6b: does it survive new tempos?)

`perturb-*.tsv` is every third dev track (40 groove, 40 MacLeod), played 6%
slower and 6% faster (`dump --speed`; the tempo and the pitch move, and the
labels and MIDI grid scale). Every tempo is one the bench has never seen.

| Speed | Build | Groove end_ok | Groove held | Groove phase | Drum songs | Songs held |
|---|---|---|---|---|---|---|
| ×0.94 | shipped | 97.5% | **97.5%** | 89.5% | 95.8% | **83.6%** |
| ×0.94 | drumkit 97e1f65 | 97.5% | 95.2% | 92.1% | 100% | 82.4% |
| ×1 | shipped | **100%** | **100%** | 94.7% | 100% | **79.8%** |
| ×1 | drumkit 97e1f65 | 97.5% | 97.9% | 94.7% | 100% | 78.8% |
| ×1.06 | shipped | **100%** | **100%** | **94.7%** | 100% | 73.7% |
| ×1.06 | drumkit 97e1f65 | 97.5% | 98.2% | 89.5% | 100% | 75.0% |

**What it says:**
- On drum takes the gains hold at every speed. End_ok and held are equal or
  better at all three speeds; phase goes −1 track at ×0.94 and +2 at ×1.06.
- The one groove miss at ×0.94 (chacarera read at 3:2, 166 for 110.9) fails
  identically in both builds. It's the tracker at that tempo, not the policy.
- The one drum-song loss at ×0.94 is Lachaim, a klezmer piece. Its long reads
  hold 139.2, then climb to 140.3 and 141.0 in the last 15 s: the music speeds
  up. The shipped build follows it and ends 1.2 BPM off the label; the old
  build ended on it by luck. The old build's time on a correct tempo for the
  songs is lower.
- The gains on the subset are smaller than on dev. Part of the dev margin is
  selection, as Codex warned. **No sign of overfitting that breaks
  anything:** the worst case is −1 track at one speed.

## Holdout protocol, frozen before unsealing (2026-10-18)

Written while the holdouts were being dumped (`dump --causal`; dumping reads no
scores) and before any holdout number existed. One run, logged, and nothing
changes after it. Any later change is a new configuration and needs new
holdout data.

**Configuration:** `live.py e9_anchor` with its defaults, which are what drumkit
ships, at commit fa3ec45:
- E12 estimator, clock v2, phase fit over 8 beats;
- E9b anchor;
- a long read every 15 s, 3.4 s busy after each.

**Sets** (sealed since they were fetched, never scored):
- `groove-holdout` (80 drummer takes);
- `macleod-holdout` (80);
- `fsm-holdout` (141 tracks, 57 composers; a second source).

**Metrics, per set:**
- end_ok (right at 60 s, within 1 BPM after ×½/×1/×2);
- held (share of time from 15 s on a correct tempo);
- phase_ok (groove only).

**Eligibility, decided now:**
- Groove: every take.
- MacLeod:
  - **primary** = drum-tagged tracks, **raw labels**. No audit exclusions,
    because `audit.py` uses our own measurements to doubt a label, and on a
    holdout that's self-serving.
  - Secondary, reported beside it: the same with `audit.py`'s rule applied.
  - The tempo-change keyword scan found no holdout track.
- fsm: no drum tags exist; every track, raw labels. Report by genre. Its BPMs
  may be software-made, so misses get the blind A/B before any claim rests on
  them.

**What counts as 99%.** n=80 means one miss is 98.75%, so a 99% claim on
groove-holdout needs **80/80**. Every number is reported with its Wilson 95%
interval. The claim is made only where the interval's lower bound and the
target can honestly be stated together; 80/80 gives a lower bound of 95.4%.
That's the honest size of what 80 tracks can show, and why Codex asked for
~300 per source.

### Holdout results: the one run (2026-10-18)

Run at 4b0f21e (`live-history.tsv`, "holdout: the frozen protocol"). live.py
had changed since fa3ec45 (Wilson intervals, two killed experiments, the /rev
simplify), so first both versions ran over the dev sets: **all 239 per-track
lines identical**, the summaries differing only by the Wilson column. Same
configuration.

| Set | end_ok | Wilson 95% | Also |
|---|---|---|---|
| groove-holdout | **80/80, 100%** | 95.4–100% | phase_ok 76/78 (91.1–99.3%); held 99.6% |
| macleod-holdout, drum-tagged, raw labels (primary) | 57/60, 95.0% | 86.3–98.3% | |
| macleod-holdout, drum-tagged, audit applied | 55/56, 98.2% | 90.6–99.7% | |
| macleod-holdout, every track | 71/80, 88.8% | 80.0–94.0% | held 88.7% |
| fsm-holdout (133 tracks, 53 artists) | 124/133, 93.2% | 87.6–96.4% | held 89.9% |

Against dev (same configuration): groove 120/120, phase 97.4%; MacLeod drum
songs 96.1% (audited), every track 84.0%. **The holdouts land where dev did:
no sign the dev work fitted the dev sets.**

**The 99% claim.** Drummer takes: 80/80 is consistent with 99%, and 95.4% is
the most 80 tracks can prove. Songs, on raw labels: not 99%. MacLeod's raw
interval (86.3–98.3%) excludes it. Of its 3 drum misses, the audit disputes 2
labels (Slow-Burn 84 → measured 84.99; Hotrock 124 → 127.99). The third is
Opportunity-Walks, read 154.97 against 156 (1.03 off).

**fsm misses, by kind** (reported, not re-scored; they get the blind A/B
before any claim rests on them):
- Meter: ×3 (a waltz, 66 → 198), ×1.5 (two blues shuffles), ×⅔ (swung
  hip-hop, 120 → 80). That's outside the ×½/×1/×2 the metric forgives. Blues
  is 6/9 because of it.
- 5–10% off: three "corporate" volumes, which look like bundles of several
  pieces (one label can't fit them all), and a Silent Night arrangement.
- 1.03 BPM off: Play Easy, 120.97 against 122.

By genre: electronic-pop 20/20, lo-fi-hip-hop 17/18, jazz 12/12, trap 12/12,
rock 10/11, disco 10/11, blues 6/9, pop 6/9, rnb-soul 8/8, dubstep 7/7, reggae
5/5, house 5/5, electronica 4/4, chill-hop 2/2.

**A data bug the breakdown found:** fsm-holdout.tsv had 141 lines but 133
tracks. fetch-fsm.py's tempo spread repeated indices when it found fewer
tracks than wanted, so 8 tracks were written twice and the 8 fastest found
were never reached (fixed test-first, 07cb7df). The repeats were exact (same
file, same label, all scored ok), so the run's per-track results stand; the
counts above are on the 133. The logged line still says 141.

### The 20 ms grid, measured (2026-10-18)

Beat This! marks beats on 20 ms frames (50 fps). Sam asked whether there's a
way around that. What the grid costs, and what was tried:

**Tempo** (`every_tempo_and_grid_offset_reads_within_a_bpm`):
- every tempo 60.0–200.0 in 0.1 BPM steps, at five offsets between frames,
  marked on the grid and fitted;
- the worst error from the grid alone is **0.44 BPM for a 10 s read** (at 187.6)
  and **0.14 BPM for a 30 s read** (at 176.5);
- the line fit (E0b/E1) is the way around it for tempo. It is why the long read
  matters most at fast tempos. No remaining miss is a resolution miss.

**Ticks:**
- After the 8-beat phase fit, the grid's ±10 ms per beat is ~2–3 ms.
- The typical tick error against the MIDI click is 0.034 beat (mean of
  per-take medians; median 0.029), about 15 ms at 120 BPM. So most of the tick
  error is not the grid; it's the tracker's own timing plus the drummer's
  looseness against the click.

**E15: sub-frame peaks from the logits. KILLED.**
- Each beat moves to the parabola peak through its logit frame and the two
  neighbours.
- Tempo: P1/P2/P3 unchanged within one read.
- Live: the same on groove (phase_ok 97.4%, drum_aud 96.1%); songs end_ok
  +1 track.
- Mean tick error 0.0339 → 0.0342 beat.
- Why: Beat This! trains with a shift-tolerant loss, so where its peak sits
  within a frame or two carries no timing.

**Display, for scale:** drumkit's LED frames are 33 ms and the widget's
redraw is 16.7 ms. The grid is finer than what lights can show; it matters
for audible clicks, drops and sync.

**Next, E16:** the drum hit itself. Snap each beat to the sharpest onset near
it in the audio (resolvable to ~5 ms), and score ticks against the drummer's
actual MIDI hits, not the click. Risk (E3's lesson): snapping to an
off-beat hi-hat.

### Where the ticks sit against the drummer, and E16 (2026-10-18)

New truth: `<take>.hits` (`fetch-groove-midi.py`), the drummer's actual hit on
each click beat. That's the loudest MIDI note within 0.15 beat (≤ 80 ms), the
earliest on a tie; beats nobody hit are skipped. Groove dev, last 20 s of the
live replay (shipped config):

| Measured | Result |
|---|---|
| The drummer vs the click | median \|hit − click\| per take 0.034 beat; on average **0.024 beat ahead** (drummers rush) |
| Our ticks vs the click | 0.034 beat (mean of per-take medians); centred (−0.0025 on average) |
| Our ticks vs the hits | 0.043 beat; systematically **+0.0215 beat late** (~11 ms) |
| Audio onset vs MIDI note (10 takes, 1 ms log-energy slope) | **+11 ms** median of medians (2–18 ms per take) |

**Reading.** The dataset's audio trails its MIDI by about the same ~11 ms by
which our ticks trail the MIDI hits. So against the audio, which is what a
listener hears, the ticks land on the drummer's hits on average. Their offset
per take follows the drummer's own lead or lag (correlation 0.44). What's
left is per-hit human timing, which a steady clock shouldn't chase. The 20 ms
grid is not the limit here either.

**E16: snap the phase-fit beats to the audio's steepest attack. KILLED.**
- The audio attack is found at 1 ms, within ±10/15/25 ms.
- phase_ok 96.6 / 95.7 / 96.6% vs 97.4% shipped.
- Mean tick error 0.0365 vs 0.0339 beat (median 0.0315 vs 0.0287).
- Snapping imports each hit's wobble, flams and the odd hat into the phase;
  the fit through the model's beats averages them out.

### /rev on the feature (2026-10-18): every fix test first

Sam: red-green TDD. The review fixes were backed out (1aa8f60) and redone one
at a time: the test first, watched failing for the reason named, then the fix.

| Finding | Red | Commit |
|---|---|---|
| A beat fitted just past the read (~8% of reads) panicked drumkit: a negative `Duration` | `a_beat_just_past_the_read_is_after_it` | 45a561a |
| Simplify: the clock and the TEMPO-off nudge spelled signed time out by hand | `ms_after_is_signed`, against a stub | 17bc4d1 |
| With TEMPO off, the listener re-read the frozen ring every second | `no_new_sound_no_read`, `a_long_read_needs_new_sound_too` | c078c49 |
| TEMPO during a mic take flushed the inputs: a hole in the take, or the take lost | `tempo_during_a_take_leaves_the_take_alone` | 1329565 |
| portal's tempo verb let inf, nan and absurd meters into the widget's beat maths | 4 Swift tests, 3 red | 9cb0ba3 |
| `fetch-models.sh` kept a download that failed its checksum (`shasum -c` in an `&&` list, which `set -e` ignores) | `test-fetch-models.sh` | 6325cb2 |
| `fetch-fsm.py` let a `../` slug through and kept tabs in TSV fields | `test_fetch_fsm.py` | 770d2a6 |
| `--check` skipped floors for sets the run didn't score: after the dev rename it checked one floor of six | `a_floor_for_a_set_not_scored_fails` | b5121b2 |

**Dropped: a long read held over a quiet Mac.** drumkit drains the long-read
slot only while the Mac isn't quiet. So a long read made in the first seconds
of a quiet, while its window still holds the old audio, lands when the sound
comes back. The fix would drain the slot on every pass. Not made, Sam's call:
- No test can reach it: it lives in drumkit's main loop.
- A long read always describes the trailing 30 s, held or not. E9b is the
  guard against old audio: a long read anchors only within 1% of the median of
  the last five short reads.
- What's left: after a gap of a few seconds between songs, those five short
  reads are still the old song's. The held read can then re-anchor the old
  tempo. If the lock was anchored there already, nothing changes. If not, the
  new song needs five short reads to take over instead of two (~3 s).

### Mutation check: the code written before its tests (2026-10-18)

Sam's pick after the TDD redo: break each rule of the earlier tempo code on
purpose and see what goes red. One mutant per rule, across the Lock, retime,
from_marks, the Listener's receiving side and drumkit's TEMPO wiring. The checks
run in order: the unit tests, then the parity and replay comparisons with
lab.py and live.py. Those two need the local cache and saved outputs, so they
aren't in the repo, and a break only they catch is a break the repo misses.

| | First run (56) | Final run (59) |
|---|---|---|
| Caught by the unit tests | 23 | **57** |
| Caught only by parity or replay | 18 | 0 |
| Caught by nothing | 15 | 2 |

What changed between the two runs:
- Every break the repo missed got a test, watched failing against its mutant:
  9 for the Lock, 4 for retime, 9 for from_marks, 2 for the listener's reset
  filter, 7 for drumkit's wiring.
- Seams, as pure moves (parity and replay unchanged): from_marks' dedup and
  numbering, and drumkit's drop rule, ear lock, tempo and clock step.
- Three behaviours changed, each test first:
  - A long read with no short reads to agree with doesn't anchor. The old
    fallback anchored within 3% of the lock, against E9b; no caller reaches
    it, so no read moved.
  - Numbering's forced step is gone. It can't fire on the model's 20 ms grid;
    off the grid it pushed every beat after a stray one off.
  - Without that step, numbering heals a split beat by itself, so switching
    dedup off went green. dedup got its own test.
- Three more D mutants (the one restarting the clock, the loop's octave fold,
  a loop's wrap) cover the moved wiring's other branches.

Left uncaught, on purpose:
- `pts.len() < 4` (a read needs four beats): loosening it changes no output on
  any data, parity and replay included. A floor, not a tuned value.
- from_marks' `conf`: computed, never read. Dead code, for the simplify round.

Not mutated: the listener thread itself (drain-all, the silence skip, its
reset count). It needs the model to run, so no unit test reaches it; the
replay covers what it feeds the Lock.

### /rev, Codex rounds 1–3 (2026-10-18)

Codex (read-only) on the TEMPO range, three rounds, each told what the last
settled. 18 findings: 14 fixed, 1 elegance change, 2 dismissed, 1 deferred.
Test first unless marked "by reading" (live CoreAudio streams or the main
loop's TEMPO toggle, which no test reaches yet).

| Round | Finding | Fix |
|---|---|---|
| 1 | PERFORM off paused the Mac tap TEMPO shares (P1) | `tap_on` = PERFORM or TEMPO, at every site |
| 1 | The mic's grid ear re-read a stale window every second | `MicEar`, paced on new sound by tempo's `Pace` |
| 1 | A rebuilt mic at a new rate mixed rates in the window | `MicEar` starts over on a new rate |
| 1 | The meter vote never decayed (an hour of 4/4 outvoted 3/4) | the last 16 reads' vote |
| 1 | TEMPO-on kept the old clock (widget, knob, loop re-time) | reset with the tempo (by reading) |
| 1 | The widget dropped tempos the knob makes (20–400 vs 7.5–1,150) | 1–2,000 |
| 1 | Elegance: a loop juggling an Option to apply two locks | two plain steps, one macro |
| 1 | Dismissed: the first read stamped after the model loads | the first read can't lock (two must agree) |
| 2 | A TEMPO-off nudge across the loop's end fired a burst | `into_cycle` before resync |
| 2 | A listener dead for want of a model was kept forever | `reset()` says whether it's alive; TEMPO-on respawns |
| 2 | The widget blinked forever after drumkit died | gone after 2 s without a message |
| 2 | A mic that died while TEMPO listened stayed deaf | retried every 2 s (by reading) |
| 2 | Dismissed again: a long read held over a quiet Mac | Sam's call, above |
| 2 | Deferred: one TEMPO-state struct with an `Ear` enum | Sam: its own PR, after this one |
| 3 | The nudge (round 2's fix) left mic clips marked from the cycle before | `clips_passed` resyncs them like hits |
| 3 | A mic rebuilt at the same rate spliced old audio into new | `Mic::ear()` = (rate, build) |
| 3 | A take with no live input held the mic forever | taking only when an input starts (by reading) |
| 3 | A failed `play()` in listen never marked the input dead | marks it dead (by reading) |

Round 3 still found 4, so the cap of three rounds ended it without the two
clean rounds in a row /rev needs. Sam: keep going until there are two.

### /rev, Codex round 4 (2026-10-18)

Codex found 3, all fixed, and the round's simplify pass found 1. All test
first, except where marked.

| Finding | Fix |
|---|---|
| The mic's beat sat under the newest hit, on the "and" whenever one ended the window | the half of the grid with more hits (E17, below) |
| A pad held through TEMPO-off's re-time recorded its hold where its hit used to be | `retime_holds`: the start moves with the loop, and the time held so far counts at the new tempo |
| A main-loop stall past the tap's ring (~2.7 s) left a gap inside the Mac listener's window | the callback marks the drop; main resets the listener instead of feeding it the batch (by reading) |
| The same gap in the mic's ring (~1.4 s), the sibling path | `Mic::ear()` carries the drop count, and `MicEar` starts over when it changes |
| Simplify: `Mic::listen`'s rate went unread | dropped, and `Mic::rate` with it |

- **The batch that shows the drop goes too.** rtrb drops the *newest* sample
  when full, so after a stall the ring holds the stall's first seconds and
  stops at the gap, and that batch straddles it. A rebuild now drops its
  first batch as well, about 16 ms of sound: one rule is simpler than two.
- **The drop is read before the drain.** A drop landing between the two then
  shows on the next pass, instead of never.
- **The mic's overrun count no longer resets.** It was zeroed at every take
  start. TEMPO now watches it for changes, so it only grows and a take
  reports its share.
- **Mutation check.** The new mutants (D9–D13, G1–G3) are each killed by the
  test written for them.

### E17: which half of the grid is the beat (2026-10-18)

grid_beat reads a mic take as 8ths, with the beat on every other grid step.
It took the newest hit's half, so a window ending on an "and" ticked on the
"and" (Codex round 4). Four rules, scored with `examples/grid_phase.rs`:

| Rule | Groove dev, on the click | Sea foam, on the beat | Groove holdout, on the click |
|---|---|---|---|
| the newest hit's half (before) | 66.4% | 3 of 9 | 69.2% |
| **the half with more hits (kept)** | **86.1%** | **9 of 9** | **89.0%** |
| the half with more energy | 78.8% | 9 of 9 | 87.5% |
| the half holding the loudest hit | 72.6% | 9 of 9 | 80.0% |

- **Groove dev** is 116 takes in 4/4 and 3/4. Each was cut into 10 s windows
  every 2 s and scored against the drummer's MIDI click, where the read's
  beat is the click's quarter (1,445 windows) or 8th (3,439).
  - A window is on when its newest beat line is within a quarter beat of a
    click line.
  - The windows overlap, so they aren't independent samples. The shares are
    a measure, not an interval.
- **Ties** (as many hits on each half) are 167 of 5,847 windows. Breaking
  them on energy instead of the newest hit changes nothing (86.1% both), so
  a tie keeps the newest's half, as before.
- **Sea foam** has no labels. Its log names the sounds (boom, boom, pap, a
  breath in; 8ths at 111), and the bar's shape says where it starts.
  - The loud hits (energy 30–58; the rest 0.5–14) fall two 8ths apart,
    three to a bar: beats 1, 2 and 3. The breath is on the "and" of 3.
  - Windows are read as the mic hears a fresh take: from its start, at 5 s,
    then every 0.5 s.
- **A first pass got sea foam backwards** (newest right, the rest wrong). It
  took the old test's "8ths 0, 3, 5, 7" as counted from the one, but they're
  counted from the breath. The test now says so.
- **Groove holdout, once.** It was scored once before, for the Mac's ear
  with that configuration frozen. The grid ear had never seen it, and
  nothing in E17 looked at it. It was scored once with the rule frozen at
  the dev result: 89.0%. Energy wins at ×1 there (83.5% vs 78.1%), but not
  overall.
- **Not measured:**
  - love bombed (one sound per beat, so it reads as half notes, where either
    half is a beat);
  - sea foam with the voice over it (the voice breaks the bar's shape);
  - Sam beatboxing live.
- **The tempo is unchanged.** Only where the ticks fall moved, so eval.rs's
  grid scores (BPM and meter) can't change.

### /rev, Codex round 5 (2026-10-18)

Codex found 2 and the review 1, all fixed.

| Finding | Fix |
|---|---|
| At ×½ the clock's bar is two of the song's, so a read's "one" can be either. Every other read sat half a clock bar off, and two in a row flipped the bar | `clock_step` moves the read's mark by whole read units onto the clock line nearest it before `retime` judges it |
| A take's start, end and abort emptied the mic's rings without TEMPO hearing them: a gap in its window at every take | one drain path feeds both the take and TEMPO, which collects it with `heard()` (by reading) |
| Review: `listen()`'s doc sat on `dead()`, with a line about a return value round 4 removed | moved and fixed |

- **The fold lives in drumkit, not in `tempo::retime`.** retime is the part
  proven equal to the lab, and the lab has no knob.
- **It runs only when the clock is slower than the ear** (its unit over 4/3
  of the read's). So ×1 and ×2 are unchanged.
  - Round 5 also said a loop that put the clock in half time was covered by
    the same rule. It wasn't (round 6).
- **Mutants C1–C5 are each killed.** C4 (folding the wrong way) only shows at
  ×¼, where a clock bar is four of the song's, so the test runs ×½ and ×¼.

### /rev, Codex round 6 (2026-10-18)

Codex found 1: round 5's own fix missed a case that round 5's note said it
covered.

| Finding | Fix |
|---|---|
| A loop in half time still flipped the clock's bar. The lock reports a read at the lock's octave (a 60 BPM loop over 120 BPM music reads 60), so the fold saw equal units while the read's marks came 500 ms apart | `clock_step` takes the read's own beat (`read_beat_ms`) beside the locked read, and folds by that |

- **The lock keeps a read's marks but gives it the lock's octave**
  (`Lock::short`, `Lock::long`). So the marks' spacing has to travel beside
  the read. tempo is untouched.
- **It's wider than loops.** It holds whenever the lock's octave is below
  what the ear hears. Marks sparser than the clock's lines always land on one.
- **Mutant C6** (folding by the lock's BPM) is killed by the new test.

### /rev, Codex round 7 (2026-10-18)

Codex found 2, both fixed. Both are about the Mac ear's timing.

| Finding | Fix |
|---|---|
| A read was stamped when the worker got to it, not when its sound came. If main stalls while the worker listens, the next read comes a stall late. Its long read, of the same sound, is as late, so retime's "off twice the same way" takes the false correction | each chunk carries the instant `hear` got it, and a read is stamped with the newest chunk's (test first) |
| When the tapped sound catches up (more than `PASS_MAX` queued, after a stall or drift), it skips ahead, and TEMPO's window joined the two sides | the skip raises the same gap flag as a full TEMPO ring, so the listener starts again after it (by reading: the audio callback) |

- **The worker's ear can now be swapped for a test.** `Listener::spawn_with`
  takes what the first listen loads. Beat This! in drumkit, a stand-in that
  takes 200 ms in the test. This is the worker's first test.
- **Mutants N5 to N7 are each killed:** stamping "now", never moving the
  stamp, and taking the first chunk of a batch rather than the newest.
- **It also settles one round 1 dismissed:** the first read was stamped after
  the model loaded, about a second late. Now it's stamped when its sound came.
- **Not flagged: the pass-through running dry.** It rebuffers about 21 ms of
  silence, under retime's 40 ms. A pause in the music is the music's own.
- **An output stall too short to trim is not a gap either.** The tapped sound
  plays on whole, only later, so the window still matches what you hear.

### /rev, Codex round 8 (2026-10-18)

Codex found 2, and the simplify pass 1. All fixed.

| Finding | Fix |
|---|---|
| Round 7's skip flag could be lost. The callback raises it before it pushes the sound after the skip, so main could take the flag, drain nothing, and skip the reset | a gap resets the listener whether or not anything was drained (by reading: the main loop) |
| A TEMPO read or the knob mid-take moved the metronome's beat, which the second tap had set for the take. The loop kept the heard beat, for its effects and for a palm restart's count-in | `effects_follow_tempo`: only with no loop yet (Free, Measuring). Both sites use it (test first; mutants E1, E2 killed) |
| Simplify: `spawn_with`'s ear needed no `'static` bound | dropped |

- **The race is narrow but real.** It is the same for a full TEMPO ring: the
  flag can land between two drains with no sound pushed in between.

### /rev, Codex round 9 (2026-10-18)

Codex found 2. One is dismissed on the replay's evidence, with its doc fixed;
the other is applied.

| Finding | Outcome |
|---|---|
| `nearest_octave` tries only ×½, ×1, ×2, so a read two octaves from the lock lands an octave off (240 against 60 is 120) | dismissed, by measurement: the full fold lost 102 of the dev replay's points on two takes and gained none. Its doc said "the octave closest", which it isn't, so the doc now says what it does and a test pins it |
| Elegance: `gen` was an `AtomicU64`, but only the main thread touches it (the worker counts its own resets) | `Cell<u64>` |

| Take (label) | Points right, one octave at most | Full fold |
|---|---|---|
| Groove rock 4/4 (50) | 63 of 67 | 0 |
| Longing and Concern (130) | 39 of 67 | 0 |
| three others | the same either way | |

- **Why one octave at most is right.** Both takes first locked on a wrong
  octave: 149 for the 50 BPM take, 32.5 for the 130 BPM song. Later the ear
  heard the true tempo.
  - One octave at most kept that read within an octave of what was heard:
    100 and 65, right by the eval's ×½ to ×2.
  - The full fold took it to the lock's octave, 200 and 32.5, and the lock
    stayed wrong.
- **The lab's `fold` is the same** (lab.py), so the lock still equals the lab.
- **Mutant O1** (the full fold) is killed by the pinning test. The replay
  check goes red on it too.
- **Codex's other example is accepted.** A 400 BPM loop over a 60 BPM ear is
  nearly three octaves apart, and settles an octave short. No loop is tapped
  at 8× the music's beat.

### /rev converged (2026-10-18)

Rounds 10 and 11 found nothing: two clean rounds in a row, so the loop ends.

| Round | Simplify | Review | Codex |
|---|---|---|---|
| 1 | 4 | 7 | 8 |
| 2 | 1 | 0 | 6 |
| 3 | 0 | 0 | 4 |
| 4 | 1 | 0 | 3 |
| 5 | 0 | 1 | 2 |
| 6 | 0 | 0 | 1 |
| 7 | 0 | 0 | 2 |
| 8 | 1 | 0 | 2 |
| 9 | 0 | 0 | 2 |
| 10 | 0 | 0 | 0 |
| 11 | 0 | 0 | 0 |

- **Codex raised 30.** 26 fixed, 3 dismissed (two in rounds 1–3, the
  multi-octave fold by measurement in round 9), 1 deferred to its own PR (the
  TEMPO-state struct).
- **The findings shrank round by round.** Early ones were feature-level (a
  shared tap, the meter vote). Late ones were the edges of timing: stalls,
  skips, and races between the audio callback and the main loop.
- **Twice the notes were wrong, and Codex caught it.** Round 5's note said a
  loop in half time was covered (round 6). The fold's doc said "the octave
  closest" (round 9).
- **Only the replay could settle round 9.** The code read as a bug, and the
  data said otherwise.
- **Gates at 989a9ac.** Workspace build; tempo 53, eval 2, drumkit 36; release
  build; fetch-models; fsm 5; swift 6. Unit, parity and replay checks equal
  the lab.
- **Skipped: a11y, UX, perf.** No web UI; the widget is native AppKit.
