//! One module per screen or region. Each builds elements from `&App` and the design kit and
//! wires handlers that mutate `AppState` through `App::act` (which also hands the queued commands
//! to the backend). Nothing here owns state.

pub mod account;
pub mod admin;
pub mod attach;
pub mod auth;
pub mod channels;
pub mod composer;
pub mod dialogs;
pub mod emoji;
pub mod emoji_settings;
pub mod members;
pub mod rail;
pub mod security;
pub mod server_settings;
pub mod settings;
pub mod shell;
pub mod sidebar;
pub mod toasts;
pub mod transcript;
pub mod update_required;
pub mod voice_bar;
pub mod voice_settings;
pub mod voice_stage;
