//! The server's settings page: a full-window view like the user settings, with a nav on the left
//! (Overview, Roles, Members) and the page on the inset panel. Overview is the name and icon,
//! Roles the list (ordered, draggable, with colour dots and counts) beside an editor, Members
//! the people with their role chips. What the signed-in user may not do is greyed out here the
//! way the server will refuse it (`server_settings.rs` in the state mirrors the rules); an error
//! the server still gives is shown inline.

use rune::{col, el, icon, row, text, Anim, Drag, El, Phase, Tx};
use rune_kit::{Kit, Menu, MenuItem};
use thehideout_api::{PaletteColor, Permissions, Role};
use thehideout_design::components::avatar::{avatar_tones, initials, picture, tone_index};
use thehideout_design::components::{
    dialog_field, switch, Avatar, Button, ColorSwatches, IconButton, Size, TextInput, Variant,
};
use thehideout_design::icons::{
    self, ARROW_DOWN, ARROW_UP, IMAGE, LOCK, PLUS, SEARCH, SHIELD, TRASH, USERS, X,
};
use thehideout_design::{icon as isize, palette_color, palette_text, radius, space, theme_of, type_};

use super::settings::{frame, header, nav_shell, setting, toggle, PAGE_W};
use crate::app::App;
use crate::backend::ImagePurpose;
use crate::forms::DialogKind;
use crate::media::IMAGE_EXTENSIONS;
use crate::server_settings::{
    has_bit, ImagePick, RoleForm, ServerPage, ServerSettings, PERMISSION_ROWS, ROLE_PITCH,
};

/// The row of a role in the list: 36 tall, 6 apart (`ROLE_PITCH` 42 steps over one).
const ROLE_ROW_H: f32 = 36.0;
/// Points a press must travel before it is a drag, not a click.
const SLOP: f32 = 4.0;

pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, bar: f32) -> El {
    let Some(ss) = a.state.ui.server_settings.as_ref() else {
        return el();
    };
    let name = a
        .state
        .guild(ss.guild)
        .map_or_else(String::new, |g| g.name.clone());
    let page = match ss.page {
        ServerPage::Overview => overview(a, k, ss),
        ServerPage::Roles => roles(a, k, ss),
        ServerPage::Members => members(a, k, layer, ss),
        ServerPage::Bans => bans(a, k, ss),
        ServerPage::Emojis => super::emoji_settings::page(a, k, ss),
    };
    frame(
        a,
        k,
        bar,
        nav(a, k, ss, &name),
        col().w_full().items_center().child(page),
    )
}

fn nav(a: &App, k: &Kit, ss: &ServerSettings, name: &str) -> El {
    let guild = ss.guild;
    let mut rows = Vec::new();
    for (page, label, glyph) in [
        (ServerPage::Overview, "Overview", &icons::INFO),
        (ServerPage::Roles, "Roles", &SHIELD),
        (ServerPage::Members, "Members", &USERS),
        (ServerPage::Bans, "Bans", &icons::PROHIBIT),
        (ServerPage::Emojis, "Emojis", &icons::SMILEY),
    ] {
        if !a.state.settings_page_allowed(guild, page) {
            continue;
        }
        rows.push(
            thehideout_design::components::NavRow::new(label, glyph)
                .selected(ss.page == page)
                .highlight_group("server-settings-nav")
                .on_press(move |a: &mut App| a.act(|s| s.server_page(page)))
                .build(k)
                .key(format!("server-nav-{label}")),
        );
    }
    nav_shell(k, name, rows)
}

fn close(a: &mut App) {
    a.act(|s| s.close_server_settings());
}

pub(super) fn page_col(k: &Kit, title: &str) -> El {
    col()
        .key(format!("page-server-{}", title.to_lowercase()))
        .w_full()
        .max_w(PAGE_W)
        .px(32.0)
        .pb(32.0)
        .child(header(k, title, close))
}

pub(super) fn errors(k: &Kit, e: &Option<String>, key: &str) -> Option<El> {
    let t = theme_of(k);
    e.as_ref().map(|e| {
        text(e.clone())
            .key(key.to_owned())
            .font(type_::at(k, type_::CONTROL))
            .color(t.danger.color())
            .wrap()
            .w_full()
    })
}

// ---- overview --------------------------------------------------------------------------------

