#!/usr/bin/env python3
"""Label audit (tempo/LAB.md H7): find labels the audio disagrees with, by a fixed
rule decided before looking at any score, and never by hand:

  disputed  = two independent measurements of the 30 s stretch —
                (a) Beat This! beats, line fit, one 30 s read
                (b) autocorrelation of the 200 fps onset curve near (a), sub-frame
              agree within 0.2 BPM, their mean is within 0.15 of a whole number
              (songs made in a DAW sit on whole tempos), and that whole number
              misses the label by 1 BPM or more at every octave.

Writes eval/<set>.disputed.tsv (file, label, measured). The lab reports scores
both on everything and on the undisputed rest; the labels themselves never change.

usage: eval/audit.py macleod-dev groove-dev …
"""
import json, os, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import lab  # noqa: E402


def onset_period(c, bpm):
    x = np.array(c["onsets"])
    x = np.maximum(x - np.convolve(x, np.ones(50) / 50, "same"), 0)
    # the onset hop is a whole number of samples: 22050 / 200 → 110, i.e. 200.45 fps
    # (the cache's "onset_fps" says 200; dump.rs now writes the true rate)
    fps, n = 22050 / int(22050 / c["onset_fps"]), len(x)
    L = 60 * fps / bpm
    best = None
    for k in range(1, 9):
        c0 = int(round(k * L))
        w = max(2, k)
        if c0 + w + 2 >= n // 2:
            break
        ac = {l: float(np.dot(x[l:], x[: n - l])) / (n - l) for l in range(c0 - w - 1, c0 + w + 2)}
        p = max(range(c0 - w, c0 + w + 1), key=ac.get)
        y0, y1, y2 = ac[p - 1], ac[p], ac[p + 1]
        d = y0 - 2 * y1 + y2
        best = (p + (0.5 * (y0 - y2) / d if d < 0 else 0)) / k
    return 60 * fps / best if best else None


def main():
    for set_ in sys.argv[1:]:
        out = []
        for line in open(os.path.join(HERE, f"{set_}.tsv")):
            if line.startswith("#") or not line.strip():
                continue
            name = os.path.basename(line.split("\t")[0])
            c = json.load(open(os.path.join(HERE, "cache", "full", name + ".json")))
            t, _ = lab.truth(c)
            a = lab.linefit_seq(c["listens"][-1])
            b = onset_period(c, a[0]) if a else None
            if not (a and b) or abs(a[0] - b) > 0.2:
                continue
            m = (a[0] + b) / 2
            whole = round(m)
            if abs(m - whole) <= 0.15 and all(abs(whole / k - t) >= 1 for k in (0.5, 1, 2)):
                out.append((line.split("\t")[0], t, m))
        with open(os.path.join(HERE, f"{set_}.disputed.tsv"), "w") as f:
            f.write("# file\tlabel\tmeasured  (eval/audit.py: two independent measurements agree on a whole tempo the label misses)\n")
            for o in out:
                f.write(f"{o[0]}\t{o[1]:g}\t{o[2]:.2f}\n")
        print(f"{set_}: {len(out)} disputed")
        for o in out:
            print(f"   label {o[1]:6.1f}  measured {o[2]:7.2f}  {os.path.basename(o[0])}")


if __name__ == "__main__":
    main()
