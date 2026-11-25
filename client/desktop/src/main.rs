//! thehideout desktop client, drawn with Rune. Every screen is built from `thehideout-design`
//! and driven through the `Backend` trait: `real.rs` talks to the server, `fake.rs` is the
//! deterministic stand-in (`--fake`, and every headless `--scene`/`--snap` run).
//!
//! Headless review: `./render.sh` writes the scenes in `scenes.rs` to `snapshots/`.

#![forbid(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
include!("modules.rs");

#[cfg(not(target_arch = "wasm32"))]
use rune::Titlebar;

// The browser build is the library (`lib.rs`); this binary has nothing to run there.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // Headless review runs (scenes, snapshots) write no crash files and never look at the prefs.
    if !app::headless()
        && let Some(dir) = crash::crash_dir()
    {
        crash::report_unseen(&dir);
        crash::install(dir);
    }
    rune::prewarm_gpu();
    // The titlebar is drawn by the view (a drag band at the top, see `ui::shell`); macOS keeps its
    // traffic lights centred in it, Windows gets caption buttons, Linux keeps its native frame.
    app::spec().titlebar(Titlebar::new(ui::shell::TITLEBAR)).on_open_url(app::on_open_url).run();
}