/// Opens the system's file chooser for a picture; its answer goes to `pick_image` (the backend
/// reads the file off the UI thread).
pub(super) fn choose_image(tx: &mut Tx, purpose: ImagePurpose) {
    let options = rune::FileDialogOptions {
        title: "Choose a picture".into(),
        ..Default::default()
    }
    .filter("Images", IMAGE_EXTENSIONS);
    tx.open_file(options, move |a: &mut App, result, _tx| match result {
        Ok(Some(path)) => a.act(|s| s.pick_image(purpose, &path)),
        Ok(None) => {}
        Err(e) => a.act(|s| {
            s.toast(
                thehideout_design::components::ToastKind::Danger,
                format!("Can't open the file chooser. {e}"),
            )
        }),
    });
}

/// A big square for a picture: initials on the deterministic fill with the picture over them.
pub(super) fn tile(k: &Kit, id: &str, name: &str, pixels: Option<rune::Pixels>, side: f32, corner: f32) -> El {
    let t = theme_of(k);
    let fill = avatar_tones(t.spec.accent)[tone_index(id)].color();
    let font = rune::TextStyle { size: side * 0.35, line_height: side * 0.4, weight: 600.0, ..type_::TITLE };
    let mut tile = el()
        .key("picture-tile")
        .role("img")
        .label(name.to_owned())
        .size(side, side)
        .center()
        .radius(corner)
        .fill(fill)
        .child(text(initials(name)).font(font).color(thehideout_design::components::avatar::AVATAR_INK.color()).a11y_hidden());
    if let Some(p) = pixels {
        tile = tile.child(picture(&p, side, corner));
    }
    tile
}

/// The server's icon (or the picked preview) as a tile.
fn icon_tile(a: &App, k: &Kit, guild: u64, pick: &ImagePick) -> El {
    let name = a.state.guild(guild).map_or_else(String::new, |g| g.name.clone());
    let pixels = match &pick.draft {
        Some(d) => Some(d.preview.pixels().clone()),
        None => a.state.icon_pixels(guild),
    };
    tile(k, &guild.to_string(), &name, pixels, 80.0, radius::XL).key("server-icon-tile")
}

