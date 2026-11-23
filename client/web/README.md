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
- Voice is not available yet (`voice::available()`): voice channels answer with a note, the
  Voice & audio page and the mute/deafen buttons are hidden. The plain voice types live in
  `thehideout-voice-types`; the engine trait is `desktop/src/voice_engine.rs::Engine`.
- WebGPU, with WebGL2 as the fallback (rune's `webgl` feature).

`RUSTFLAGS` must carry `--cfg getrandom_backend="wasm_js"` (see `.cargo/config.toml`; `build.sh`
adds it, because an env `RUSTFLAGS` replaces the config's).
