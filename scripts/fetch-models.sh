#!/usr/bin/env bash
# Beat This! (Foscarin, Schlüter & Widmer, ISMIR 2024; MIT) model files for the
# tempo tracker, via the beat-this-rs port. Not in git (10 + 83 MB): fetched once
# into ~/Library/Application Support/tempo/models, where drumkit and the eval read them.
#  - mel_spectrogram.onnx, beat_this_small.onnx: the repo's committed copies, pinned to a commit
#  - beat_this.onnx: the full model from the repo's model-large release
# Each file is checked against a sha256 pinned here (2026-10-18; beat_this.onnx's
# matches the release's own .sha256). A download that doesn't match is deleted and
# the script fails, and a file already there that doesn't match is fetched again.
# Tested by test-fetch-models.sh.
set -euo pipefail
DIR="$HOME/Library/Application Support/tempo/models"
REPO=danigb/beat-this-rs
COMMIT=1ae768e78f1ad83b0ed3886241dc29ffde853c40
mkdir -p "$DIR" && cd "$DIR"

fetch() { # file url sha256
    if [ -s "$1" ] && echo "$3  $1" | shasum -a 256 -c --status; then return; fi
    curl -fL --retry 3 -o "$1.part" "$2"
    if ! echo "$3  $1.part" | shasum -a 256 -c --status; then
        rm -f "$1.part"
        echo "fetch-models: $1 does not match its pinned sha256, not installed" >&2
        exit 1
    fi
    mv "$1.part" "$1"
}

fetch mel_spectrogram.onnx "https://raw.githubusercontent.com/$REPO/$COMMIT/models/mel_spectrogram.onnx" \
    fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9
fetch beat_this_small.onnx "https://raw.githubusercontent.com/$REPO/$COMMIT/models/beat_this_small.onnx" \
    a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f
fetch beat_this.onnx "https://github.com/$REPO/releases/download/model-large/beat_this.onnx" \
    5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02
ls -la "$DIR"