fn overview(a: &App, k: &Kit, ss: &ServerSettings) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let current = a
        .state
        .guild(guild)
        .map_or_else(String::new, |g| g.name.clone());
    let editable = a.state.can_manage_server(guild);

    let input = TextInput::new(&ss.name)
        .placeholder("The Hideout")
        .disabled(!editable || ss.name_busy)
        .on_edit(name_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.rename_guild()));
    let unchanged = ss.name.text.trim() == current;
    let save = Button::new(if ss.name_busy {
        "Saving\u{2026}"
    } else {
        "Save"
    })
    .disabled(!editable || ss.name_busy || unchanged)
    .on_press(|a: &mut App| a.act(|s| s.rename_guild()))
    .build(k)
    .key("server-name-save");
    let name_field =
        col().key("server-name").w_full().gap(8.0).child(
            row()
                .w_full()
                .gap(8.0)
                .items_start()
                .child(col().grow(1.0).child(
                    dialog_field(k, "Server name", input, ss.name_error.as_deref()).w_full(),
                ))
                .child(save.mt(22.0)),
        );

    let pick = &ss.icon;
    let tile = icon_tile(a, k, guild, pick);
    let zone_fill = if pick.drop_over {
        t.accent_subtle.color()
    } else {
        t.hover.color()
    };
    let mut actions = row().key("server-icon-actions").gap(8.0).center_y();
    if pick.draft.is_some() {
        actions = actions
            .child(
                Button::new(if ss.icon_busy {
                    "Saving\u{2026}"
                } else {
                    "Save icon"
                })
                .disabled(ss.icon_busy || !editable)
                .on_press(|a: &mut App| a.act(|s| s.save_icon()))
                .build(k)
                .key("server-icon-save"),
            )
            .child(
                Button::new("Cancel")
                    .variant(Variant::Secondary)
                    .disabled(ss.icon_busy)
                    .on_press(|a: &mut App| a.act(|s| s.discard_icon()))
                    .build(k)
                    .key("server-icon-cancel"),
            );
    } else {
        actions = actions.child(
            Button::new(if pick.reading {
                "Reading\u{2026}"
            } else {
                "Upload image\u{2026}"
            })
            .icon(&IMAGE)
            .variant(Variant::Secondary)
            .disabled(!editable || pick.reading || ss.icon_busy)
            .build(k)
            .key("server-icon-upload")
            .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                choose_image(tx, ImagePurpose::GuildIcon { guild })
            }),
        );
        if a.state.guild(guild).is_some_and(|g| g.icon.is_some()) {
            actions = actions.child(
                Button::new(if ss.icon_busy {
                    "Removing\u{2026}"
                } else {
                    "Remove"
                })
                .variant(Variant::Ghost)
                .icon(&TRASH)
                .disabled(!editable || ss.icon_busy)
                .on_press(|a: &mut App| a.act(|s| s.remove_icon()))
                .build(k)
                .key("server-icon-remove"),
            );
        }
    }
    let zone = row()
        .key("server-icon-zone")
        .w_full()
        .gap(16.0)
        .padding(12.0)
        .center_y()
        .radius(radius::LG)
        .border(
            1.0,
            if pick.drop_over {
                t.accent.color()
            } else {
                rune::Color::TRANSPARENT
            },
        )
        .fill(zone_fill)
        .child(tile)
        .child(
            col()
                .gap(8.0)
                .grow(1.0)
                .shrink()
                .child(actions)
                .child(
                    text("PNG, JPEG, WebP or GIF, up to 8 MiB. Drop a file here or choose one.")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color())
                        .wrap(),
                )
                .children(errors(k, &ss.icon_error, "server-icon-error")),
        )
        .on_drop_files(move |a: &mut App, files, _tx| {
            if let Some(path) = files.first() {
                a.act(|s| s.pick_image(ImagePurpose::GuildIcon { guild }, path));
            }
        })
        .accepts_extensions(IMAGE_EXTENSIONS)
        .on_drop_hover(|a: &mut App, over, _tx| {
            if let Some(ss) = a.state.ui.server_settings.as_mut() {
                ss.icon.drop_over = over;
            }
        });

    page_col(k, "Overview")
        .child(setting(
            k,
            "Name",
            "What people see at the top of the sidebar and in their server list.",
            name_field,
        ))
        .child(setting(
            k,
            "Icon",
            "A picture for the server rail. Without one the server shows its initials.",
            zone,
        ))
        .children(danger_zone(a, k, guild))
}

/// The owner's last section: hand the server over, or delete it. Each opens its own dialog,
/// which asks for the server's name.
fn danger_zone(a: &App, k: &Kit, guild: u64) -> Option<El> {
    if !a.state.can_delete_guild(guild) {
        return None;
    }
    let t = theme_of(k);
    let transfer = Button::new("Transfer\u{2026}")
        .variant(Variant::Secondary)
        .on_press(move |a: &mut App| a.act(|s| s.open_dialog(DialogKind::TransferOwnership { guild })))
        .build(k)
        .key("server-transfer");
    let delete = Button::new("Delete server\u{2026}")
        .variant(Variant::Danger)
        .on_press(move |a: &mut App| a.act(|s| s.open_dialog(DialogKind::DeleteGuild { guild })))
        .build(k)
        .key("server-delete");
    Some(
        col()
            .key("danger-zone")
            .w_full()
            .mt(16.0)
            .gap(8.0)
            .child(text("Danger zone").font(type_::at(k, type_::CONTROL_STRONG)).color(t.danger.color()))
            .child(
                col()
                    .w_full()
                    .px(16.0)
                    .py(4.0)
                    .radius(radius::LG)
                    .border(1.0, t.danger_border.color())
                    .child(toggle(k, "Transfer ownership", "Hand this server to another member. You become an ordinary member.", transfer))
                    .child(toggle(k, "Delete server", "Delete the server and everything in it for everyone. This can't be undone.", delete)),
            ),
    )
}

fn name_edit(a: &mut App) -> &mut rune::TextEdit {
    match a.state.ui.server_settings.as_mut() {
        Some(s) => &mut s.name,
        None => &mut a.state.ui.idle_composer,
    }
}

fn search_edit(a: &mut App) -> &mut rune::TextEdit {
    match a.state.ui.server_settings.as_mut() {
        Some(s) => &mut s.search,
        None => &mut a.state.ui.idle_composer,
    }
}

