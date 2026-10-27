#!/usr/bin/env python3
"""Fetch takes from Google's Groove MIDI Dataset (CC BY 4.0,
https://magenta.tensorflow.org/datasets/groove) without downloading the 5 GB zip:
the zip is read over HTTP range requests and only the chosen WAVs come down,
stored as mono 22.05 kHz FLAC (what the ears hear; ~7x smaller).

Each take belongs to one split forever, by a hash of its name: `holdout` is
sealed (scored only at milestones, never tuned on), `dev` is for iterating.
Writes eval/data/groove/*.flac and eval/groove-<split>.tsv (file, bpm in
quarter notes, time signature, style). Picks full grooves (not fills) of 20 s or
more, every non-4/4 take in the split, then a tempo spread.

usage: eval/fetch-groove.py dev 120 | holdout 80
"""
import csv, hashlib, io, os, subprocess, sys, tempfile, urllib.request, zipfile

URL = "http://storage.googleapis.com/magentadata/datasets/groove/groove-v1.0.0.zip"
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "data", "groove")


def split_of(name):
    return "holdout" if int(hashlib.sha1(name.encode()).hexdigest(), 16) % 10 < 4 else "dev"


class Remote(io.RawIOBase):
    """A seekable view of a remote file, fetched in 1 MB blocks."""

    def __init__(self, url):
        self.url, self.pos, self.cache = url, 0, {}
        req = urllib.request.Request(url, method="HEAD")
        self.size = int(urllib.request.urlopen(req).headers["Content-Length"])

    def seekable(self):
        return True

    def readable(self):
        return True

    def seek(self, off, whence=0):
        self.pos = [off, self.pos + off, self.size + off][whence]
        return self.pos

    def tell(self):
        return self.pos

    def block(self, i):
        if i not in self.cache:
            lo, hi = i << 20, min(((i + 1) << 20) - 1, self.size - 1)
            req = urllib.request.Request(self.url, headers={"Range": f"bytes={lo}-{hi}"})
            self.cache = {i: urllib.request.urlopen(req).read()}  # one block: files stream in order
        return self.cache[i]

    def readinto(self, b):
        n = min(len(b), self.size - self.pos)
        got = 0
        while got < n:
            i, o = divmod(self.pos + got, 1 << 20)
            chunk = self.block(i)[o : o + n - got]
            if not chunk:
                raise OSError(f"empty range response at byte {self.pos + got}")
            b[got : got + len(chunk)] = chunk
            got += len(chunk)
        self.pos += n
        return n


def main():
    split, count = sys.argv[1], int(sys.argv[2])
    z = zipfile.ZipFile(io.BufferedReader(Remote(URL), 1 << 20))
    rows = list(csv.DictReader(io.TextIOWrapper(z.open("groove/info.csv"))))
    ok = [r for r in rows if r["beat_type"] == "beat" and r["audio_filename"] and float(r["duration"]) >= 20
          and split_of(r["audio_filename"]) == split]
    odd = [r for r in ok if r["time_signature"] != "4-4"]
    even = sorted((r for r in ok if r["time_signature"] == "4-4"), key=lambda r: (int(r["bpm"]), r["audio_filename"]))
    want = max(0, count - len(odd))
    pick = odd + [even[i * len(even) // want] for i in range(want)] if want else odd[:count]
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(HERE, f"groove-{split}.tsv"), "w") as m:
        m.write(f"# file\tbpm (quarter notes)\ttime signature\tstyle  (Groove MIDI Dataset, CC BY 4.0, Google LLC; split {split})\n")
        for r in pick:
            name = r["audio_filename"].replace("/", "_").removesuffix(".wav") + ".flac"
            dst = os.path.join(OUT, name)
            if not os.path.exists(dst):
                with tempfile.NamedTemporaryFile(suffix=".wav") as tmp:
                    with z.open("groove/" + r["audio_filename"]) as src:
                        tmp.write(src.read())
                    tmp.flush()
                    subprocess.run(["afconvert", "-f", "flac", "-d", "flac@22050", "-c", "1", tmp.name, dst], check=True)
            m.write(f"data/groove/{name}\t{r['bpm']}\t{r['time_signature'].replace('-', '/')}\t{r['style']}\n")
            print(f"{r['bpm']:>4} {r['time_signature']:>5} {r['style']:<24} {name}", flush=True)


main()
