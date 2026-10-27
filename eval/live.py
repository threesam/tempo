#!/usr/bin/env python3
"""The live replay (Codex R2, E7): drumkit's TEMPO logic run over the causal cache
(tempo/examples/dump.rs --causal) from each track's true start, scored the way Sam
hears it. The defaults are what drumkit ships (LAB.md E13b); tempo/examples/replay.rs
runs the shipped Rust over the same cache and matches it exactly.

  eval/live.py <policy> [--sets groove-dev,macleod-dev] [--log "E… what changed"] [--show N]

Policies:
  e9_anchor    what drumkit ships (tempo::Lock)
  drumkit_now  the short-read lock alone; with EST=linefit_seq CLOCK=v0 PHASE=last
               LONG_EVERY=5 BUSY=0 it is drumkit 97e1f65, the baseline (E7a)

Per track, over its first 60 s:
  end_ok           locked and correct at the end — the primary number
  ever_ok, lock_s, good_s   whether and when it first locked, and first locked right
  wrong%           share of locked time on a wrong tempo
  held             share of time from 15 s on a correct tempo
  phase_ok         (groove) ticks within 0.1 beat of the MIDI click for their median,
                   none past 0.25 beat, over the last 20 s — "the beats tick along"
  aud, drum_aud    end_ok on undisputed labels, and on undisputed drum tracks
Correct = within 1 BPM of the label after folding ×½/×1/×2.

Knobs (environment). Every logged run records its git sha, so a killed
experiment's code lives at that sha, not here:
  EST=linefit_seq2|linefit_seq  the per-read estimator (lab.py)
  CLOCK=v2|v0                   tempo::retime, or 97e1f65's stale-period clock (E10)
  PHASE=fit8|last               a read's phase: the last-8 fit (E11a) or its raw last beat
  PHASE_N=8  TOL=0.040          the phase fit's beats; the clock's in-step tolerance (s)
  LONG_EVERY=15  BUSY=3.4       s between long reads; s of short reads each one costs (E13)
  LONG=<cache>                  the long reads from another cache (E13c/d)
  LONG_BIAS=<f>                 every long read's tempo × f (robustness, E9b)
  SPEED=<f> SUBSET=1            the dump --speed copy, and only its subset (E14)
"""
import argparse, datetime, json, math, os, subprocess, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import lab  # noqa: E402

EST = os.environ.get("EST", "linefit_seq2")
CLOCK = os.environ.get("CLOCK", "v2")
PHASE = os.environ.get("PHASE", "fit8")
PHASE_N = int(os.environ.get("PHASE_N", "8"))
TOL = float(os.environ.get("TOL", "0.040"))
LONG_EVERY = int(os.environ.get("LONG_EVERY", "15"))
BUSY = float(os.environ.get("BUSY", "3.4"))
LONG = os.environ.get("LONG")
LONG_BIAS = float(os.environ.get("LONG_BIAS", "0"))
SPEED = float(os.environ.get("SPEED", "1"))
SUBSET = os.environ.get("SUBSET")

POLICIES = {}
fold = lab.fold


def policy(f):
    POLICIES[f.__name__] = f
    return f


def read_est(r):
    """(bpm, meter, ms since the read's last beat as of its end) from one read, or None."""
    e = lab.ESTIMATORS[EST](r)
    if e is None or not r["beats"]:
        return None
    last = r["beats"][-1]
    gap = lab.med_gap(r["beats"])
    if PHASE == "fit8" and gap is not None:
        # the line through the last 8 beats (E11a): the window's edge beat has no
        # future to judge it by, and 8 rather than all because tempo drift inside
        # the window biases a long fit; split beats dropped and numbered as the
        # tempo fit numbers them
        b = lab.dedup(np.array(r["beats"]), gap)
        k = lab.seq_index(b, gap)[-PHASE_N:]
        if len(k) >= 4:
            slope, icpt = np.polyfit(k, b[-PHASE_N:], 1)
            last = icpt + slope * k[-1]
    return e[0], e[1], (r["len"] - last) * 1000