fn role_name_edit(a: &mut App) -> &mut rune::TextEdit {
    match a
        .state
        .ui
        .server_settings
        .as_mut()
        .and_then(|s| s.form.as_mut())
    {
        Some(f) => &mut f.name,
        None => &mut a.state.ui.idle_composer,
    }
}

// ---- roles -----------------------------------------------------------------------------------

/// A role's colour dot: its palette tone, or a hollow ring for no colour.
pub fn role_dot(k: &Kit, color: PaletteColor, size: f32) -> El {
    let t = theme_of(k);
    match palette_color(color.as_key(), t) {
        Some(tone) => el()
            .size(size, size)
            .radius(radius::FULL)
            .fill(tone.color())
            .a11y_hidden(),
        None => el()
            .size(size, size)
            .radius(radius::FULL)
            .border(1.5, t.border_strong.color())
            .a11y_hidden(),
    }
}

fn roles(a: &App, k: &Kit, ss: &ServerSettings) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let can = a.state.can_manage_roles(guild);
    let list = a.state.roles_of(guild);

    let mut rows = col().key("role-list").w_full().gap(ROLE_PITCH - ROLE_ROW_H);
    let selected = ss.form.as_ref().and_then(|f| f.role);
    let busy = a.state.data.role_undo.contains_key(&guild);
    for (i, r) in list.iter().enumerate() {
        rows = rows.child(role_row(
            a,
            k,
            ss,
            r,
            i,
            list.len(),
            selected == Some(r.id),
            busy,
        ));
    }
    if list.is_empty() {
        rows = rows.child(
            text("No roles yet. Create one to give members a colour and permissions.")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .wrap()
                .w_full(),
        );
    }
    let new = Button::new("New role")
        .icon(&PLUS)
        .size(Size::Sm)
        .disabled(!can)
        .on_press(|a: &mut App| a.act(|s| s.new_role()))
        .build(k)
        .key("role-new");
    let left = col()
        .key("roles-left")
        .w(240.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text("Roles".to_owned()).font(type_::at(k, type_::CONTROL_STRONG)).color(t.text.color()).grow(1.0))
                .child(new),
        )
        .child(text("The higher a role, the more it outranks. A member's name takes the colour of their highest role that has one.").font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).wrap().w_full())
        .children(errors(k, &ss.role_error, "role-error"))
        .child(rows);

    let right = match &ss.form {
        Some(f) => editor(a, k, ss, f),
        None => col()
            .key("role-editor-empty")
            .grow(1.0)
            .min_w(0.0)
            .h(160.0)
            .center()
            .gap(6.0)
            .child(
                icon(&SHIELD, isize::MD)
                    .color(t.text_faint.color())
                    .a11y_hidden(),
            )
            .child(
                text("Pick a role to edit it, or create a new one.")
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color()),
            ),
    };
    page_col(k, "Roles").child(
        row()
            .key("roles-body")
            .w_full()
            .gap(20.0)
            .pt(16.0)
            .items_start()
            .child(left)
            .child(right),
    )
}

