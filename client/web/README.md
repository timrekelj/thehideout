# The web client

The desktop client (`thehideout-desktop`) also builds for the browser: `desktop/src/lib.rs` is the
wasm32 entry (a `wasm_bindgen(start)` that runs the same `app::spec()` as `main.rs`), and
`web/build.sh` packages it as a static folder meant to be served under `/app/`.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version "$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/,""); print; exit}' Cargo.lock)" --locked
brew install binaryen          # wasm-opt, optional: shrinks the wasm
web/build.sh                   # release build into web/dist (web/build.sh --dev for a fast debug one)
```

The folder is `index.html`, `boot.js`, `app.js`, `app_bg.wasm`, Inter's fonts and wasm-bindgen's
`snippets/` (the JS in `desktop/js/web.js`). All URLs are relative and there is no inline script, so
it works under any prefix and under `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'`.

How it differs from the desktop app:

- The server is the page's origin (`location.origin`); the client header is `web/<version>`.
- The session, look and other prefs live in `localStorage` (`thehideout.*`), written at once.
- Files are picked with `<input type=file>` and uploaded with `XMLHttpRequest` (upload progress);
  downloads and recovery codes go through `<a download>`. The picked `File` is carried through the
  path-based state as a `/web-files/<n>/<name>` path (`web.rs`).
- Passkeys run in the page (`navigator.credentials`), no ceremony page.
- Voice, video and screen share run on the `livekit-client` SDK, behind the same `Engine` trait as the
  native engine: `desktop/src/voice_web.rs` + `desktop/js/voice.js`. `build.sh` downloads the SDK
  (version and integrity in `web/livekit-client.lock`, Apache-2.0) to the bundle root; it is loaded on
  the first join, never at page load.
  - Audio: voice-activity gating, push to talk (while the tab is focused), input/output pickers, mic
    test, mute for me, deafen, and cues (synthesized, played through WebAudio).
  - Video: camera (with a picker on Voice & audio) and screen share (the browser's own picker opens
    from the share click, replacing the desktop's source list). Tiles are fed by copying frames out of
    the decoder (`voice.js`, "the frame copy": a detached <video> -> canvas scaled to what the tile
    count can show -> RGBA -> rune's `Pixels`), so dialogs and menus composite over them.
  - `build.sh --test-hooks` keeps the `// @hook` logging the browser tests read; the shipped bundle
    has none of it.
- WebGPU, with WebGL2 as the fallback (rune's `webgl` feature).

`RUSTFLAGS` must carry `--cfg getrandom_backend="wasm_js"` (see `.cargo/config.toml`; `build.sh`
adds it, because an env `RUSTFLAGS` replaces the config's).
