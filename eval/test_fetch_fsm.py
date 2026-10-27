#!/usr/bin/env python3
"""fetch-fsm.py's page parsing, on a hand-built page (no network).

  python3 -I eval/test_fetch_fsm.py
"""
import importlib.util, os

spec = importlib.util.spec_from_file_location("fetch_fsm", os.path.join(os.path.dirname(os.path.abspath(__file__)), "fetch-fsm.py"))
fsm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fsm)

PAGE = b"""
<div data-dt='mp3' data-url='good-track' data-author='Some One'><a href='/music/good-track.mp3'>x</a> BPM: 120 Duration: 2:30 License: CC BY 4.0</div>
<div data-dt='mp3' data-url='../../../escape' data-author='Bad'><a href='/music/../../../escape.mp3'>x</a> BPM: 120 Duration: 2:30 License: CC BY</div>
<div data-dt='mp3' data-url='tabbed' data-author='A&#9;B'><a href='/music/tabbed.mp3'>x</a> BPM: 90 Duration: 1:30 License: CC  BY</div>
"""


def test_a_slug_cannot_steer_where_the_file_is_written():
    # the slug names data/fsm/<slug>.mp3, and the site is not ours
    assert [t[0] for t in fsm.tracks(PAGE)] == ["good-track", "tabbed"]


def test_tsv_fields_have_no_tabs_or_runs_of_spaces():
    # author and licence go into the manifest's tab-separated line
    tabbed = [t for t in fsm.tracks(PAGE) if t[0] == "tabbed"][0]
    assert tabbed[1] == "A B" and tabbed[5] == "CC BY", tabbed


def test_a_normal_track_comes_through_whole():
    assert next(fsm.tracks(PAGE)) == ("good-track", "Some One", "/music/good-track.mp3", 120, 150, "CC BY 4.0")


def test_fewer_found_than_wanted_keeps_each_once():
    # fsm-holdout.tsv: 141 lines, 133 tracks — 8 written twice, the fastest never
    assert fsm.spread(list(range(5)), 8) == [0, 1, 2, 3, 4], fsm.spread(list(range(5)), 8)


def test_more_found_than_wanted_spreads_across_them():
    assert fsm.spread(list(range(10)), 5) == [0, 2, 4, 6, 8]


if __name__ == "__main__":
    fails = 0
    for name, f in list(globals().items()):
        if name.startswith("test_"):
            try:
                f()
                print("ok  ", name)
            except AssertionError as e:
                fails += 1
                print("FAIL", name, e)
    raise SystemExit(fails)