#[allow(clippy::too_many_arguments)]
fn role_row(
    a: &App,
    k: &Kit,
    ss: &ServerSettings,
    r: &Role,
    index: usize,
    count: usize,
    selected: bool,
    busy: bool,
) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let s = &a.state;
    let id = r.id;
    let mine = s.can_edit_role(guild, r);
    let members = s.role_members(guild, id);
    let dragging = ss.drag.filter(|d| d.guild == guild);
    let mut line = row()
        .key(format!("role-row-{id}"))
        .role("button")
        .label(format!("{}, {} members", r.name, members))
        .focus_selected(selected)
        .w_full()
        .h(ROLE_ROW_H)
        .pl(10.0)
        .pr(4.0)
        .gap(10.0)
        .center_y()
        .radius(radius::MD)
        .focusable(true)
        .focus_ring(space::RING_WIDTH, space::RING_INSET, k.c.border_focus)
        .on_press(move |a: &mut App| a.act(|s| s.select_role(id)))
        .child(role_dot(k, r.color, 12.0))
        .child(
            text(r.name.clone())
                .font(if selected {
                    type_::CONTROL_STRONG
                } else {
                    type_::CONTROL
                })
                .color(if mine { t.text } else { t.text_muted }.color())
                .max_lines(1)
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        );
    line = if selected {
        line.fill(t.accent_subtle.color())
    } else {
        line.hover(t.hover.color()).press(t.pressed.color())
    };
    line = line.child(
        text(members.to_string())
            .font(type_::at(k, type_::CAPTION))
            .color(t.text_muted.color())
            .a11y_hidden(),
    );
    if mine {
        let can_up = s.can_move_role(guild, id, -1);
        let can_down = s.can_move_role(guild, id, 1);
        line = line
            .child(
                IconButton::new(&ARROW_UP, format!("Move {} up", r.name))
                    .size(Size::Sm)
                    .disabled(!can_up || busy)
                    .on_press(move |a: &mut App| a.act(|s| {
                    let _ = s.step_role(guild, id, -1);
                }))
                    .build(k)
                    .key(format!("role-up-{id}")),
            )
            .child(
                IconButton::new(&ARROW_DOWN, format!("Move {} down", r.name))
                    .size(Size::Sm)
                    .disabled(!can_down || busy)
                    .on_press(move |a: &mut App| a.act(|s| {
                    let _ = s.step_role(guild, id, 1);
                }))
                    .build(k)
                    .key(format!("role-down-{id}")),
            );
        line = line
            .on_drag(move |a: &mut App, d: &mut Drag, tx: &mut Tx| role_drag(a, guild, id, d, tx));
    } else {
        line = line.child(
            icon(&LOCK, isize::SM)
                .color(t.text_faint.color())
                .label("Above your highest role")
                .key(format!("role-lock-{id}")),
        );
    }
    // Held rows follow the pointer; the rows it passes spring aside.
    let (shift, lifted) = role_shift(a, ss, index, count, id, dragging);
    if shift != 0.0 {
        line = line.offset(0.0, shift);
    }
    if lifted {
        line = k.elevate(
            line.raise().offset_motion(Anim::INSTANT).fill(
                thehideout_design::components::util::over(t.pressed.color(), t.surface.color()),
            ),
            &k.elevation.raised,
        );
    }
    line
}

fn role_drag(a: &mut App, guild: u64, role: u64, d: &mut Drag, tx: &mut Tx) {
    match d.phase {
        Phase::Down => {}
        Phase::Move => {
            if a.state
                .ui
                .server_settings
                .as_ref()
                .is_some_and(|s| s.drag.is_some())
                || d.dy.abs() >= SLOP
            {
                tx.animated();
                a.state.drag_role(guild, role, d.dy);
            }
        }
        Phase::Up => {
            let cancelled = d.cancelled();
            tx.animated();
            a.act(|s| s.end_role_drag(cancelled));
        }
    }
}

/// How far row `id` sits from its place while another is dragged, and whether it is the one held.
fn role_shift(
    a: &App,
    _ss: &ServerSettings,
    index: usize,
    _count: usize,
    id: u64,
    drag: Option<crate::server_settings::RoleDrag>,
) -> (f32, bool) {
    let Some(d) = drag else { return (0.0, false) };
    if d.role == id {
        return (d.dy, true);
    }
    let roles = a.state.roles_of(d.guild);
    let (Some(from), Some(to)) = (
        roles.iter().position(|r| r.id == d.role),
        a.state.role_drop_slot(),
    ) else {
        return (0.0, false);
    };
    let shift = if from < index && index <= to {
        -ROLE_PITCH
    } else if to <= index && index < from {
        ROLE_PITCH
    } else {
        0.0
    };
    (shift, false)
}