class Clock:
    """drumkit's heard_clock: a beat period and an anchor, kept in step with each read."""

    def __init__(self):
        self.beat = self.anchor = None  # s per beat; the time (s) of a beat
        self.pending = None  # drift (s) of the last read that said the clock is off

    def update(self, bpm, beat_at):
        beat = 60 / bpm
        if self.beat is None:
            self.beat, self.anchor = beat, beat_at
            return
        if CLOCK == "v0":
            # drumkit 97e1f65: keeps the OLD period whenever the read is within 1% and
            # lines up with a clock at the NEW period — the ticks ran on a stale period
            off = beat_at - self.anchor
            k = round(off / beat)
            if abs(self.beat / beat - 1) < 0.01 and abs(off - k * beat) < 0.040:
                return
            self.beat, self.anchor = beat, beat_at - k * beat
            return
        # tempo::retime (E10, E11b): which beat of the running clock this read's beat is,
        # and how far the running clock (its own period) has drifted from it
        k = round((beat_at - self.anchor) / self.beat)
        drift = beat_at - (self.anchor + k * self.beat)
        if abs(self.beat / beat - 1) < 0.01 and (abs(drift) < TOL or self.pending is None or abs(drift - self.pending) >= TOL):
            # in step, or off for the first time (more likely a bad read than a moved
            # beat): take the new period, pivoting on beat k so nothing jumps
            self.pending = None if abs(drift) < TOL else drift
            self.anchor += k * (self.beat - beat)
        else:
            self.anchor = beat_at - k * beat  # a new tempo, or off twice the same way: snap, keep the count
            self.pending = None
        self.beat = beat


@policy
def drumkit_now(reads, agree=0.015):
    """The short-read lock alone: two reads in a row within 1.5% lock it, once
    locked the octave sticks, long reads are ignored."""
    prev, locked, clock, timeline = None, None, Clock(), []
    for r in reads:
        e = read_est(r) if r["kind"] == "short" else None
        if e is not None:
            bpm = fold(e[0], locked) if locked else e[0]
            if prev is not None and max(bpm, prev) / min(bpm, prev) - 1 < agree:  # as drumkit
                locked = bpm
                clock.update(bpm, r["end"] - e[2] / 1000)
            prev = bpm
        timeline.append((r["end"], locked, (clock.beat, clock.anchor)))
    return timeline


@policy
def e9_anchor(reads, gate=0.03, need=5, agree=0.015, long_min=15):
    """What drumkit ships (tempo::Lock, LAB.md E9/E9b): short reads acquire the lock;
    a long read (>= 15 s) within 1% of the last five short reads' median anchors
    it; anchored, short reads within 3% only move the phase, and five in a row that
    agree with each other outside it are a real tempo change and re-lock there."""
    prev, locked, anchor, out, recent, clock, timeline = None, None, False, [], [], Clock(), []
    for r in reads:
        t, e = r["end"], read_est(r)
        if r["kind"] == "short" and e is not None:
            bpm = fold(e[0], locked) if locked else e[0]
            recent = (recent + [bpm])[-5:]
            if not anchor:
                if prev is not None and max(bpm, prev) / min(bpm, prev) - 1 < agree:  # as drumkit
                    locked = bpm
                    clock.update(bpm, t - e[2] / 1000)
            elif abs(bpm / locked - 1) < gate:
                out = []
                clock.update(locked, t - e[2] / 1000)  # phase only
            else:
                out = (out + [bpm])[-need:]
                if len(out) == need and max(out) / min(out) - 1 < agree:
                    locked, anchor, out = bpm, False, []
                    clock.update(bpm, t - e[2] / 1000)
            prev = bpm
        elif r["kind"] == "long" and locked and e is not None and r["len"] >= long_min:
            f = fold(e[0], locked)
            # E9b: within 1% of what the short reads say now, or a long read biased
            # inside the 3% gate would take over (Codex R3)
            ref = float(np.median([fold(x, locked) for x in recent])) if recent else locked
            if abs(f / ref - 1) < (0.01 if recent else gate):
                locked, anchor = f, True
                clock.update(f, t - e[2] / 1000)
        timeline.append((t, locked, (clock.beat, clock.anchor)))
    return timeline


