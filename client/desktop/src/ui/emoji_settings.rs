//! Server settings → Emojis: the count against the 200 the server allows, the upload zone (a file
//! chooser or a drop; PNG, JPEG, WebP or GIF up to 2 MiB), the new emoji's name before it is
//! added, and the list with each emoji's picture, name (renamed in place), "Animated" badge,
//! who added it, and rename and delete buttons. Everything greys out without MANAGE_EMOJIS, and
//! the upload zone at 200.
//!
//! Animated emojis rest on their first frame and play while the pointer is over their row.

use rune::{col, row, text, El, Key, TextEdit, Tx};
use rune_kit::Kit;
use thehideout_design::components::{
    Badge, BadgeVariant, Button, IconButton, Size, TextInput, Variant,
};
use thehideout_design::icons::{IMAGE, PENCIL, TRASH, X};
use thehideout_design::{radius, theme_of, type_};

use super::server_settings::{errors, page_col};
use super::settings::setting;
use crate::app::App;
use crate::emoji::MAX_EMOJIS;
use crate::forms::DialogKind;
use crate::media::IMAGE_EXTENSIONS;
use crate::server_settings::ServerSettings;

fn draft_edit(a: &mut App) -> &mut TextEdit {
    match a
        .state
        .ui
        .server_settings
        .as_mut()
        .and_then(|s| s.emojis.draft.as_mut())
    {
        Some(d) => &mut d.name,
        None => &mut a.state.ui.idle_composer,
    }
}

fn rename_edit(a: &mut App) -> &mut TextEdit {
    match a
        .state
        .ui
        .server_settings
        .as_mut()
        .and_then(|s| s.emojis.rename.as_mut())
    {
        Some(r) => &mut r.name,
        None => &mut a.state.ui.idle_composer,
    }
}

/// Opens the system's file chooser; its answer goes to `pick_emoji_file`.
fn choose_file(tx: &mut Tx) {
    let options = rune::FileDialogOptions {
        title: "Choose an emoji image".into(),
        ..Default::default()
    }
    .filter("Images", IMAGE_EXTENSIONS);
    tx.open_file(options, |a: &mut App, result, _tx| match result {
        Ok(Some(path)) => a.act(|s| s.pick_emoji_file(&path)),
        Ok(None) => {}
        Err(e) => a.act(|s| {
            s.toast(
                thehideout_design::components::ToastKind::Danger,
                format!("Can't open the file chooser. {e}"),
            )
        }),
    });
}

pub fn page(a: &App, k: &Kit, ss: &ServerSettings) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let s = &a.state;
    let editable = s.can_manage_emojis(guild);
    let list = s.emojis_of(guild);
    let full = list.len() >= MAX_EMOJIS;
    let page = &ss.emojis;

    let mut head = row().key("emoji-head").w_full().gap(12.0).center_y();
    head = head
        .child(
            text(format!("{} / {MAX_EMOJIS}", list.len()))
                .key("emoji-count")
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(if full { t.danger } else { t.text }.color()),
        )
        .child(
            text("emojis")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .grow(1.0),
        );
    let upload = Button::new(if page.reading {
        "Reading\u{2026}"
    } else {
        "Upload emoji\u{2026}"
    })
    .icon(&IMAGE)
    .disabled(!editable || full || page.reading || page.busy)
    .build(k)
    .key("emoji-upload")
    .on_press_tx(|_: &mut App, tx: &mut Tx| choose_file(tx));
    head = head.child(upload);

    let hint = if full {
        format!(
            "This server has {MAX_EMOJIS} emojis, the most it can have. Delete one to add another."
        )
    } else {
        "PNG, JPEG, WebP or GIF, up to 2 MiB. Names use lowercase letters, digits and underscores. Drop a file here or choose one.".to_owned()
    };
    let mut zone = col()
        .key("emoji-zone")
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .radius(radius::LG)
        .border(
            1.0,
            if page.drop_over {
                t.accent.color()
            } else {
                rune::Color::TRANSPARENT
            },
        )
        .fill(if page.drop_over {
            t.accent_subtle.color()
        } else {
            t.hover.color()
        })
        .child(head)
        .child(
            text(hint)
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color())
                .wrap()
                .w_full(),
        )
        .children(errors(k, &page.error, "emoji-error"));
    if let Some(d) = &page.draft {
        let input = TextInput::new(&d.name)
            .placeholder("emoji_name")
            .label("Emoji name")
            .disabled(page.busy)
            .on_edit(draft_edit)
            .on_submit(|a: &mut App, _| a.act(|s| s.create_emoji()))
            .on_key(|a: &mut App, ev, _tx| {
                if ev.key == Key::Escape {
                    a.act(|s| s.discard_emoji_draft());
                    return true;
                }
                false
            })
            .build(k);
        let preview = row()
            .key("emoji-draft-preview")
            .size(40.0, 40.0)
            .center()
            .child(
                rune::el()
                    .size(36.0, 36.0)
                    .pixels_fit(d.image.preview.pixels(), rune::SurfaceFit::Fit)
                    .a11y_hidden(),
            );
        zone = zone.child(
            row()
                .key("emoji-draft")
                .w_full()
                .gap(10.0)
                .center_y()
                .child(preview)
                .child(col().grow(1.0).min_w(0.0).child(input))
                .child(
                    Button::new(if page.busy { "Adding\u{2026}" } else { "Add" })
                        .disabled(page.busy)
                        .on_press(|a: &mut App| a.act(|s| s.create_emoji()))
                        .build(k)
                        .key("emoji-add"),
                )
                .child(
                    Button::new("Cancel")
                        .variant(Variant::Secondary)
                        .disabled(page.busy)
                        .on_press(|a: &mut App| a.act(|s| s.discard_emoji_draft()))
                        .build(k)
                        .key("emoji-cancel"),
                ),
        );
        if let Some(e) = &d.error {
            zone = zone.child(
                text(e.clone())
                    .key("emoji-draft-error")
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.danger.color())
                    .wrap()
                    .w_full(),
            );
        }
    }
    let zone = zone
        .on_drop_files(|a: &mut App, files, _tx| {
            if let Some(path) = files.first() {
                a.act(|s| s.pick_emoji_file(path));
            }
        })
        .accepts_extensions(IMAGE_EXTENSIONS)
        .on_drop_hover(|a: &mut App, over, _tx| {
            if let Some(ss) = a.state.ui.server_settings.as_mut() {
                ss.emojis.drop_over = over;
            }
        });

    // The rows' hover boxes overhang the column by their padding (a row grows past the column
    // by the margins), so the emoji and the buttons line up with the text above.
    let mut rows = col().key("emoji-list").grow(1.0).min_w(0.0).mx(-8.0);
    for e in list {
        rows = rows.child(emoji_row(a, k, ss, e.id, editable));
    }
    if list.is_empty() {
        rows = rows.child(
            text("No custom emojis yet. Upload one to use it in messages and reactions.")
                .key("emoji-empty")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .wrap()
                .w_full()
                .px(8.0)
                .py(8.0),
        );
    }

    page_col(k, "Emojis")
        .child(setting(
            k,
            "Upload",
            "Anyone in the server can use them in messages, reactions and channel icons.",
            zone,
        ))
        .child(setting(
            k,
            "Custom emojis",
            "Type :name: in a message to use one.",
            row().w_full().child(rows),
        ))
}