fn editor(a: &App, k: &Kit, ss: &ServerSettings, f: &RoleForm) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let s = &a.state;
    let editable = s.form_editable();
    let creating = f.role.is_none();
    let id = f.role.unwrap_or(0);

    let name = TextInput::new(&f.name)
        .placeholder("New role")
        .disabled(!editable)
        .on_edit(role_name_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.save_role()));
    let mut swatches = ColorSwatches::new(f.color.as_key());
    if editable {
        swatches =
            swatches.on_pick(|a: &mut App, key: &'static str| a.act(|s| s.set_role_color(key)));
    }
    let mut colours = swatches.build(k);
    if !editable {
        colours = colours.layer_opacity(0.5).inert();
    }

    let mut perms = col().key("role-perms").w_full().gap(2.0);
    for p in PERMISSION_ROWS.iter().chain(&crate::voice::VOICE_PERMISSION_ROWS) {
        let on = has_bit(f.perms, p.bit);
        let holds = s.can_grant(guild, p.bit);
        let bit: Permissions = p.bit;
        let admin = bit == Permissions::ADMINISTRATOR;
        let sw = switch(on)
            .label(p.name)
            .hide_label()
            .disabled(!editable || !holds)
            .on_toggle(move |a: &mut App| a.act(|s| s.toggle_permission(bit)))
            .build(k)
            .key(format!("perm-{}", p.name.to_lowercase().replace(' ', "-")));
        let help = if !holds && editable {
            format!("{} You don't have this permission yourself.", p.help)
        } else {
            p.help.to_owned()
        };
        let mut labels = col()
            .grow(1.0)
            .shrink()
            .gap(2.0)
            .child(
                text(p.name.to_owned())
                    .font(type_::at(k, type_::CONTROL_STRONG))
                    .color(t.text.color()),
            )
            .child(
                text(help)
                    .font(type_::at(k, type_::CAPTION))
                    .color(t.text_muted.color())
                    .wrap()
                    .w_full(),
            );
        if admin && on {
            labels = labels.child(
                row()
                    .key("admin-warning")
                    .gap(6.0)
                    .center_y()
                    .child(
                        icon(&icons::WARNING, isize::XS)
                            .color(t.warning.color())
                            .a11y_hidden(),
                    )
                    .child(
                        text("This role can do everything on this server.")
                            .font(type_::at(k, type_::CAPTION))
                            .color(t.warning.color())
                            .wrap(),
                    ),
            );
        }
        perms = perms.child(
            row()
                .w_full()
                .gap(12.0)
                .py(8.0)
                .items_start()
                .child(labels)
                .child(sw),
        );
    }

    let dirty = f.dirty();
    let save = Button::new(match (f.busy, creating) {
        (true, true) => "Creating\u{2026}",
        (true, false) => "Saving\u{2026}",
        (false, true) => "Create role",
        (false, false) => "Save changes",
    })
    .disabled(!editable || f.busy || !dirty)
    .on_press(|a: &mut App| a.act(|s| s.save_role()))
    .build(k)
    .key("role-save");
    let mut footer = row().w_full().gap(8.0).center_y().justify_end();
    if !creating {
        footer = footer.child(
            Button::new("Delete role")
                .variant(Variant::Danger)
                .icon(&TRASH)
                .disabled(!editable || f.busy)
                .on_press(move |a: &mut App| a.act(|s| s.ask_delete_role(id)))
                .build(k)
                .key("role-delete"),
        );
        footer = footer.child(el().grow(1.0));
    }
    footer = footer
        .child(
            Button::new(if dirty && !creating {
                "Discard"
            } else {
                "Close"
            })
            .variant(Variant::Secondary)
            .on_press(|a: &mut App| a.act(|s| s.close_role_form()))
            .build(k)
            .key("role-close"),
        )
        .child(save);

    let mut c = col()
        .key("role-editor")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(16.0)
        .padding(16.0)
        .radius(radius::LG)
        .fill(t.bg.color());
    if !editable && !creating {
        c = c.child(
            row()
                .key("role-readonly")
                .gap(8.0)
                .center_y()
                .child(
                    icon(&LOCK, isize::SM)
                        .color(t.text_muted.color())
                        .a11y_hidden(),
                )
                .child(
                    text("This role is at or above your highest role, so you can only look at it.")
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .wrap()
                        .grow(1.0)
                        .shrink(),
                ),
        );
    }
    c.child(dialog_field(k, "Role name", name, None))
        .child(
            col()
                .key("role-colour")
                .w_full()
                .gap(6.0)
                .child(
                    text("Colour")
                        .font(type_::at(k, type_::LABEL))
                        .color(t.text_muted.color()),
                )
                .child(colours),
        )
        .child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    text("Permissions")
                        .font(type_::at(k, type_::LABEL))
                        .color(t.text_muted.color()),
                )
                .child(perms),
        )
        .children(errors(k, &f.error, "role-form-error"))
        .child(footer)
}

// ---- members ---------------------------------------------------------------------------------