def ok(bpm, label):
    return bpm is not None and any(abs(bpm / k - label) <= 1 for k in (0.5, 1, 2))


def phase_score(timeline, grid, label_beat, span=None):
    """Ticks emitted over the last 20 s vs the true grid at the emitted level.
    `span`: (first, last) note of the performance — no truth outside it (Codex R3)."""
    if not timeline or timeline[-1][2][0] is None:
        return None
    lo, hi = span if span else (grid[0], grid[-1])
    end = timeline[-1][0]
    ticks = []  # (tick time, emitted beat)
    for (t0, _, (beat, anchor)), nxt in zip(timeline, timeline[1:] + [(end + 1e-9, None, None)]):
        if beat is None or t0 < end - 20:
            continue
        # this clock state is in force from t0 until the next read
        tick = anchor + math.ceil((t0 - anchor) / beat) * beat
        while tick < nxt[0]:
            if lo <= tick <= hi:
                ticks.append((tick, beat))
            tick += beat
    if not ticks:
        return None
    level = np.median([b for _, b in ticks]) / label_beat
    if level < 0.75:  # double time: the beats and the midpoints between them
        grids = [np.sort(np.concatenate([grid, (grid[:-1] + grid[1:]) / 2]))]
        step = label_beat / 2
    elif level > 1.5:  # half time: every other beat, one parity for the whole stretch
        grids, step = [grid[0::2], grid[1::2]], label_beat * 2
    else:
        grids, step = [grid], label_beat
    best = None
    for g in grids:
        errs = np.abs([(t - g[np.argmin(np.abs(g - t))]) / step for t, _ in ticks])
        if best is None or np.median(errs) < np.median(best):
            best = errs
    return float(np.median(best)), float(best.max())


def wilson(k, n, z=1.96):
    """Wilson 95% interval (%) for k of n."""
    if not n:
        return float("nan"), float("nan")
    p, d = k / n, 1 + z * z / n
    c, h = (p + z * z / (2 * n)) / d, z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return 100 * (c - h), 100 * (c + h)


assert abs(wilson(80, 80)[0] - 95.4) < 0.1  # the holdout protocol's number


def spans(timeline):
    """(start, end, bpm) per timeline step; the last one lasts a second."""
    pts = [(t, b) for t, b, _ in timeline]
    return [(t0, t1, b) for (t0, b), (t1, _) in zip(pts, pts[1:] + [(pts[-1][0] + 1.0, None)])]


def held(timeline, label, start=15.0):
    """Share of time from `start` to the end spent locked on a correct tempo."""
    s = [(max(t0, start), t1, b) for t0, t1, b in spans(timeline) if t1 > max(t0, start)]
    tot = sum(t1 - t0 for t0, t1, _ in s)
    return sum((t1 - t0) * ok(b, label) for t0, t1, b in s) / tot if tot else None


def time_wrong(timeline, label):
    """Share of locked time on a wrong tempo, by time not by read (a long read at the
    same instant as a short one counts once)."""
    s = [(t1 - t0, b) for t0, t1, b in spans(timeline) if b is not None]
    lk = sum(d for d, _ in s)
    return sum(d for d, b in s if not ok(b, label)) / lk if lk else 0.0


def names(path):
    """The file names a label list (disputed, varying) holds."""
    return {os.path.basename(l.split("\t")[0]) for l in open(path) if not l.startswith("#")} if os.path.exists(path) else set()


