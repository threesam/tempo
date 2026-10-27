#!/usr/bin/env python3
"""The beat grid under each Groove take, for tick-phase scoring (Codex R2): the
dataset's MIDI is the performance aligned to its click, so its tempo map gives
the true beat times. Fetches only the .mid files for takes already in
groove-*.tsv (range reads, a few KB each) and writes eval/data/groove-beats/<take>.tsv:
one beat time in seconds per line (quarter notes; the lab converts compound meters).

usage: eval/fetch-groove-midi.py
"""
import csv, io, os, sys, zipfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

HERE = os.path.dirname(os.path.abspath(__file__))
ns = {"__file__": os.path.join(HERE, "fetch-groove.py")}
src = open(os.path.join(HERE, "fetch-groove.py")).read().replace("\nmain()\n", "\n")
exec(compile(src, "fetch-groove.py", "exec"), ns)


def vlq(b, i):
    v = 0
    while True:
        c = b[i]
        i += 1
        v = (v << 7) | (c & 0x7F)
        if c < 0x80:
            return v, i


def parse(mid):
    """(ticks per quarter, tempo map [(tick, µs per quarter)], note-on ticks)."""
    assert mid[:4] == b"MThd"
    division = int.from_bytes(mid[12:14], "big")
    i, tempos, notes = 14, [(0, 500000)], []
    while i < len(mid):
        assert mid[i : i + 4] == b"MTrk", "bad chunk"
        n = int.from_bytes(mid[i + 4 : i + 8], "big")
        j, end, tick, status = i + 8, i + 8 + n, 0, 0
        while j < end:
            d, j = vlq(mid, j)
            tick += d
            c = mid[j]
            if c == 0xFF:
                kind, ln = mid[j + 1], None
                ln, k = vlq(mid, j + 2)
                if kind == 0x51:
                    tempos.append((tick, int.from_bytes(mid[k : k + 3], "big")))
                j = k + ln
            elif c in (0xF0, 0xF7):
                ln, k = vlq(mid, j + 1)
                j = k + ln
            else:
                if c & 0x80:
                    status, j = c, j + 1
                if status & 0xF0 == 0x90 and mid[j + 1] > 0:
                    notes.append((tick, mid[j], mid[j + 1]))  # tick, pitch, velocity
                j += 1 if (status & 0xF0) in (0xC0, 0xD0) else 2
        i = end
    # the MIDI default (120 BPM) applies only until the file's first tempo event;
    # a tempo at tick 0 replaces it (a sort would put the default after it — the
    # bug that made every grid 120 BPM, 2026-10-17)
    if any(t == 0 for t, _ in tempos[1:]):
        tempos = tempos[1:]
    tempos.sort(key=lambda x: x[0])
    return division, tempos, sorted(notes)


def seconds_at(tick, division, tempos):
    t, prev, us = 0.0, 0, tempos[0][1]
    for at, nxt in tempos[1:]:
        if at >= tick:
            break
        t += (at - prev) / division * us / 1e6
        prev, us = at, nxt
    return t + (tick - prev) / division * us / 1e6


def beat_times(mid, seconds):
    """Quarter-note beat times (s) from a type 0/1 MIDI file's tempo map, up to `seconds`."""
    division, tempos, _ = parse(mid)
    out, t, beat_tick = [], 0.0, 0
    # walk beat by beat through the tempo map
    k = 0
    while t <= seconds:
        out.append(t)
        while k + 1 < len(tempos) and tempos[k + 1][0] <= beat_tick:
            k += 1
        t += tempos[k][1] / 1e6  # one quarter at the current tempo
        beat_tick += division
    return out


def note_span(mid):
    """(first, last) note-on time (s): where the drummer actually plays (Codex R3:
    the grid runs to the catalogue duration + 1 s, past the performance)."""
    division, tempos, notes = parse(mid)
    return seconds_at(notes[0][0], division, tempos), seconds_at(notes[-1][0], division, tempos)


def played_hits(mid, beats):
    """The drummer's actual hit on each click beat (E16 truth): the loudest note
    within 0.15 beat (at most 80 ms) of it, the earliest on a tie. Beats with no
    hit near them are left out — that's where the ear hears no beat either."""
    division, tempos, notes = parse(mid)
    times = sorted((seconds_at(t, division, tempos), v) for t, _, v in notes)
    out = []
    for i, b in enumerate(beats):
        gap = (beats[i + 1] - b) if i + 1 < len(beats) else (b - beats[i - 1])
        w = min(0.15 * gap, 0.080)
        near = [(t, v) for t, v in times if abs(t - b) <= w]
        if near:
            out.append(max(near, key=lambda x: (x[1], -x[0]))[0])
    return out


def main():
    z = zipfile.ZipFile(io.BufferedReader(ns["Remote"](ns["URL"]), 1 << 20))
    rows = {r["audio_filename"].replace("/", "_").removesuffix(".wav") + ".flac": r
            for r in csv.DictReader(io.TextIOWrapper(z.open("groove/info.csv")))}
    out = os.path.join(HERE, "data", "groove-beats")
    os.makedirs(out, exist_ok=True)
    for tsv in ("groove-dev.tsv", "groove-holdout.tsv"):
        for line in open(os.path.join(HERE, tsv)):
            if line.startswith("#") or not line.strip():
                continue
            name = os.path.basename(line.split("\t")[0])
            dst, span, hits = (os.path.join(out, name + ext) for ext in (".tsv", ".span", ".hits"))
            if all(os.path.exists(x) for x in (dst, span, hits)):
                continue
            r = rows[name]
            mid = z.open("groove/" + r["midi_filename"]).read()
            if not os.path.exists(dst):
                beats = beat_times(mid, float(r["duration"]) + 1)
                open(dst, "w").write("\n".join(f"{b:.6f}" for b in beats) + "\n")
            a, b = note_span(mid)
            open(span, "w").write(f"{a:.6f}\t{b:.6f}\n")
            grid = [float(x) for x in open(dst)]
            h = played_hits(mid, grid)
            open(hits, "w").write("\n".join(f"{x:.6f}" for x in h) + "\n")
            print(f"notes {a:7.2f}–{b:7.2f} s, {len(h)} of {len(grid)} beats hit  {name}", flush=True)


main()
