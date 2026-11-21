//! The dialogs: Create server, Join server, Create channel, Channel settings, Create / Rename /
//! Delete section, Invite and New message. The design `Dialog` supplies the modal, Esc and the
//! motion; these supply the content. Enter submits, the primary button says what it is doing
//! while it waits, and an error from the server (or from validation) shows under the field.

use rune::{col, icon, row, text, El, Key, TextEdit, Tx};
use rune_kit::{Kit, Select};
use thehideout_design::metrics_of;
use thehideout_design::components::chat::emoji::glyph_label;
use thehideout_design::components::chat::pickers::row_key;
use thehideout_design::components::{
    dialog_field, Avatar, Button, ColorSwatches, Dialog, EmojiPicker, EmojiPickerState,
    EmojiScope, IconButton, IconGrid, PickerTabs, Pick, Size, TextInput, ToastKind, Variant,
};
use thehideout_design::icons::{catalog, CHEVRON_DOWN, COPY, HASH, SEARCH, X};
use thehideout_design::{palette_color, radius, space, theme_of, type_};

use crate::app::App;
use crate::emoji::IconEmoji;
use crate::state::{ChannelForm, DialogKind, DialogState, Phase};

fn dialog_edit(a: &mut App) -> &mut TextEdit {
    match &mut a.state.ui.dialog {
        Some(d) => &mut d.input,
        None => &mut a.state.ui.idle_composer,
    }
}

fn topic_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.dialog.as_mut().and_then(|d| d.channel.as_mut()) {
        Some(f) => &mut f.topic,
        None => &mut a.state.ui.idle_composer,
    }
}

fn search_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.dialog.as_mut().and_then(|d| d.channel.as_mut()) {
        Some(f) => &mut f.search,
        None => &mut a.state.ui.idle_composer,
    }
}

/// What of a window's height a dialog's chrome (padding, header, footer, gaps, the layer's margin)
/// takes: the body gets the rest and scrolls past it.
const DIALOG_CHROME: f32 = 230.0;

pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, window_h: f32) {
    let Some(d) = &a.state.ui.dialog else { return };
    let max_h = (window_h - DIALOG_CHROME).max(160.0);
    match d.kind {
        DialogKind::CreateGuild => form(
            k,
            layer,
            d,
            "Create server",
            "Give your server a name. You can change it later.",
            "Server name",
            "The Hideout",
            ("Create", "Creating\u{2026}"),
            false,
        ),
        DialogKind::JoinGuild => form(
            k,
            layer,
            d,
            "Join server",
            "Paste an invite code from a friend.",
            "Invite code",
            "HIDEOUT1",
            ("Join", "Joining\u{2026}"),
            false,
        ),
        DialogKind::CreateSection { .. } => form(
            k,
            layer,
            d,
            "Create section",
            "Sections group channels in the sidebar.",
            "Section name",
            "Projects",
            ("Create", "Creating\u{2026}"),
            false,
        ),
        DialogKind::RenameSection { .. } => form(
            k,
            layer,
            d,
            "Rename section",
            "Channels in it stay where they are.",
            "Section name",
            "Projects",
            ("Save", "Saving\u{2026}"),
            false,
        ),
        DialogKind::DeleteSection { section, .. } => delete_section(a, k, layer, d, section),
        DialogKind::CreateChannel { guild } => channel_dialog(
            a,
            k,
            layer,
            max_h,
            d,
            guild,
            "Create channel",
            "Channels keep a conversation in one place.",
            ("Create", "Creating\u{2026}"),
        ),
        DialogKind::ChannelSettings { guild, .. } => channel_dialog(
            a,
            k,
            layer,
            max_h,
            d,
            guild,
            "Edit channel",
            "Change how this channel looks and what it is about.",
            ("Save", "Saving\u{2026}"),
        ),
        DialogKind::NewDm => new_dm(a, k, layer, d),
        DialogKind::Profile => profile(a, k, layer, d),
        DialogKind::DeleteRole { guild, role } => delete_role(a, k, layer, d, guild, role),
        DialogKind::DeleteEmoji { guild, emoji } => delete_emoji(a, k, layer, d, guild, emoji),
        DialogKind::Invite { guild } => invite(a, k, layer, guild),
        DialogKind::InviteByEmail { guild } => invite_by_email(a, k, layer, max_h, d, guild),
    }
}