fn members(a: &App, k: &Kit, layer: &mut Vec<El>, ss: &ServerSettings) -> El {
    let t = theme_of(k);
    let guild = ss.guild;
    let search = TextInput::new(&ss.search)
        .placeholder("Search members")
        .label("Search members")
        .leading_icon(&SEARCH)
        .large()
        .width(None)
        .on_edit(search_edit);
    let ids = a.state.searched_members(guild);
    let mut list = col().key("member-roles-list").w_full();
    for id in &ids {
        list = list.child(member_row(a, k, layer, ss, *id));
    }
    if ids.is_empty() {
        list = list.child(
            text(if ss.search.text.trim().is_empty() {
                "No members yet."
            } else {
                "No one matches that."
            })
            .font(type_::at(k, type_::CONTROL))
            .color(t.text_muted.color())
            .py(16.0),
        );
    }
    page_col(k, "Members")
        .child(col().w_full().gap(12.0).py(8.0).child(text(if a.state.can_manage_roles(guild) { "Give people roles for a name colour and permissions. You can only hand out roles below your own highest role." } else { "Remove people from the server. You can only act on members whose roles are below yours." }).font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).wrap().w_full()).child(search.build(k)))
        .children(errors(k, &ss.member_error, "member-error"))
        .child(list)
}

fn chip(k: &Kit, role: &Role, removable: bool, guild: u64, user: u64) -> El {
    let t = theme_of(k);
    let mut c = row()
        .key(format!("chip-{}-{}", user, role.id))
        .h(24.0)
        .pl(8.0)
        .pr(if removable { 2.0 } else { 8.0 })
        .gap(6.0)
        .center_y()
        .radius(radius::SM)
        .fill(t.hover.color())
        .child(role_dot(k, role.color, 8.0))
        .child(
            text(role.name.clone())
                .font(type_::at(k, type_::CAPTION))
                .color(t.text.color())
                .max_lines(1),
        );
    if removable {
        let rid = role.id;
        c = c.child(
            el().key(format!("chip-x-{}-{}", user, role.id))
                .role("button")
                .label(format!("Remove {}", role.name))
                .size(20.0, 20.0)
                .center()
                .radius(radius::SM)
                .focusable(true)
                .focus_ring(space::RING_WIDTH, space::RING_OFFSET, k.c.border_focus)
                .hover(t.hover.color())
                .press(t.pressed.color())
                .on_press(move |a: &mut App| a.act(|s| s.set_member_role(guild, user, rid, false)))
                .child(
                    icon(&X, isize::XS)
                        .color(t.text_muted.color())
                        .a11y_hidden(),
                ),
        );
    }
    c
}

fn member_row(a: &App, k: &Kit, layer: &mut Vec<El>, ss: &ServerSettings, user: u64) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let guild = ss.guild;
    let name = s.user_name(user);
    let name_ink = palette_text(s.name_color(guild, user).as_key(), t)
        .map_or(t.text.color(), |tone| tone.color());
    let held: Vec<Role> = s
        .roles_of(guild)
        .iter()
        .filter(|r| s.member_role_ids(guild, user).contains(&r.id))
        .cloned()
        .collect();
    let assignable = s.assignable_roles(guild, user);
    let busy = ss.member_busy == Some(user);

    let mut chips = row()
        .key(format!("member-chips-{user}"))
        .flex_wrap()
        .gap(6.0)
        .center_y();
    for r in &held {
        chips = chips.child(chip(k, r, s.can_edit_role(guild, r) && !busy, guild, user));
    }
    let trigger = Button::new("Role")
        .icon(&PLUS)
        .size(Size::Sm)
        .variant(Variant::Secondary)
        .disabled(assignable.is_empty() || busy)
        .on_press(move |a: &mut App| a.act(|s| s.toggle_member_menu(user)))
        .build(k)
        .key(format!("member-add-{user}"));
    let can_roles = s.can_manage_roles(guild);
    let mut menu = Menu::new(format!("member-menu-{user}")).trigger(trigger);
    for r in &assignable {
        let rid = r.id;
        menu = menu.item(
            MenuItem::new(r.name.clone())
                .lead(role_dot(k, r.color, 10.0))
                .on_select(move |a: &mut App| a.act(|s| s.set_member_role(guild, user, rid, true))),
        );
    }
    let menu = menu
        .open(ss.member_menu == Some(user))
        .min_width(180.0)
        .on_close(|a: &mut App| {
            a.act(|s| {
                if let Some(ss) = s.ui.server_settings.as_mut() {
                    ss.member_menu = None;
                }
            })
        })
        .build(k, layer);
    if can_roles {
        chips = chips.child(menu);
    }
    let mut actions = row().key(format!("member-actions-{user}")).gap(6.0).h(32.0).center_y();
    if s.can_kick(guild, user) {
        actions = actions.child(
            Button::new("Kick")
                .icon(&icons::USER_MINUS)
                .size(Size::Sm)
                .variant(Variant::Secondary)
                .on_press(move |a: &mut App| a.act(|s| s.ask_kick(guild, user)))
                .build(k)
                .key(format!("member-kick-{user}")),
        );
    }
    if s.can_ban(guild, user) {
        actions = actions.child(
            Button::new("Ban\u{2026}")
                .icon(&icons::PROHIBIT)
                .size(Size::Sm)
                .variant(Variant::Secondary)
                .on_press(move |a: &mut App| a.act(|s| s.ask_ban(guild, user)))
                .build(k)
                .key(format!("member-ban-{user}")),
        );
    }

    row()
        .key(format!("member-roles-{user}"))
        .w_full()
        .py(10.0)
        .gap(12.0)
        .items_start()
        .opacity(if busy { 0.6 } else { 1.0 })
        .child(
            Avatar::new(user.to_string(), name.clone())
                .size(32)
                .image(s.avatar_pixels(user))
                .backdrop(t.surface.color())
                .build(k),
        )
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(6.0)
                .child(
                    row().h(32.0).center_y().child(
                        text(name)
                            .font(type_::at(k, type_::BODY_STRONG))
                            .color(name_ink)
                            .max_lines(1),
                    ),
                )
                .child(chips),
        )
        .child(actions)
}

