#!/usr/bin/env bash
# Builds the web client into a static folder that works under any URL prefix (the server
# serves it at /app/): index.html, boot.js, app.js, app_bg.wasm, the Inter fonts and wasm-bindgen's
# snippets. No inline scripts, so a strict CSP (script-src 'self' 'wasm-unsafe-eval') is enough.
#
#   client/web/build.sh [--dev] [--locked] [--out DIR]
#   client/web/build.sh [--dev] [--locked] --cook RECIPE   (Docker: build only the dependencies of a
#                                                           `cargo chef prepare` recipe, same flags)
#
# Needs: the wasm32-unknown-unknown target, wasm-bindgen-cli at the version in Cargo.lock
# (cargo install wasm-bindgen-cli --version <v> --locked), `buf` (the API crate's codegen) and,
# for a smaller release wasm, wasm-opt (binaryen; skipped when missing).
#
# This replicates rune's scripts/web-build.sh, which needs Python 3.11+ (tomllib).
set -euo pipefail
cd "$(dirname "$0")/.."

out=web/dist
profile=release
locked=()
cook=
while (($#)); do
    case "$1" in
        --dev) profile=debug; shift ;;
        --locked) locked=(--locked); shift ;;
        --cook) cook=${2:?--cook needs a recipe}; shift 2 ;;
        --out) out=${2:?--out needs a directory}; shift 2 ;;
        -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

# getrandom's browser backend; RUSTFLAGS replaces .cargo/config.toml's setting, so repeat it.
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""
build=()
if [[ $profile == release ]]; then
    build=(--release)
    # Defaults for size; set any of these to override.
    export CARGO_PROFILE_RELEASE_OPT_LEVEL=${CARGO_PROFILE_RELEASE_OPT_LEVEL:-s} \
        CARGO_PROFILE_RELEASE_LTO=${CARGO_PROFILE_RELEASE_LTO:-thin} CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
        CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=debuginfo
    # Keep the builder's home folder out of panic messages in a wasm meant for the public.
    export RUSTFLAGS="$RUSTFLAGS --remap-path-prefix=$HOME=/home --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
fi

if [[ -n $cook ]]; then
    exec cargo chef cook --target wasm32-unknown-unknown -p thehideout-desktop --recipe-path "$cook" \
        ${build[@]+"${build[@]}"} ${locked[@]+"${locked[@]}"}
fi

want=$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, "", $0); print; exit}' Cargo.lock)
if ! command -v wasm-bindgen >/dev/null || [[ $(wasm-bindgen --version) != "wasm-bindgen $want" ]]; then
    echo "Install the matching CLI: cargo install wasm-bindgen-cli --version $want --locked" >&2
    exit 2
fi

cargo build -p thehideout-desktop --lib --target wasm32-unknown-unknown ${build[@]+"${build[@]}"} ${locked[@]+"${locked[@]}"}
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
artifact="$target/wasm32-unknown-unknown/$profile/thehideout_desktop.wasm"

rm -rf "$out"
mkdir -p "$out"
bindgen=(--target web --out-dir "$out" --out-name app)
[[ $profile == release ]] && bindgen+=(--remove-name-section)
wasm-bindgen "${bindgen[@]}" "$artifact"
rm -f "$out"/*.d.ts

if [[ $profile == release ]] && command -v wasm-opt >/dev/null; then
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types \
        --enable-exception-handling "$out/app_bg.wasm" -o "$out/app_bg.opt.wasm"
    mv "$out/app_bg.opt.wasm" "$out/app_bg.wasm"
elif [[ $profile == release ]]; then
    echo "wasm-opt not found: the wasm is not size-optimised (brew install binaryen)" >&2
fi

# Rune embeds Inter cut to Latin, Greek and Cyrillic and fetches the full faces from beside app.js
# once the first frame is up: ship the ones that come with the Rune this build uses.
fonts=$(cargo metadata --format-version 1 --filter-platform wasm32-unknown-unknown | python3 -c '
import json, os, sys
for p in json.load(sys.stdin)["packages"]:
    if p["name"] == "rune":
        print(os.path.join(os.path.dirname(p["manifest_path"]), "assets", "web"))
        break')
cp "$fonts"/Inter-*.ttf "$out/"

cat > "$out/index.html" <<'HTML'
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<title>thehideout</title>
<link rel="icon" href="data:,">
<style>html,body{margin:0;width:100%;height:100%;overflow:hidden;overscroll-behavior:none;touch-action:none}#status{font:16px system-ui;padding:24px}</style>
<div id="status">Loading…</div>
<script type="module" src="./boot.js"></script>
</html>
HTML

cat > "$out/boot.js" <<'JS'
// Starts the app. A separate file (not an inline script) so the page's CSP can forbid inline code.
import init from './app.js';

const status = document.querySelector('#status');
try {
    await init();
    status.remove();
} catch (error) {
    status.textContent = `thehideout could not start: ${error}`;
    console.error(error);
}
JS

wasm="$out/app_bg.wasm"
raw=$(wc -c < "$wasm")
gz=$(gzip -9 -c "$wasm" | wc -c)
if command -v brotli >/dev/null; then br=$(brotli -q 11 -c "$wasm" | wc -c); else br=n/a; fi
echo "$wasm: $raw bytes raw, $gz gzip, $br brotli"
echo "Built $profile. Static folder: $out"