fn close(a: &mut App) {
    a.act(|s| s.close_dialog());
}

fn footer(k: &Kit, d: &DialogState, labels: (&str, &str)) -> (El, El) {
    let busy = d.phase == Phase::Busy;
    let cancel = Button::new("Cancel")
        .variant(Variant::Secondary)
        .on_press(close)
        .build(k)
        .key("dialog-cancel");
    let submit = Button::new(if busy { labels.1 } else { labels.0 })
        .disabled(busy)
        .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
        .build(k)
        .key("dialog-submit");
    (cancel, submit)
}

#[allow(clippy::too_many_arguments)]
fn form(
    k: &Kit,
    layer: &mut Vec<El>,
    d: &DialogState,
    title: &str,
    description: &str,
    label: &str,
    placeholder: &str,
    labels: (&str, &str),
    hash: bool,
) {
    let mut input = TextInput::new(&d.input)
        .placeholder(placeholder)
        .on_edit(dialog_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.submit_dialog()));
    if hash {
        input = input.leading_icon(&HASH);
    }
    let (cancel, submit) = footer(k, d, labels);
    Dialog::new("dlg", title)
        .description(description)
        .open(true)
        .child(col().key("dialog-field").w_full().child(dialog_field(
            k,
            label,
            input,
            d.error.as_deref(),
        )))
        .footer(cancel)
        .footer(submit)
        .on_close(close)
        .build(k, layer);
}

fn delete_section(a: &App, k: &Kit, layer: &mut Vec<El>, d: &DialogState, section: u64) {
    let guild = match d.kind {
        DialogKind::DeleteSection { guild, .. } => guild,
        _ => return,
    };
    let name = a
        .state
        .layout(guild)
        .and_then(|l| l.sections.iter().find(|s| s.id == section))
        .map_or_else(String::new, |s| s.name.clone());
    let busy = d.phase == Phase::Busy;
    let t = theme_of(k);
    let mut body = col().key("dialog-confirm").w_full().gap(6.0);
    if let Some(e) = &d.error {
        body = body.child(
            text(e.clone())
                .font(type_::at(k, type_::CONTROL))
                .color(t.danger.color())
                .wrap()
                .w_full(),
        );
    }
    Dialog::new("dlg", "Delete section")
        .description(format!("\u{201c}{name}\u{201d} will be removed. Its channels move to the top of the list; nothing is deleted from them."))
        .open(true)
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).on_press(close).build(k).key("dialog-cancel"))
        .footer(
            Button::new(if busy { "Deleting\u{2026}" } else { "Delete section" })
                .variant(Variant::Danger)
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

// ---- channel create / settings ---------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn channel_dialog(
    a: &App,
    k: &Kit,
    layer: &mut Vec<El>,
    max_h: f32,
    d: &DialogState,
    guild: u64,
    title: &str,
    description: &str,
    labels: (&str, &str),
) {
    let Some(f) = &d.channel else { return };
    let t = theme_of(k);
    let creating = matches!(d.kind, DialogKind::CreateChannel { .. });
    let name = TextInput::new(&d.input)
        .placeholder("plans")
        .leading_icon(&HASH)
        .on_edit(dialog_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.submit_dialog()));
    let mut body = col()
        .key("dialog-field")
        .w_full()
        .gap(16.0)
        .child(dialog_field(k, "Channel name", name, d.error.as_deref()));
    // Name, topic, icon and colour are always all there: the picker opens under the Icon field and
    // its grid is short, so the dialog fits a 1280x800 window whole (and scrolls in a smaller one).
    if !creating {
        let topic = TextInput::new(&f.topic)
            .placeholder("What is this channel about?")
            .on_edit(topic_edit)
            .on_submit(|a: &mut App, _| a.act(|s| s.submit_dialog()));
        body = body.child(dialog_field(k, "Topic", topic, None));
    }
    if creating {
        body = body.child(section_field(a, k, layer, f, guild));
    }
    body = body.child(icon_row(a, k, f, guild));
    if f.picker_open {
        body = body.child(picker(a, k, f, guild));
    }
    body = body.child(
        col()
            .key("color-field")
            .w_full()
            .gap(6.0)
            .child(text("Colour").font(type_::at(k, type_::LABEL)).color(t.text_muted.color()))
            .child(
                ColorSwatches::new(f.color)
                    .on_pick(|a: &mut App, key: &'static str| a.act(|s| s.pick_color(key)))
                    .build(k),
            ),
    );
    let (cancel, submit) = footer(k, d, labels);
    Dialog::new("dlg", title)
        .description(description)
        .open(true)
        .body_max_h(max_h)
        .child(body)
        .footer(cancel)
        .footer(submit)
        .on_close(close)
        .build(k, layer);
}

