#!/usr/bin/env python3
"""Second, independent source (Codex R2): free-stock-music.com, many composers,
CC-licensed MP3s, each with a published BPM. Kevin MacLeod is excluded (he's
source one). Sealed: the whole set is a holdout, never tuned on.

Caveat for the notebook: it's not known whether the site's BPM comes from the
composer or from software. If software, its errors may correlate with ours.

Crawls genre pages politely (1 request/s), keeps tracks of 60 s or more at
50-200 BPM, at most 8 per artist (diversity, and a cap on any one composer's
habits), and writes eval/data/fsm/*.mp3 and eval/fsm-holdout.tsv (file, bpm,
0, artist / genre / license).

usage: eval/fetch-fsm.py 150
"""
import html, os, re, sys, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "data", "fsm")
BASE = "https://www.free-stock-music.com/"
UA = {"User-Agent": "Mozilla/5.0 (tempo-eval; personal test set)"}
GENRES = ["hip-hop", "lo-fi-hip-hop", "chill-hop", "electronic-pop", "pop", "rock", "funk", "dubstep",
          "electronica", "jazz", "reggae", "latin", "drum-and-bass", "house", "trap", "rnb-soul", "country", "blues", "disco"]


def get(url, referer=None):
    time.sleep(1)
    try:
        h = dict(UA, Referer=referer) if referer else UA  # the site serves an HTML page, not the MP3, without one
        return urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=60).read()
    except Exception as e:
        print(f"  skip {url}: {e}", flush=True)
        return None


def tracks(page):
    h = page.decode("utf-8", "ignore")
    for m in re.finditer(r"data-dt='mp3' data-url='([^']+)' data-author='([^']+)'.*?href='(/music/[^']+\.mp3)'.*?BPM: (\d+).*?Duration: (\d+):(\d+).*?License: ([^<]+?)</div>", h, re.S):
        slug, author, href, bpm, mm, ss, lic = m.groups()
        # the slug names the file written under data/fsm, and the site is not ours:
        # letters, digits and dashes only, so no "../" can steer a write (/rev)
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", slug):
            continue
        # author and licence go into a TSV line: no tabs or newlines inside them
        clean = lambda x: " ".join(html.unescape(x).split())
        yield slug, clean(author), href, int(bpm), int(mm) * 60 + int(ss), clean(lic)


def spread(pick, want):
    """`want` of `pick` (sorted by tempo), spread across it; all of it when there are no more."""
    return pick if len(pick) <= want else [pick[i * len(pick) // want] for i in range(want)]


def main():
    want = int(sys.argv[1])
    seen, per_artist, pick = set(), {}, []
    for g in GENRES:
        for n in range(1, 8):
            page = get(BASE + (f"{g}.html" if n == 1 else f"{g}.page-{n}.html"))
            if not page:
                break
            found = 0
            for slug, author, href, bpm, dur, lic in tracks(page):
                found += 1
                if slug in seen or slug not in href or "macleod" in author.lower() or not (50 <= bpm <= 200) or dur < 60:
                    continue
                if per_artist.get(author, 0) >= 8:
                    continue
                seen.add(slug)
                per_artist[author] = per_artist.get(author, 0) + 1
                pick.append((slug, author, href, bpm, g, lic))
            if not found:
                break
        print(f"{g}: {len(pick)} so far, {len(per_artist)} artists", flush=True)
        if len(pick) >= want * 1.3:
            break
    # a tempo spread across what was found
    pick.sort(key=lambda t: (t[3], t[0]))
    pick = spread(pick, want)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(HERE, "fsm-holdout.tsv"), "w") as m:
        m.write("# file\tbpm (site)\ttime signature (0 = unknown)\tartist / genre / license  (free-stock-music.com; sealed holdout, source 2)\n")
        for slug, author, href, bpm, g, lic in pick:
            dst = os.path.join(OUT, slug + ".mp3")
            if not os.path.exists(dst):
                data = get(BASE.rstrip("/") + href, referer=BASE + slug + ".html")
                if not data or not (data[:3] == b"ID3" or data[:2] in (b"\xff\xfb", b"\xff\xf3", b"\xff\xf2")):
                    print(f"  not audio: {slug}", flush=True)
                    continue
                open(dst, "wb").write(data)
            m.write(f"data/fsm/{slug}.mp3\t{bpm}\t0\t{author} / {g} / {lic}\n")
            print(f"{bpm:>4} {author:<24} {g:<14} {slug}", flush=True)


if __name__ == "__main__":
    main()