def reads_of(c, name):
    """The track's reads, as the knobs say drumkit would get them."""
    reads = c["reads"]
    if LONG:  # E13: the long reads from another cache (a cheaper model or window)
        longs = {r["end"]: r for r in json.load(open(os.path.join(HERE, "cache", LONG, name + ".json")))["reads"]}
        reads = [longs.get(r["end"], r) if r["kind"] == "long" else r for r in reads]
    if LONG_BIAS:  # robustness: every long read's tempo off by this factor
        reads = [dict(r, beats=[x / LONG_BIAS for x in r["beats"]], len=r["len"] / LONG_BIAS) if r["kind"] == "long" else r for r in reads]
    if LONG_EVERY > 5:  # E13: fewer long reads
        reads = [r for r in reads if r["kind"] != "long" or round(r["end"]) % LONG_EVERY == 0]
    if BUSY:  # E13 (Codex R3): no short reads while a long one runs (~BUSY s after it)
        ends = [r["end"] for r in reads if r["kind"] == "long" and r["len"] >= 15]
        reads = [r for r in reads if r["kind"] == "long" or not any(0 < r["end"] - e <= BUSY for e in ends)]
    return reads


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("policy", choices=POLICIES)
    ap.add_argument("--sets", default="groove-dev,macleod-dev")
    ap.add_argument("--model", default="full")
    ap.add_argument("--log")
    ap.add_argument("--show", type=int, default=0, help="print the N worst tracks per set")
    ap.add_argument("--partial", action="store_true", help="skip tracks not dumped yet (smoke tests only; never for a logged score)")
    a = ap.parse_args()
    if a.partial and a.log:
        sys.exit("--partial runs are never logged")
    pol = POLICIES[a.policy]
    out = []
    for set_ in a.sets.split(","):
        if "holdout" in set_ and not os.environ.get("HOLDOUT_OK"):
            sys.exit("holdout is sealed (tempo/LAB.md)")
        base = set_.replace("perturb-", "") + "-dev" if set_.startswith("perturb-") else set_  # its labels' lists
        # disputed labels, and tracks whose tempo changes by design (no one label is right)
        disputed = names(os.path.join(HERE, f"{base}.disputed.tsv")) | names(os.path.join(HERE, f"{base}.varying.tsv"))
        rows = []
        for line in open(os.path.join(HERE, f"{set_}.tsv")):
            if line.startswith("#") or not line.strip():
                continue
            cols = line.rstrip("\n").split("\t")
            name = os.path.basename(cols[0])
            # groove takes are all drums; MacLeod tags its drum tracks in the genre column
            drums = base.startswith("groove") or (len(cols) > 3 and cols[3].endswith("/ drums"))
            path = os.path.join(HERE, "cache", f"{a.model}-causal" + (f"-x{SPEED:g}" if SPEED != 1 else ""), name + ".json")
            if not os.path.exists(path):
                if a.partial or (SPEED != 1 and SUBSET):
                    continue
                sys.exit(f"missing {path}: run dump.rs --causal")
            c = json.load(open(path))
            label, _ = lab.truth(c)
            tl = pol(reads_of(c, name))
            if not tl:
                print(f"   (no reads: {name}, {c.get('duration', 0):.1f} s)")
                continue
            locked_ts = [t for t, b, _ in tl if b is not None]
            good_ts = [t for t, b, _ in tl if ok(b, label)]
            ph = None
            gpath = os.path.join(HERE, "data", "groove-beats", name + ".tsv")
            if base.startswith("groove") and os.path.exists(gpath) and "/8" not in c["sig"]:
                grid = np.array([float(x) for x in open(gpath)]) / SPEED  # a faster copy: the grid comes sooner
                spath = gpath[:-4] + ".span"
                span = tuple(float(x) / SPEED for x in open(spath).read().split()) if os.path.exists(spath) else None
                ph = phase_score(tl, grid, 60 / float(c["bpm"]), span)
            rows.append(dict(name=name, label=label, lock=locked_ts[0] if locked_ts else None,
                             good=good_ts[0] if good_ts else None, wrong=time_wrong(tl, label), held=held(tl, label),
                             end=tl[-1][1], end_ok=ok(tl[-1][1], label), phase=ph, aud=name not in disputed, drums=drums))
        n = len(rows)
        if not n:
            continue
        med = lambda v: float(np.median(v)) if v else float("nan")
        pct = lambda sub: 100 * sum(r["end_ok"] for r in sub) / len(sub) if sub else float("nan")
        ph = [r["phase"] for r in rows if r["phase"] is not None]
        out.append(dict(set=set_, n=n,
                        end_ok=pct(rows),
                        # the honest size of what n tracks can show (LAB.md holdout protocol)
                        ci=wilson(sum(r["end_ok"] for r in rows), n),
                        # end_ok on undisputed labels, and on undisputed drum tracks (the 99% target)
                        aud=pct([r for r in rows if r["aud"]]),
                        drum_aud=pct([r for r in rows if r["aud"] and r["drums"]]),
                        ever_good=100 * sum(r["good"] is not None for r in rows) / n,
                        lock_s=med([r["lock"] for r in rows if r["lock"] is not None]),
                        good_s=med([r["good"] for r in rows if r["good"] is not None]),
                        wrong=100 * float(np.mean([r["wrong"] for r in rows])),
                        held=100 * float(np.mean([r["held"] for r in rows if r["held"] is not None])),
                        phase_ok=100 * sum(1 for p in ph if p[0] <= 0.1 and p[1] <= 0.25) / len(ph) if ph else float("nan"),
                        phase_n=len(ph)))
        if a.show:
            for r in sorted(rows, key=lambda r: (r["end_ok"], -r["wrong"]))[: a.show]:
                print(f"   end {'ok ' if r['end_ok'] else 'BAD'} label {r['label']:6.1f} end {r['end'] and round(r['end'], 2)!s:>7} "
                      f"lock {r['lock']} good {r['good']} wrong {r['wrong']:.0%} phase {r['phase'] and tuple(round(x, 2) for x in r['phase'])}  {r['name']}")
    sha = subprocess.run(["git", "-C", HERE, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    print(f"live replay: {a.policy}  (model {a.model}, git {sha})")
    print(f"{'set':<16}{'n':>5}{'end_ok':>9}{'ever_ok':>9}{'lock_s':>8}{'good_s':>8}{'wrong%':>8}{'phase_ok':>10}{'aud':>8}{'drum_aud':>10}{'held':>7}")
    for s in out:
        print(f"{s['set']:<16}{s['n']:>5}{s['end_ok']:>8.1f}%{s['ever_good']:>8.1f}%{s['lock_s']:>8.1f}{s['good_s']:>8.1f}{s['wrong']:>7.1f}%{s['phase_ok']:>8.1f}% ({s['phase_n']}){s['aud']:>7.1f}%{s['drum_aud']:>9.1f}%{s['held']:>6.1f}%   end_ok 95% CI {s['ci'][0]:.1f}–{s['ci'][1]:.1f}%")
    if a.log:
        with open(os.path.join(HERE, "live-history.tsv"), "a") as h:
            if h.tell() == 0:
                h.write("date\tgit\texperiment\tpolicy\tmodel\tset\tn\tend_ok\tever_ok\tlock_s\tgood_s\twrong\tphase_ok\tphase_n\taud\tdrum_aud\theld\n")
            for s in out:
                h.write(f"{datetime.datetime.now():%Y-%m-%d %H:%M}\t{sha}\t{a.log}\t{a.policy}\t{a.model}\t{s['set']}\t{s['n']}\t"
                        f"{s['end_ok']:.1f}\t{s['ever_good']:.1f}\t{s['lock_s']:.1f}\t{s['good_s']:.1f}\t{s['wrong']:.1f}\t{s['phase_ok']:.1f}\t{s['phase_n']}\t{s['aud']:.1f}\t{s['drum_aud']:.1f}\t{s['held']:.1f}\n")


if __name__ == "__main__":
    main()
