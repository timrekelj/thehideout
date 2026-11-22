//! thehideout desktop client, drawn with Rune. Every screen is built from `thehideout-design`
//! and driven through the `Backend` trait: `real.rs` talks to the server, `fake.rs` is the
//! deterministic stand-in (`--fake`, and every headless `--scene`/`--snap` run).
//!
//! Headless review: `./render.sh` writes the scenes in `scenes.rs` to `snapshots/`.

#![forbid(unsafe_code)]

#[cfg(test)]
mod a11y_tests;
mod account;
mod admin;
#[cfg(test)]
mod account_tests;
#[cfg(test)]
mod admin_tests;
mod app;
mod apply;
mod attach;
#[cfg(test)]
mod attach_tests;
mod backend;
mod crash;
mod emoji;
#[cfg(test)]
mod emoji_tests;
mod fake;
mod fake_admin;
mod fake_lines;
mod format;
mod forms;
mod layout;
mod lifecycle;
mod look;
mod media;
mod moderation;
mod moderation_ui;
#[cfg(test)]
mod moderation_ui_tests;
#[cfg(test)]
mod moderation_tests;
#[cfg(test)]
mod account_lifecycle_tests;
mod perms;
mod real;
#[cfg(test)]
mod roles_tests;
mod scenes;
mod server_settings;
mod state;
mod structure;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod tests;
mod ui;
mod voice;
mod voice_engine;
#[cfg(test)]
mod voice_engine_tests;
#[cfg(test)]
mod voice_tests;
#[cfg(test)]
mod voice_settings_tests;
#[cfg(test)]
mod voice_ui_tests;
#[cfg(test)]
mod voice_video_tests;

use rune::{AppSpec, Titlebar};
use thehideout_design::{FAMILIES, FONTS};

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
    rune::App::new(AppSpec::<app::App> {
        fonts: FONTS,
        families: FAMILIES,
        min_size: Some((800.0, 600.0)),
        scenes: scenes::SCENES,
        setup: Some(app::setup),
        poll: Some(app::poll),
        on_key: Some(app::on_key),
        ..AppSpec::new("thehideout", (1280.0, 800.0), app::init, app::view)
    })
    .titlebar(Titlebar::new(ui::shell::TITLEBAR))
    .on_open_url(app::on_open_url)
    .run();
}
