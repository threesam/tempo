#!/usr/bin/env python3
"""Fetch real, mixed music with a known tempo: Kevin MacLeod's tracks
(incompetech.com, CC BY 3.0) as mirrored on Wikimedia Commons, whose file
pages carry the composer's own Tempo, Genre and Instruments fields.

Each track belongs to one split forever, by a hash of its title: `holdout` is
sealed (scored only at milestones, never tuned on), `dev` is for iterating.
Writes eval/data/macleod/*.mp3 and eval/macleod-<split>.tsv (file, bpm, 0 = meter
unknown, genre / drums|nodrums). Picks a tempo spread: mostly tracks with a
drum kit, a quarter without (the hard case: the beat lives in the harmony).

usage: eval/fetch-macleod.py dev 120 | holdout 80
"""
import hashlib, json, os, re, sys, time, urllib.error, urllib.parse, urllib.request

API = "https://commons.wikimedia.org/w/api.php"
UA = {"User-Agent": "tempo-eval/0.1 (personal test set)"}
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "data", "macleod")


def get(url):
    """GET with Wikimedia manners: one request a second, back off on 429."""
    for wait in (2, 5, 15, 40, 90):
        time.sleep(1)
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers=UA)).read()
        except urllib.error.HTTPError as e:
            if e.code != 429:
                raise
            time.sleep(wait)
    raise SystemExit("still rate-limited; try again later")


def api(**q):
    q["format"] = "json"
    return json.loads(get(API + "?" + urllib.parse.urlencode(q)))


def field(text, name):
    m = re.search(r"name=" + name + r"\|value=([^}]*)\}", text)
    return m.group(1).strip() if m else ""


def split_of(title):
    return "holdout" if int(hashlib.sha1(title.encode()).hexdigest(), 16) % 10 < 4 else "dev"


def main():
    split, count = sys.argv[1], int(sys.argv[2])
    index = os.path.join(OUT, "index.json")  # metadata cached: the slow, rate-limited part
    if os.path.exists(index):
        tracks = [tuple(t) for t in json.load(open(index))]
    else:
        titles, off = [], 0
        while off is not None and len(titles) < 1400:
            d = api(action="query", list="search", srsearch='incompetech insource:"name=Tempo"',
                    srnamespace=6, srlimit=100, sroffset=off)
            titles += [r["title"] for r in d["query"]["search"] if r["title"].endswith(".mp3")]
            off = d.get("continue", {}).get("sroffset")
        tracks = []
        for i in range(0, len(titles), 25):
            d = api(action="query", titles="|".join(titles[i : i + 25]), prop="revisions|imageinfo",
                    rvprop="content", rvslots="main", iiprop="url")
            for p in d["query"]["pages"].values():
                text = p["revisions"][0]["slots"]["main"]["*"]
                m = re.match(r"(\d+(?:\.\d+)?) ?bpm", field(text, "Tempo"))
                if not m:
                    continue
                kit = "Kit" in field(text, "Instruments") or "Drum" in field(text, "Instruments")
                src = re.search(r"mp3: (https://incompetech\.com/\S+?\.mp3)", text)  # the composer's own copy
                url = src.group(1) if src else p["imageinfo"][0]["url"]
                tracks.append((float(m.group(1)), kit, field(text, "Genre"), p["title"], url))
        os.makedirs(OUT, exist_ok=True)
        json.dump(tracks, open(index, "w"))
    tracks = sorted({t[3]: t for t in tracks if 50 <= t[0] <= 200 and split_of(t[3]) == split}.values(), key=lambda t: (t[0], t[3]))
    pick = []
    for kit, n in ((True, count - count // 4), (False, count // 4)):
        pool = sorted((t for t in tracks if t[1] == kit), key=lambda t: t[0])
        pick += [pool[i * len(pool) // n] for i in range(min(n, len(pool)))]
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(HERE, f"macleod-{split}.tsv"), "w") as m:
        m.write(f"# file\tbpm\ttime signature (0 = unknown)\tgenre  (Kevin MacLeod, incompetech.com, CC BY 3.0, via Wikimedia Commons; split {split})\n")
        for bpm, kit, genre, title, url in pick:
            name = re.sub(r"[^A-Za-z0-9]+", "-", title[5:-4]).strip("-") + ".mp3"
            dst = os.path.join(OUT, name)
            if not os.path.exists(dst):
                data = get(url)
                with open(dst, "wb") as f:
                    f.write(data)
            tag = "drums" if kit else "nodrums"
            m.write(f"data/macleod/{name}\t{bpm:g}\t0\t{genre} / {tag}\n")
            print(f"{bpm:>6g} {tag:<8} {genre:<20} {name}", flush=True)


main()
