#!/usr/bin/env bash
# Fetch every test set (dev + sealed holdout). Safe to re-run: files on disk are kept.
set -euo pipefail
cd "$(dirname "$0")"
python3 -I fetch-groove.py dev 120
python3 -I fetch-groove.py holdout 80
for split in "dev 120" "holdout 80"; do
    for try in 1 2 3 4 5 6; do python3 -I fetch-macleod.py $split && break; sleep 120; done
done
wc -l groove-*.tsv macleod-*.tsv
du -sh data
