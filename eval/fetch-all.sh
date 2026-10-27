#!/usr/bin/env bash
# Fetch every test set (dev + sealed holdout). Safe to re-run: files on disk are kept.
set -euo pipefail
cd "$(dirname "$0")"
python3 -I fetch-groove.py dev 120
python3 -I fetch-groove.py holdout 80
for split in "dev 120" "holdout 80"; do
    ok=0
    for try in 1 2 3 4 5 6; do python3 -I fetch-macleod.py $split && ok=1 && break; sleep 120; done
    [ $ok = 1 ] || { echo "fetch-all: macleod $split failed after 6 tries" >&2; exit 1; }
done
wc -l groove-*.tsv macleod-*.tsv
du -sh data
