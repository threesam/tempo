#!/usr/bin/env python3
"""The tempo lab bench: score ideas for "beats in, tempo out" on cached model
output (tempo/examples/dump.rs writes eval/cache/<model>/<file>.json), in
seconds instead of minutes. Winners get ported to tempo/src and confirmed by
the Rust eval (the source of truth).

  eval/lab.py <estimator> [--p2 lock|lock+long] [--sets groove-dev,macleod-dev]
              [--model full] [--log "E… what changed"] [--misses]

Protocols (tempo/LAB.md): P1 = each 10 s read scored alone, P2 = the 11 reads
of the 30 s stretch locked as drumkit locks them, P3 = the one 30 s read.
Every logged run records its git sha: a killed experiment's code (E2–E4 locks
and refiners, E12b/c, E15) lives at that sha, not here.
--log appends one line per protocol x set to eval/history.tsv.
--misses prints every P2 miss with its ratio to the label.

Metrics (tempo/LAB.md): exact_any (primary) · exact · drift30 (the ticks drift
under a quarter beat over 30 s, octave folded) · acc1 · acc2 · meter · none
(abstentions, counted as misses) · aud = exact_any on labels the audit
(eval/audit.py) did not dispute — reported beside the raw score, never instead.
"""
import argparse, collections, datetime, json, math, os, subprocess, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
FPS = 50.0  # Beat This! frames per second

# ---------------------------------------------------------------- truth


def truth(c):
    """(beat bpm, beats per bar or 0) from the label, compound meters in dotted quarters."""
    bpm, sig = float(c["bpm"]), c["sig"]
    num, den = (int(x) for x in sig.split("/")) if "/" in sig else (0, 4)
    if den == 8 and num % 3 == 0:
        return bpm * 2 / 3, num // 3
    return bpm, num


# ---------------------------------------------------------------- estimators
# Each takes one read (a dict from the cache) and returns (bpm, meter) or None.

ESTIMATORS = {}


def estimator(f):
    ESTIMATORS[f.__name__] = f
    return f


def meter_of(beats, downbeats):
    bars = collections.Counter()
    for a, b in zip(downbeats, downbeats[1:]):
        n = sum(1 for t in beats if a - 0.01 <= t < b - 0.01)
        if 2 <= n < 8:
            bars[n] += 1
    return bars.most_common(1)[0][0] if bars else 4