// ---- bans ------------------------------------------------------------------------------------

fn bans(a: &App, k: &Kit, ss: &ServerSettings) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let guild = ss.guild;
    let loaded = s.data.bans.contains_key(&guild);
    let mut list = col().key("ban-list").w_full();
    for b in s.bans_of(guild) {
        let user = b.user.id;
        let name = s.user_name(user);
        let by = if b.banned_by == 0 { String::new() } else { format!("Banned by {}", s.user_name(b.banned_by)) };
        let day = crate::format::day_of(s.tz, crate::format::unix(b.created_at));
        let when = crate::format::day_label(day, s.today);
        let detail = if by.is_empty() { when } else { format!("{by} \u{b7} {when}") };
        let reason = if b.reason.trim().is_empty() {
            text("No reason given").font(type_::at(k, type_::CONTROL)).color(t.text_faint.color()).wrap().w_full()
        } else {
            text(b.reason.clone()).font(type_::at(k, type_::CONTROL)).color(t.text.color()).wrap().w_full()
        };
        list = list.child(
            row()
                .key(format!("ban-{user}"))
                .w_full()
                .py(10.0)
                .gap(12.0)
                .items_start()
                .child(
                    Avatar::new(user.to_string(), name.clone())
                        .size(32)
                        .image(s.avatar_pixels(user))
                        .backdrop(t.surface.color())
                        .build(k),
                )
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .gap(2.0)
                        .child(row().h(32.0).center_y().child(text(name).font(type_::at(k, type_::BODY_STRONG)).color(t.text.color()).max_lines(1)))
                        .child(reason)
                        .child(text(detail).font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).max_lines(1)),
                )
                .child(
                    row().h(32.0).center_y().child(
                        Button::new("Unban")
                            .size(Size::Sm)
                            .variant(Variant::Secondary)
                            .on_press(move |a: &mut App| a.act(|s| {
                                s.unban(guild, user);
                            }))
                            .build(k)
                            .key(format!("unban-{user}")),
                    ),
                ),
        );
    }
    if s.bans_of(guild).is_empty() {
        list = list.child(
            text(if loaded { "No one is banned." } else { "Loading\u{2026}" })
                .key("bans-empty")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .py(16.0),
        );
    }
    page_col(k, "Bans")
        .child(
            col().w_full().py(8.0).child(
                text("People who were banned can't join again until you unban them.")
                    .font(type_::at(k, type_::CAPTION))
                    .color(t.text_muted.color())
                    .wrap()
                    .w_full(),
            ),
        )
        .child(list)
}
