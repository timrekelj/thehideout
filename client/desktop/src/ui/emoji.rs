//! Emoji on screen outside the message rows: the picker popover (composer button, "+" chip and
//! hover bar on a message), the `:sm` autocomplete above the composer, and the pictures of a
//! channel's emoji icon in the sidebar, header and icon picker.
//!
//! The picker is the design crate's `EmojiPicker` in a `rune_kit` popover surface, hung from the
//! element that opened it (`PickerUi::anchor`) with a scrim behind it for outside presses.

use rune::{el, row, text, El, Key, Tx};
use rune_kit::{popover, Kit, Placement};
use thehideout_api::Channel;
use thehideout_design::components::chat::emoji_picker::picture;
use thehideout_design::components::EmojiPickerState;
use thehideout_design::components::{EmojiPicker, EmojiScope, EmojiSuggest, Pick};
use thehideout_design::{radius, theme_of};

use crate::app::App;
use crate::emoji::{IconEmoji, PickerTarget};
use crate::state::Place;

fn picker_state(a: &mut App) -> &mut EmojiPickerState {
    match a.state.ui.picker.as_mut() {
        Some(p) => &mut p.state,
        None => &mut a.state.ui.idle_picker,
    }
}

/// The picker and the autocomplete, pushed into the window's overlay layer.
pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>) {
    let s = &a.state;
    if let Some(p) = &s.ui.picker {
        let scope = if s.ui.place == Place::Home {
            EmojiScope::Unicode
        } else {
            EmojiScope::All
        };
        let body = EmojiPicker::new(
            "emoji-picker",
            &p.state,
            scope,
            &s.server_emojis(),
            &s.ui.recents,
            s.emoji_set(),
        )
        .build::<App>(
            k,
            picker_state,
            |a: &mut App, pick: Pick, _tx: &mut Tx| a.act(|s| s.picker_pick(pick)),
            |a: &mut App| a.act(|s| s.close_picker()),
        );
        // A transparent full-window press target under the picker: pressing outside closes it.
        layer.push(
            el().key("emoji-picker-scrim")
                .role("none")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .on_press(|a: &mut App| a.act(|s| s.close_picker())),
        );
        let placement = match p.target {
            PickerTarget::Composer => Placement::TopEnd,
            PickerTarget::Reaction { .. } => Placement::TopStart,
        };
        let surface = popover::raised(k, body.role("dialog").label("Emoji picker"), k.radius.md);
        layer.push(popover::anchored(k, surface, &p.anchor, placement, 6.0));
    }
    if let Some((_, list)) = s.suggestions() {
        let highlight = s.ui.suggest.highlight.min(list.len() - 1);
        let body = EmojiSuggest::new("emoji-suggest", &list, highlight, s.emoji_set())
            .on_pick(|a: &mut App, i: usize| {
                a.act(|s| {
                    s.suggest_accept(i);
                })
            })
            .build(k);
        let surface = popover::raised(k, body, k.radius.md);
        // Above the composer's left edge, clear of the box.
        layer.push(popover::anchored(
            k,
            surface,
            "composer",
            Placement::TopStart,
            6.0,
        ));
    }
}

/// Keys in the composer while the autocomplete is open: Up and Down move, Tab and Enter take the
/// highlighted emoji, Escape hides the list. Everything else goes on to the field.
pub fn composer_key(a: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx) -> bool {
    // Up in an empty box edits your last message (the autocomplete has nothing open then).
    if ev.key == Key::Up && !ev.mods.shift && !ev.mods.alt && !ev.mods.cmd && !ev.mods.ctrl && a.state.suggestions().is_none() {
        let mut took = false;
        a.act(|s| took = s.edit_last());
        if took {
            return true;
        }
    }
    let Some((_, list)) = a.state.suggestions() else {
        return false;
    };
    let at = a.state.ui.suggest.highlight.min(list.len() - 1);
    match ev.key {
        Key::Up => a.act(|s| s.suggest_step(-1)),
        Key::Down => a.act(|s| s.suggest_step(1)),
        Key::Tab | Key::Enter => {
            a.act(|s| {
                s.suggest_accept(at);
            });
        }
        Key::Escape => a.act(|s| s.suggest_dismiss()),
        _ => {
            a.state.ui.suggest.highlight = 0;
            return false;
        }
    }
    true
}

/// The picture of an emoji icon, `side` square: a Unicode emoji as text, a custom one through the
/// image cache (a faint square while it downloads).
pub fn icon_art(a: &App, k: &Kit, emoji: &IconEmoji, guild: u64, side: f32) -> Option<El> {
    let t = theme_of(k);
    Some(match emoji {
        IconEmoji::Unicode(g) => row().size(side, side).center().child(
            text(g.clone())
                .font(rune::TextStyle::new(side * 0.85, 400.0).lh(side))
                .color(t.text.color())
                .a11y_hidden(),
        ),
        IconEmoji::Custom(id) => {
            let img = a.state.emoji_image(guild, *id)?;
            let set = a.state.emoji_set();
            row()
                .size(side, side)
                .center()
                .child(picture(k, &set, &img, side, false))
        }
    })
}

/// A channel's emoji icon as an element of about `side`, or `None` when it has none (or its
/// custom emoji is gone, and the plain `#` shows).
pub fn channel_art(a: &App, k: &Kit, c: &Channel, side: f32) -> Option<El> {
    let emoji = a.state.channel_emoji(c)?;
    icon_art(a, k, &emoji, c.guild_id, side)
}

/// A custom emoji at rest for a settings row or grid tile.
pub fn emoji_tile(a: &App, k: &Kit, guild: u64, id: u64, side: f32) -> El {
    let t = theme_of(k);
    match a.state.emoji_image(guild, id) {
        Some(img) => {
            let set = a.state.emoji_set();
            picture(k, &set, &img, side, true)
        }
        None => el()
            .size(side, side)
            .radius(radius::SM)
            .fill(t.surface_raised.color())
            .a11y_hidden(),
    }
}

