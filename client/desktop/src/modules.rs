// The module list shared by the desktop binary (`main.rs`) and the browser library (`lib.rs`):
// both `include!` this file at their crate root.
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
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(test)]
mod account_ui_tests;
mod perms;
mod real;
mod rt;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(test)]
mod roles_tests;
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(target_arch = "wasm32")]
mod voice_web;
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
