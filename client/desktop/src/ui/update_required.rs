//! The blocking "Update required" screen: the server refused this app version, so nothing else
//! works until a newer build is installed. It replaces the whole window.

use rune::{El, col, text};
use rune_kit::Kit;
use thehideout_design::components::Button;
use thehideout_design::{theme_of, type_};

use crate::app::App;

/// Where the newest build is published.
pub const DOWNLOAD_URL: &str = "https://thehideout.zabit.si/download";

pub fn build(_a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    col()
        .key("screen-update-required")
        .w_full()
        .h_full()
        .fill(t.bg.color())
        .items_center()
        .justify_center()
        .gap(12.0)
        .child(
            text("Update required")
                .font(type_::at(k, type_::TITLE))
                .color(t.text.color()),
        )
        .child(
            text("This version of thehideout is too old to connect. Download the latest version to continue.")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .wrap()
                .max_w(380.0),
        )
        .child(
            Button::new("Download update")
                .build(k)
                .key("update-download")
                .on_press_tx(|_a: &mut App, tx| {
                    let _ = tx.open_url(DOWNLOAD_URL);
                }),
        )
}