fn emoji_row(a: &App, k: &Kit, ss: &ServerSettings, id: u64, editable: bool) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let guild = ss.guild;
    let Some(e) = s.emoji(guild, id) else {
        return row();
    };
    let renaming = ss.emojis.rename.as_ref().filter(|r| r.emoji == id);
    let mut name_cell = col().grow(1.0).shrink().min_w(0.0).gap(2.0);
    name_cell = match renaming {
        Some(r) => {
            let input = TextInput::new(&r.name)
                .label("Emoji name")
                .disabled(r.busy)
                .on_edit(rename_edit)
                .on_submit(|a: &mut App, _| a.act(|s| s.commit_rename()))
                .on_key(|a: &mut App, ev, _tx| {
                    if ev.key == Key::Escape {
                        a.act(|s| s.cancel_rename());
                        return true;
                    }
                    false
                })
                .build(k)
                .key(format!("emoji-rename-{id}"));
            name_cell.child(input).children(errors(
                k,
                &r.error,
                &format!("emoji-rename-error-{id}"),
            ))
        }
        None => name_cell.child(
            text(format!(":{}:", e.name))
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(t.text.color())
                .max_lines(1),
        ),
    };
    let by = s.user_name(e.creator_id);
    let mut r = row()
        .key(format!("emoji-row-{id}"))
        .w_full()
        .h(48.0)
        .gap(12.0)
        .center_y()
        .px(8.0)
        .radius(radius::MD)
        .hover_group()
        .hover(t.hover.color())
        .child(
            row()
                .size(32.0, 32.0)
                .center()
                .child(super::emoji::emoji_tile(a, k, guild, id, 28.0)),
        )
        .child(name_cell);
    if e.animated {
        r = r.child(
            Badge::new("Animated")
                .variant(BadgeVariant::Accent)
                .build(k),
        );
    }
    r = r.child(
        text(format!("Added by {by}"))
            .font(type_::at(k, type_::CAPTION))
            .color(t.text_muted.color())
            .max_lines(1)
            .w(140.0),
    );
    let actions = match renaming {
        Some(_) => row().gap(2.0).child(
            IconButton::new(&X, "Cancel rename")
                .size(Size::Sm)
                .on_press(|a: &mut App| a.act(|s| s.cancel_rename()))
                .build(k)
                .key(format!("emoji-rename-cancel-{id}")),
        ),
        None => row()
            .gap(2.0)
            .child(
                IconButton::new(&PENCIL, "Rename")
                    .size(Size::Sm)
                    .disabled(!editable)
                    .on_press(move |a: &mut App| a.act(|s| s.start_rename(id)))
                    .build(k)
                    .key(format!("emoji-rename-{id}-button")),
            )
            .child(
                IconButton::new(&TRASH, "Delete")
                    .size(Size::Sm)
                    .disabled(!editable)
                    .on_press(move |a: &mut App| {
                        a.act(|s| s.open_dialog(DialogKind::DeleteEmoji { guild, emoji: id }))
                    })
                    .build(k)
                    .key(format!("emoji-delete-{id}")),
            ),
    };
    r.child(actions.mr(-4.0))
}
