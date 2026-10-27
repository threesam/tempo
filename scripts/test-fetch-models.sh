#!/usr/bin/env bash
# Runs fetch-models.sh for real, in a scratch HOME, with a stand-in curl on PATH
# that serves either good bytes (copies of the installed models) or junk.
#   scripts/test-fetch-models.sh     (needs the models installed once)
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REAL="$HOME/Library/Application Support/tempo/models"
[ -s "$REAL/beat_this.onnx" ] || { echo "skip: run fetch-models.sh once first"; exit 0; }
fails=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }

run() { # mode (good|junk) [corrupt-file-to-place]; leaves $T/home
    T="$(mktemp -d)"; mkdir -p "$T/bin" "$T/home/Library/Application Support/tempo/models"
    M="$T/home/Library/Application Support/tempo/models"
    [ -n "${2:-}" ] && echo junk > "$M/$2"
    cat > "$T/bin/curl" <<SHIM
#!/usr/bin/env bash
out=""; url=""
while [ \$# -gt 0 ]; do case "\$1" in -o) out="\$2"; shift 2;; -*) shift;; *) url="\$1"; shift;; esac; done
name="\$(basename "\$url")"
if [ "$1" = junk ]; then echo junk > "\$out"
elif [ "\${name%.sha256}" != "\$name" ]; then echo "\$(shasum -a 256 "$REAL/\${name%.sha256}" | cut -d' ' -f1)  \${name%.sha256}" > "\$out"
else cp "$REAL/\$name" "\$out"; fi
SHIM
    chmod +x "$T/bin/curl"
    HOME="$T/home" PATH="$T/bin:$PATH" bash "$HERE/fetch-models.sh" > "$T/log" 2>&1
    echo $? > "$T/status"
}
sum() { shasum -a 256 "$1" 2>/dev/null | cut -d' ' -f1; }

run junk
check "a junk download fails the script" '[ "$(cat $T/status)" != 0 ]'
check "a junk download is not installed" '! ls "$M" | grep -q "^beat_this"'
check "no half-fetched file is left" '! ls "$M" | grep -q "\.part$"'

run good
check "a good download installs all three" '[ "$(cat $T/status)" = 0 ] && [ "$(sum "$M/beat_this.onnx")" = "$(sum "$REAL/beat_this.onnx")" ]'

run good beat_this_small.onnx
check "a corrupt file already there is fetched again" '[ "$(sum "$M/beat_this_small.onnx")" = "$(sum "$REAL/beat_this_small.onnx")" ]'

[ $fails = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