/// "Icon": a field-looking button showing the chosen icon; pressing it opens the picker.
fn icon_row(a: &App, k: &Kit, f: &ChannelForm, guild: u64) -> El {
    let t = theme_of(k);
    let glyph = catalog::by_name(&f.icon).unwrap_or(&thehideout_design::icons::HASH);
    let tone = palette_color(f.color, t).unwrap_or(t.text_muted);
    let emoji_art = f.emoji.as_ref().and_then(|e| super::emoji::icon_art(a, k, e, guild, 18.0));
    let label = match &f.emoji {
        Some(IconEmoji::Unicode(g)) => glyph_label(g),
        Some(IconEmoji::Custom(id)) => a.state.emoji(guild, *id).map_or_else(|| "Custom emoji".to_owned(), |e| format!(":{}:", e.name)),
        None if f.icon.is_empty() => "Default".to_owned(),
        None => f.icon.replace('-', " "),
    };
    let has_icon = !f.icon.is_empty() || f.emoji.is_some();
    let field = row()
        .key("icon-field")
        .role("button")
        .label(format!("Icon: {label}"))
        .expanded(f.picker_open)
        .grow(1.0)
        .h(32.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(radius::MD)
        .fill(t.surface_raised.color())
        .border(
            1.0,
            if f.picker_open {
                k.c.border_focus
            } else {
                t.control_border.color()
            },
        )
        .focusable(true)
        .focus_ring(space::RING_WIDTH, space::RING_OFFSET, k.c.border_focus)
        .hover(t.hover.color())
        .press(t.pressed.color())
        .on_press(|a: &mut App| a.act(|s| s.toggle_picker()))
        .child(match emoji_art {
            Some(art) => art,
            None => icon(glyph, thehideout_design::icon::SM)
                .color(tone.color())
                .a11y_hidden(),
        })
        .child(
            text(label)
                .font(type_::at(k, type_::CONTROL))
                .color(t.text.color())
                .grow(1.0)
                .max_lines(1)
                .a11y_hidden(),
        )
        .child(
            icon(&CHEVRON_DOWN, thehideout_design::icon::SM)
                .color(t.text_muted.color())
                .a11y_hidden(),
        );
    let mut line = row()
        .key("icon-line")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(field);
    if has_icon {
        line = line.child(
            IconButton::new(&X, "Use the default icon")
                .size(Size::Md)
                .on_press(|a: &mut App| a.act(|s| s.pick_icon("")))
                .build(k),
        );
    }
    col()
        .key("icon-field-wrap")
        .w_full()
        .gap(6.0)
        .child(text("Icon").font(type_::at(k, type_::LABEL)).color(t.text_muted.color()))
        .child(line)
}

/// The emoji picker's state inside the open channel dialog.
fn icon_emoji_state(a: &mut App) -> &mut EmojiPickerState {
    match a.state.ui.dialog.as_mut().and_then(|d| d.channel.as_mut()) {
        Some(f) => &mut f.emoji_pick,
        None => &mut a.state.ui.idle_picker,
    }
}

/// Tabs, then the tab's search and grid: Phosphor icons, Unicode emoji or the server's emojis.
fn picker(a: &App, k: &Kit, f: &ChannelForm, guild: u64) -> El {
    let tabs = PickerTabs::new(&[("Icons", true), ("Emoji", true), ("Server emoji", true)], f.tab)
        .on_pick(|a: &mut App, i: usize| a.act(|s| s.set_picker_tab(i)))
        .build(k);
    if f.tab == 0 {
        return icons_picker(k, f, tabs);
    }
    let scope = if f.tab == 1 { EmojiScope::Unicode } else { EmojiScope::Server };
    let body = EmojiPicker::new("icon-emoji", &f.emoji_pick, scope, &a.state.server_emojis(), &a.state.ui.recents, a.state.emoji_set())
        .visible_rows(4)
        .build::<App>(
            k,
            icon_emoji_state,
            |a: &mut App, pick: Pick, _tx: &mut Tx| a.act(|s| s.pick_icon_emoji(pick)),
            |a: &mut App| a.act(|s| s.toggle_picker()),
        );
    let _ = guild;
    col().key("icon-picker").w_full().gap(8.0).child(tabs).child(row().w_full().justify_center().child(body))
}

/// The Icons tab: search and the Phosphor grid.
fn icons_picker(k: &Kit, f: &ChannelForm, tabs: El) -> El {
    let search = TextInput::new(&f.search)
        .placeholder("Search icons")
        .label("Search icons")
        .leading_icon(&SEARCH)
        .large()
        .width(None)
        .on_edit(search_edit)
        .on_change(|a: &mut App, _tx| {
            a.state
                .channel_form()
                .map(ChannelForm::research)
                .unwrap_or_default()
        })
        .on_submit(|a: &mut App, _| a.act(|s| s.pick_highlighted()))
        .on_key(|a: &mut App, ev, tx| {
            let (dx, dy) = match ev.key {
                Key::Left => (-1, 0),
                Key::Right => (1, 0),
                Key::Up => (0, -1),
                Key::Down => (0, 1),
                _ => return false,
            };
            if let Some(f) = a.state.channel_form() {
                f.step_highlight(dx, dy);
                if let Some(h) = f.highlight {
                    tx.scroll_into_view(row_key(
                        thehideout_design::components::chat::pickers::row_of(h),
                    ));
                }
            }
            true
        });
    col()
        .key("icon-picker")
        .w_full()
        .gap(8.0)
        .child(tabs)
        .child(search.build(k))
        .child(
            IconGrid::new(f.results.clone(), f.icon.clone())
                .visible_rows(4)
                .highlight(f.highlight)
                .on_pick(|a: &mut App, name: &'static str| a.act(|s| s.pick_icon(name)))
                .build::<App>(k),
        )
}

fn section_field(a: &App, k: &Kit, layer: &mut Vec<El>, f: &ChannelForm, guild: u64) -> El {
    let t = theme_of(k);
    let sections: Vec<(u64, String)> = a
        .state
        .layout(guild)
        .map(|l| l.sections.iter().map(|s| (s.id, s.name.clone())).collect())
        .unwrap_or_default();
    let mut labels = vec!["Top level".to_owned()];
    labels.extend(sections.iter().map(|(_, n)| n.clone()));
    let current = f
        .section
        .and_then(|id| sections.iter().find(|(s, _)| *s == id))
        .map_or("Top level", |(_, n)| n.as_str());
    let by_label = sections.clone();
    let select = Select::new("section-select", &labels)
        .width(392.0)
        .label("Section")
        .current(Some(current))
        .open(f.section_open)
        .on_toggle(|a: &mut App| {
            if let Some(f) = a.state.channel_form() {
                f.section_open = !f.section_open;
            }
        })
        .on_close(|a: &mut App| {
            if let Some(f) = a.state.channel_form() {
                f.section_open = false;
            }
        })
        .on_choose(move |a: &mut App, label: &str, _tx| {
            let chosen = by_label.iter().find(|(_, n)| n == label).map(|(id, _)| *id);
            if let Some(f) = a.state.channel_form() {
                f.section = chosen;
                f.section_open = false;
            }
        });
    col()
        .key("section-field")
        .w_full()
        .gap(6.0)
        .child(
            text("Section")
                .font(type_::at(k, type_::LABEL))
                .color(t.text_muted.color()),
        )
        .child(select.build(k, layer))
}

// ---- the rest --------------------------------------------------------------------------------

fn new_dm(a: &App, k: &Kit, layer: &mut Vec<El>, d: &DialogState) {
    let t = theme_of(k);
    let typed = d.input.text.trim().trim_start_matches('@').to_lowercase();
    let input = TextInput::new(&d.input)
        .placeholder("ana")
        .leading_icon(&thehideout_design::icons::AT)
        .on_edit(dialog_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.submit_dialog()));
    let mut people = col().key("dialog-people").w_full().gap(2.0);
    for u in a
        .state
        .known_users()
        .into_iter()
        .filter(|u| u.username.to_lowercase().contains(&typed))
        .take(6)
    {
        let name = u.username.clone();
        people = people.child(
            row()
                .key(format!("dm-user-{}", u.id))
                .role("button")
                .label(name.clone())
                .w_full()
                .h(metrics_of(k).row_h())
                .px(8.0)
                .gap(10.0)
                .center_y()
                .radius(radius::MD)
                .focusable(true)
                .focus_ring(space::RING_WIDTH, space::RING_INSET, k.c.border_focus)
                .hover(t.hover.color())
                .press(t.pressed.color())
                .on_press(move |a: &mut App| {
                    let name = name.clone();
                    a.act(|s| {
                        if let Some(dlg) = &mut s.ui.dialog {
                            dlg.input.replace_text(name);
                        }
                        s.submit_dialog();
                    });
                })
                .child(
                    Avatar::new(u.id.to_string(), u.username.clone())
                        .size(24)
                        .backdrop(t.surface_overlay.color())
                        .build(k),
                )
                .child(
                    text(u.username.clone())
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color())
                        .a11y_hidden(),
                ),
        );
    }
    let (cancel, submit) = footer(k, d, ("Open", "Opening\u{2026}"));
    Dialog::new("dlg", "New message")
        .description("Start a conversation with someone from your servers.")
        .open(true)
        .child(col().key("dialog-field").w_full().child(dialog_field(
            k,
            "Username",
            input,
            d.error.as_deref(),
        )))
        .child(people)
        .footer(cancel)
        .footer(submit)
        .on_close(close)
        .build(k, layer);
}