@estimator
def median_gap(r):
    """beat-this-rs calculate_bpm: 60 / median gap (frame-quantised)."""
    b = r["beats"]
    g = sorted(x for x in np.diff(b) if 0.2 < x < 2.0)
    if len(g) < 3:
        return None
    return 60 / g[len(g) // 2], meter_of(b, r["downbeats"])


def linefit_beats(b, gap):
    k = np.round((np.array(b) - b[0]) / gap)
    if np.ptp(k) == 0:
        return gap
    p = np.polyfit(k, b, 1)[0]
    return p if abs(p / gap - 1) < 0.1 else gap


@estimator
def linefit(r):
    """tempo/src/neural.rs today: line through all beats, indexed by the median gap."""
    b = r["beats"]
    g = sorted(x for x in np.diff(b) if 0.2 < x < 2.0)
    if len(g) < 3:
        return None
    return 60 / linefit_beats(b, g[len(g) // 2]), meter_of(b, r["downbeats"])


def seq_index(b, gap):
    """Number each beat from the one before it (a gap of ~2 = a missed beat), so a
    rounding error in `gap` can't pile up over a long listen."""
    k = [0]
    for d in np.diff(b):
        # an x.5 ratio is a tie (the 20 ms grid makes them common): up, decided a hair
        # past the half so f32 (Rust from_marks) and f64 agree
        k.append(k[-1] + max(1, math.floor(d / gap + 0.501)))
    return np.array(k, float)


@estimator
def linefit_seq(r):
    """E1: line fit with sequential beat numbering."""
    b = r["beats"]
    g = sorted(x for x in np.diff(b) if 0.2 < x < 2.0)
    if len(g) < 3:
        return None
    gap = g[len(g) // 2]
    p = np.polyfit(seq_index(b, gap), b, 1)[0]
    return 60 / p, meter_of(b, r["downbeats"])


def dedup(b, gap):
    """E12: a beat less than half a gap after the one before it is the same beat
    heard twice (Beat This! splits one, e.g. 0.83 + 0.17 of a gap); drop it, or
    sequential numbering counts it and every later beat lands one index off."""
    keep = [b[0]]
    for t in b[1:]:
        # beats sit on a 20 ms grid, so exact half-gap ties are common: they're kept,
        # decided a quarter frame (5 ms) out, not by float rounding (as Rust from_marks)
        if t - keep[-1] >= gap / 2 - 0.005:
            keep.append(t)
    return np.array(keep)


def med_gap(b):
    """The median gap between marks, 0.21–2 s. The marks sit on a 20 ms grid, so a
    gap of exactly 0.2 s is common: the bound sits off the grid, so f32 and f64
    agree on it (Rust from_marks)."""
    g = sorted(x for x in np.diff(b) if 0.21 <= x < 2.0)
    return g[len(g) // 2] if len(g) >= 3 else None


@estimator
def linefit_seq2(r):
    """E12: E1 after dropping split beats."""
    b = r["beats"]
    gap = med_gap(b)
    if gap is None:
        return None
    b = dedup(b, gap)
    if len(b) < 4:
        return None
    p = np.polyfit(seq_index(b, gap), b, 1)[0]
    return 60 / p, meter_of(b, r["downbeats"])


# ---------------------------------------------------------------- the lock


def fold(bpm, ref):
    return min((bpm / 2, bpm, bpm * 2), key=lambda x: abs(math.log(x / ref)))


def drumkit(reads):
    """drumkit today: locks when 2 reads in a row agree within 1.5%; octave sticks;
    the answer is the last locked read."""
    locked, prev = None, None
    for r in reads:
        if r is None:
            continue  # the live listener sends nothing for a read with no beat
        bpm = fold(r[0], locked[0]) if locked else r[0]
        if prev and abs(bpm / prev - 1) < 0.015:
            locked = (bpm, r[1])
        prev = bpm
    return locked


# ---------------------------------------------------------------- scoring


def score(pairs, disputed=frozenset()):
    """pairs: list of (estimate or None, (bpm, meter), name). Returns metric dict."""
    n = len(pairs)
    s = collections.Counter()
    for est, (t, tm), name in pairs:
        if name not in disputed:
            s["aud_n"] += 1
            s["aud"] += est is not None and any(abs(est[0] / k - t) <= 1 for k in (0.5, 1, 2))
        if est is None:
            s["none"] += 1
            continue
        b, m = est
        s["exact"] += abs(b - t) <= 1
        s["exact_any"] += any(abs(b / k - t) <= 1 for k in (0.5, 1, 2))
        s["drift30"] += min(30 * abs(b - k * t) / 60 for k in (0.5, 1, 2)) <= 0.25
        s["acc1"] += abs(b - t) <= 0.04 * t
        s["acc2"] += any(abs(b - t * k) <= 0.04 * t * k for k in (1 / 3, 0.5, 1, 2, 3))
        if tm:
            s["meter_n"] += 1
            s["meter"] += m == tm
    out = {k: 100 * s[k] / n for k in ("exact_any", "exact", "drift30", "acc1", "acc2")}
    out["aud"] = 100 * s["aud"] / s["aud_n"] if s["aud_n"] else float("nan")
    out["meter"] = 100 * s["meter"] / s["meter_n"] if s["meter_n"] else float("nan")
    out["none"] = s["none"]
    out["n"] = n
    return out


def ratio_class(b, t):
    if b is None:
        return "no beat"
    r = b / t
    for name, k in (("x1", 1), ("x2", 2), ("x1/2", 0.5), ("x3/2", 1.5), ("x2/3", 2 / 3), ("x3", 3), ("x1/3", 1 / 3), ("x3/4", 0.75), ("x4/3", 4 / 3)):
        if abs(r / k - 1) < 0.04:
            return name
    return "other"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("estimator", choices=ESTIMATORS)
    ap.add_argument("--p2", default="lock", choices=("lock", "lock+long"),
                    help="lock = drumkit's lock over the per-read estimates; "
                         "lock+long = the lock picks the level, the 30 s read sets the period if it agrees within 3% (E5)")
    ap.add_argument("--sets", default="groove-dev,macleod-dev")
    ap.add_argument("--model", default="full")
    ap.add_argument("--log")
    ap.add_argument("--misses", action="store_true")
    a = ap.parse_args()
    est = ESTIMATORS[a.estimator]
    rows = []
    for set_ in a.sets.split(","):
        if "holdout" in set_ and not os.environ.get("HOLDOUT_OK"):
            sys.exit("holdout is sealed: set HOLDOUT_OK=1 only at a milestone (tempo/LAB.md)")
        p1, p2, p3, misses, classes = [], [], [], [], collections.Counter()
        dpath = os.path.join(HERE, f"{set_}.disputed.tsv")
        disputed = {os.path.basename(l.split("\t")[0]) for l in open(dpath) if not l.startswith("#")} if os.path.exists(dpath) else set()
        vpath = os.path.join(HERE, f"{set_}.varying.tsv")  # the tempo changes by design: no one label is right
        disputed |= {os.path.basename(l.split("\t")[0]) for l in open(vpath) if not l.startswith("#")} if os.path.exists(vpath) else set()
        for line in open(os.path.join(HERE, f"{set_}.tsv")):
            if line.startswith("#") or not line.strip():
                continue
            name = os.path.basename(line.split("\t")[0])
            path = os.path.join(HERE, "cache", a.model, name + ".json")
            if not os.path.exists(path):
                sys.exit(f"missing cache {path}: run tempo/examples/dump.rs first (Codex R1: never drop files silently)")
            c = json.load(open(path))
            t = truth(c)
            reads = [est(r) for r in c["listens"][:-1]]  # each 10 s read
            p1 += [(r, t, name) for r in reads]
            locked = drumkit(reads)
            if a.p2 == "lock+long":
                long_ = est(c["listens"][-1])
                if locked and long_:
                    f = fold(long_[0], locked[0])
                    if abs(f / locked[0] - 1) < 0.03:
                        locked = (f, locked[1])
            p2.append((locked, t, name))
            p3.append((est(c["listens"][-1]), t, name))
            b = locked[0] if locked else None
            cls = ratio_class(b, t[0])
            if not (b and any(abs(b / k - t[0]) <= 1 for k in (0.5, 1, 2))):  # misses of the primary metric
                classes[cls] += 1
                misses.append((cls, t[0], b, name, c["label"]))
        rows.append((set_, {pr: score(v, disputed) for pr, v in (("P1", p1), ("P2", p2), ("P3", p3))}, classes, misses))

    sha = subprocess.run(["git", "-C", HERE, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    print(f"{a.estimator} + P2 {a.p2 if a.p2 != 'lock' else 'lock drumkit'} + refine none  (model {a.model}, git {sha})")
    print(f"{'set':<16}{'proto':<6}{'n':>5}{'exact_any':>11}{'exact':>8}{'drift30':>9}{'acc1':>7}{'acc2':>7}{'meter':>7}{'none':>6}{'aud':>8}")
    for set_, protos, classes, misses in rows:
        for pr, s in protos.items():
            print(f"{set_:<16}{pr:<6}{s['n']:>5}{s['exact_any']:>10.1f}%{s['exact']:>7.1f}%{s['drift30']:>8.1f}%{s['acc1']:>6.1f}%{s['acc2']:>6.1f}%{s['meter']:>6.1f}%{s['none']:>6}{s['aud']:>7.1f}%")
        print(f"{'':<16}P2 exact_any misses by ratio: " + ", ".join(f"{k} {v}" for k, v in classes.most_common()))
        if a.misses:
            for cls, t, b, name, label in sorted(misses):
                print(f"   {cls:<8} truth {t:6.1f}  got {b if b is None else round(b, 2)!s:>7}  {name}  ({label})")
    if a.log:
        with open(os.path.join(HERE, "history.tsv"), "a") as h:
            if h.tell() == 0:
                h.write("date\tgit\texperiment\testimator\tlock\trefine\tmodel\tset\tprotocol\tn\texact_any\texact\tdrift30\tacc1\tacc2\tmeter\tnone\taud\n")
            for set_, protos, _, _ in rows:
                for pr, s in protos.items():
                    # the lock and refine columns are kept for the older rows: always drumkit / lock+long, and none
                    h.write(f"{datetime.datetime.now():%Y-%m-%d %H:%M}\t{sha}\t{a.log}\t{a.estimator}\t{'drumkit' if a.p2 == 'lock' else a.p2}\tnone\t{a.model}\t{set_}\t{pr}\t{s['n']}\t"
                            f"{s['exact_any']:.1f}\t{s['exact']:.1f}\t{s['drift30']:.1f}\t{s['acc1']:.1f}\t{s['acc2']:.1f}\t{s['meter']:.1f}\t{s['none']}\t{s['aud']:.1f}\n")


if __name__ == "__main__":
    main()