fn invite(a: &App, k: &Kit, layer: &mut Vec<El>, guild: u64) {
    let t = theme_of(k);
    let Some(g) = a.state.guild(guild) else {
        return;
    };
    let code = g.invite_code.clone();
    let copy = Button::new("Copy")
        .icon(&COPY)
        .variant(Variant::Secondary)
        .build(k)
        .key("invite-copy")
        .on_press_tx(move |a: &mut App, tx| {
            tx.copy(code.clone());
            a.state.toast(ToastKind::Success, "Invite code copied");
        });
    let shown = row()
        .key("invite-code")
        .grow(1.0)
        .h(32.0)
        .px(12.0)
        .center_y()
        .radius(radius::MD)
        .fill(t.surface_raised.color())
        .border(1.0, t.control_border.color())
        .label(format!("Invite code {}", g.invite_code))
        .child(
            text(g.invite_code.clone())
                .font(type_::at(k, type_::CODE))
                .color(t.text.color())
                .a11y_hidden(),
        );
    Dialog::new("dlg", "Invite people")
        .description(format!("Anyone with this code can join {}.", g.name))
        .open(true)
        .child(
            row()
                .key("invite-row")
                .w_full()
                .gap(8.0)
                .center_y()
                .child(shown)
                .child(copy),
        )
        .footer(
            Button::new("Done")
                .on_press(close)
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

/// Invite someone by email: the address, then the code that was sent (with copy), and this
/// server's pending invites to send again or withdraw.
fn invite_by_email(a: &App, k: &Kit, layer: &mut Vec<El>, max_h: f32, d: &DialogState, guild: u64) {
    let t = theme_of(k);
    let s = &a.state;
    let name = s.guild(guild).map_or_else(String::new, |g| g.name.clone());
    let busy = d.phase == Phase::Busy;
    let input = TextInput::new(&d.input)
        .placeholder("friend@example.com")
        .on_edit(dialog_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.submit_dialog()));
    let mut body = col()
        .key("dialog-field")
        .w_full()
        .gap(16.0)
        .child(dialog_field(k, "Email", input, d.error.as_deref()));
    let inv = &s.ui.invites;
    if let Some(created) = &inv.created {
        let code = created.code.clone().unwrap_or_default();
        // A copyable value: the code, or the link the email has (shared some other way too).
        let value = |what: &'static str, key: &'static str, value: String, toast: &'static str| {
            let copied = value.clone();
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    row()
                        .key(key)
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .h(32.0)
                        .px(12.0)
                        .center_y()
                        .radius(radius::MD)
                        .fill(t.surface_raised.color())
                        .border(1.0, t.control_border.color())
                        .label(format!("{what} {value}"))
                        .child(text(value).font(type_::at(k, type_::CODE)).color(t.text.color()).truncate().a11y_hidden()),
                )
                .child(
                    Button::new("Copy")
                        .icon(&COPY)
                        .variant(Variant::Secondary)
                        .build(k)
                        .key(format!("{key}-copy"))
                        .on_press_tx(move |a: &mut App, tx| {
                            tx.copy(copied.clone());
                            a.state.toast(ToastKind::Success, toast);
                        }),
                )
        };
        let mut card = col()
            .key("invite-created")
            .w_full()
            .gap(8.0)
            .padding(12.0)
            .radius(radius::MD)
            .fill(t.success_bg.color())
            .child(
                text(format!("Invite sent to {}. Share the link, or the code to enter under \u{201c}Have an invite? Join with a code\u{201d} on the sign-in screen:", created.email))
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text.color())
                    .wrap()
                    .w_full(),
            );
        if let Some(link) = created.link.clone() {
            card = card.child(value("Invite link", "invite-created-link", link, "Invite link copied"));
        }
        body = body.child(card.child(value("Invite code", "invite-created-code", code, "Invite code copied")));
    }
    let pending: Vec<_> = inv.list.iter().filter(|i| i.status == thehideout_api::InviteStatus::Pending).collect();
    let mut list = col().key("invite-pending").w_full().gap(2.0).child(
        text("Pending invites").font(type_::at(k, type_::LABEL)).color(t.text_muted.color()),
    );
    if let Some(e) = &inv.error {
        list = list.child(text(e.clone()).font(type_::at(k, type_::CONTROL)).color(t.danger.color()).wrap().w_full());
    } else if !inv.loaded {
        list = list.child(text("Loading\u{2026}").font(type_::at(k, type_::CONTROL)).color(t.text_muted.color()));
    } else if pending.is_empty() {
        list = list.child(text("No pending invites.").key("invite-pending-empty").font(type_::at(k, type_::CONTROL)).color(t.text_muted.color()));
    }
    for i in pending {
        let id = i.id;
        let row_busy = inv.busy.contains(&id);
        let expires = crate::format::day_label(crate::format::day_of(s.tz, crate::format::unix(i.expires_at)), s.today);
        let by = i.inviter.as_ref().map_or_else(String::new, |u| format!(" \u{00b7} by {}", u.username));
        list = list.child(
            row()
                .key(format!("invite-row-{id}"))
                .w_full()
                .min_h(metrics_of(k).row_h() + 8.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(radius::MD)
                .hover(t.hover.color())
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(text(i.email.clone()).font(type_::at(k, type_::CONTROL_STRONG)).color(t.text.color()).max_lines(1))
                        .child(text(format!("Expires {expires}{by}")).font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).max_lines(1)),
                )
                .child(
                    Button::new("Resend")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .disabled(row_busy)
                        .on_press(move |a: &mut App| a.act(|s| s.resend_invite(id)))
                        .build(k)
                        .key(format!("invite-resend-{id}")),
                )
                .child(
                    Button::new("Withdraw")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .disabled(row_busy)
                        .on_press(move |a: &mut App| a.act(|s| s.revoke_invite(id)))
                        .build(k)
                        .key(format!("invite-revoke-{id}")),
                ),
        );
    }
    body = body.child(list);
    Dialog::new("dlg", "Invite people by email")
        .description(format!("They get an email to make an account and join {name}. Invites last 7 days."))
        .open(true)
        .body_max_h(max_h)
        .child(body)
        .footer(Button::new("Done").variant(Variant::Secondary).on_press(close).build(k).key("dialog-cancel"))
        .footer(
            Button::new(if busy { "Sending\u{2026}" } else { "Send invite" })
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

// ---- profile and roles -----------------------------------------------------------------------

/// Your picture: choose a file (or drop one on the zone), see it, save or remove it.
fn profile(a: &App, k: &Kit, layer: &mut Vec<El>, d: &DialogState) {
    let t = theme_of(k);
    let s = &a.state;
    let me = s.me();
    let (id, name) = me.map_or_else(|| (String::new(), String::new()), |u| (u.id.to_string(), u.username.clone()));
    let has_avatar = me.is_some_and(|u| u.avatar.is_some());
    let busy = d.phase == Phase::Busy;
    let pick = &d.pick;
    let pixels = match &pick.draft {
        Some(draft) => Some(draft.preview.pixels().clone()),
        None => s.avatar_pixels(s.my_id()),
    };
    let preview = super::server_settings::tile(k, &id, &name, pixels, 72.0, radius::LG).key("profile-preview");
    let mut actions = row().key("profile-actions").gap(8.0).center_y();
    actions = actions.child(
        Button::new(if pick.reading { "Reading\u{2026}" } else { "Choose image\u{2026}" })
            .icon(&thehideout_design::icons::IMAGE)
            .variant(Variant::Secondary)
            .disabled(pick.reading || busy)
            .build(k)
            .key("profile-choose")
            .on_press_tx(|_: &mut App, tx: &mut rune::Tx| super::server_settings::choose_image(tx, crate::backend::ImagePurpose::Avatar)),
    );
    if pick.draft.is_some() {
        actions = actions.child(
            Button::new("Use current")
                .variant(Variant::Ghost)
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.discard_avatar_draft()))
                .build(k)
                .key("profile-discard"),
        );
    } else if has_avatar {
        actions = actions.child(
            Button::new(if busy { "Removing\u{2026}" } else { "Remove" })
                .variant(Variant::Ghost)
                .icon(&thehideout_design::icons::TRASH)
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.remove_avatar()))
                .build(k)
                .key("profile-remove"),
        );
    }
    let zone = row()
        .key("profile-zone")
        .w_full()
        .gap(16.0)
        .padding(12.0)
        .center_y()
        .radius(radius::LG)
        .border(1.0, if pick.drop_over { t.accent.color() } else { rune::Color::TRANSPARENT })
        .fill(if pick.drop_over { t.accent_subtle.color() } else { t.hover.color() })
        .child(preview)
        .child(
            col()
                .grow(1.0)
                .shrink()
                .gap(8.0)
                .child(actions)
                .child(
                    text(if busy { "Uploading\u{2026}" } else { "PNG, JPEG, WebP or GIF, up to 8 MiB. Drop a file here." })
                        .key("profile-hint")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color())
                        .wrap(),
                ),
        )
        .on_drop_files(|a: &mut App, files, _tx| {
            if let Some(path) = files.first() {
                a.act(|s| s.pick_image(crate::backend::ImagePurpose::Avatar, path));
            }
        })
        .accepts_extensions(crate::media::IMAGE_EXTENSIONS)
        .on_drop_hover(|a: &mut App, over, _tx| {
            if let Some(d) = a.state.ui.dialog.as_mut() {
                d.pick.drop_over = over;
            }
        });
    let mut body = col().key("profile-body").w_full().gap(12.0).child(zone);
    if let Some(e) = &d.error {
        body = body.child(text(e.clone()).key("profile-error").font(type_::at(k, type_::CONTROL)).color(t.danger.color()).wrap().w_full());
    }
    Dialog::new("dlg", "Profile")
        .description(format!("This is how you look to everyone, {name}."))
        .open(true)
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).on_press(close).build(k).key("dialog-cancel"))
        .footer(
            Button::new(if busy { "Saving\u{2026}" } else { "Save" })
                .disabled(busy || pick.draft.is_none())
                .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

fn delete_emoji(a: &App, k: &Kit, layer: &mut Vec<El>, d: &DialogState, guild: u64, emoji: u64) {
    let t = theme_of(k);
    let name = a.state.emoji(guild, emoji).map_or_else(String::new, |e| e.name.clone());
    let busy = d.phase == Phase::Busy;
    let mut body = col().key("dialog-confirm").w_full().gap(8.0).child(row().gap(10.0).center_y().child(super::emoji::emoji_tile(a, k, guild, emoji, 32.0)).child(text(format!(":{name}:")).font(type_::at(k, type_::BODY_STRONG)).color(t.text.color())));
    if let Some(e) = &d.error {
        body = body.child(text(e.clone()).font(type_::at(k, type_::CONTROL)).color(t.danger.color()).wrap().w_full());
    }
    Dialog::new("dlg", "Delete emoji")
        .description("It disappears from the picker and from reactions. Old messages that used it show its name instead. This can't be undone.")
        .open(true)
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).on_press(close).build(k).key("dialog-cancel"))
        .footer(
            Button::new(if busy { "Deleting\u{2026}" } else { "Delete emoji" })
                .variant(Variant::Danger)
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

fn delete_role(a: &App, k: &Kit, layer: &mut Vec<El>, d: &DialogState, guild: u64, role: u64) {
    let t = theme_of(k);
    let s = &a.state;
    let name = s.role(guild, role).map_or_else(String::new, |r| r.name.clone());
    let holders = s.role_members(guild, role);
    let busy = d.phase == Phase::Busy;
    let mut body = col().key("dialog-confirm").w_full().gap(6.0);
    if let Some(e) = &d.error {
        body = body.child(text(e.clone()).font(type_::at(k, type_::CONTROL)).color(t.danger.color()).wrap().w_full());
    }
    let who = match holders {
        0 => "No one has it right now.".to_owned(),
        1 => "It will be taken from the 1 member who has it.".to_owned(),
        n => format!("It will be taken from the {n} members who have it."),
    };
    Dialog::new("dlg", "Delete role")
        .description(format!("\u{201c}{name}\u{201d} will be deleted. {who} This can't be undone."))
        .open(true)
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).on_press(close).build(k).key("dialog-cancel"))
        .footer(
            Button::new(if busy { "Deleting\u{2026}" } else { "Delete role" })
                .variant(Variant::Danger)
                .disabled(busy)
                .on_press(|a: &mut App| a.act(|s| s.submit_dialog()))
                .build(k)
                .key("dialog-submit"),
        )
        .on_close(close)
        .build(k, layer);
}
