//! The account lifecycle's screens: the blocking "we updated our terms" card, the suspended card
//! the sign-in screen shows, the Account part of Settings → Security (export your data, the Danger
//! zone and its "Delete account" dialog), the admin's Suspend and Delete-user dialogs, and the
//! small legal links (Terms, Privacy) that sit under the auth card and in Settings.
//!
//! The data layer is `lifecycle.rs` (+ the state at the end of `state.rs`); nothing here owns state.

use rune::{col, el, row, text, El, TextEdit, Tx};
use rune_kit::{Kit, Menu, MenuItem, Select, SelectAppearance};
use thehideout_api::OwnedGuild;
use thehideout_design::components::{
    dialog_field, Button, Checkbox, CodeInput, Dialog, Size, TextInput, Variant,
};
use thehideout_design::icons;
use thehideout_design::{radius, theme_of, type_};

use super::settings::{setting, toggle};
use crate::app::App;
use crate::format::{day_label, day_of, unix};
use crate::forms::Suspension;
use crate::state::{AdminUserDialog, AppState, SUSPEND_DURATIONS};

/// The page of the server's web site that holds the terms / the privacy notice.
fn legal_url(a: &App, page: &str) -> String {
    format!("{}/{page}", a.state.server_url().trim_end_matches('/'))
}

/// A text link that opens `<server>/<page>` in the browser.
pub(super) fn legal_link(k: &Kit, key: &str, label: &str, page: &'static str, accent: bool) -> El {
    let t = theme_of(k);
    let color = if accent { t.accent_text } else { t.text_faint };
    text(label.to_owned())
        .key(key.to_owned())
        .font(type_::at(k, type_::CAPTION))
        .color(color.color())
        .role("link")
        .label(label.to_owned())
        .focusable(true)
        .focus_ring(2.0, 2.0, k.c.border_focus)
        .radius(radius::XS)
        .hover(t.hover.color())
        .on_press_tx(move |a: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(&legal_url(a, page));
        })
}

/// "Terms · Privacy", faint, for under the auth card and the Settings nav.
pub(super) fn legal_links(k: &Kit, prefix: &str) -> El {
    let t = theme_of(k);
    row()
        .key(format!("{prefix}-legal"))
        .gap(4.0)
        .center()
        .child(legal_link(
            k,
            &format!("{prefix}-terms"),
            "Terms",
            "terms",
            false,
        ))
        .child(
            text("\u{00b7}")
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_faint.color()),
        )
        .child(legal_link(
            k,
            &format!("{prefix}-privacy"),
            "Privacy",
            "privacy",
            false,
        ))
}

/// What the footer of the Settings nav says: the version and the legal links.
pub(super) fn about(k: &Kit) -> El {
    let t = theme_of(k);
    col()
        .key("settings-about")
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(4.0)
        .child(
            text(format!("thehideout {}", env!("CARGO_PKG_VERSION")))
                .key("about-version")
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color()),
        )
        .child(
            row()
                .gap(4.0)
                .child(legal_link(k, "about-terms", "Terms", "terms", false))
                .child(
                    text("\u{00b7}")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_faint.color()),
                )
                .child(legal_link(k, "about-privacy", "Privacy", "privacy", false)),
        )
}

fn muted(k: &Kit, s: impl Into<String>) -> El {
    text(s.into())
        .font(type_::at(k, type_::CAPTION))
        .color(theme_of(k).text_muted.color())
        .wrap()
        .w_full()
}

fn error_line(k: &Kit, key: &str, s: impl Into<String>) -> El {
    text(s.into())
        .key(key.to_owned())
        .font(type_::at(k, type_::CONTROL))
        .color(theme_of(k).danger.color())
        .wrap()
        .w_full()
}

/// "Fri, Jan 1, 2027"; "today" or "yesterday" for those.
fn date_text(s: &AppState, at: std::time::SystemTime) -> String {
    let label = day_label(day_of(s.tz, unix(at)), s.today);
    match label.as_str() {
        "Today" => "today".to_owned(),
        "Yesterday" => "yesterday".to_owned(),
        _ => label,
    }
}

// ---- the terms prompt -----------------------------------------------------------------------------

/// The blocking card: nothing of the app is reachable until the new terms are accepted.
pub fn terms_screen(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let lc = &a.state.ui.lifecycle;
    let busy = lc.terms_busy;
    let mut card = col()
        .key("terms-card")
        .w(400.0)
        .max_w(400.0)
        .padding(32.0)
        .gap(16.0)
        .radius(radius::LG)
        .fill(t.surface.color())
        .border(1.0, t.border_subtle.color())
        .child(
            text("thehideout")
                .font(type_::at(k, type_::HEADING))
                .color(t.accent_text.color()),
        )
        .child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    text("We updated our Terms and Privacy Policy")
                        .font(type_::at(k, type_::TITLE))
                        .color(t.text.color())
                        .wrap()
                        .w_full(),
                )
                .child(
                    text("Please read them, then accept to keep using thehideout. Nothing else changes: your servers and messages are as you left them.")
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .wrap()
                        .w_full(),
                ),
        )
        .child(
            row()
                .key("terms-links")
                .gap(16.0)
                .child(
                    legal_link(k, "terms-read-terms", "Read the Terms", "terms", true)
                        .font(type_::at(k, type_::CONTROL)),
                )
                .child(
                    legal_link(k, "terms-read-privacy", "Read the Privacy Policy", "privacy", true)
                        .font(type_::at(k, type_::CONTROL)),
                ),
        );
    if let Some(e) = &lc.terms_error {
        card = card.child(error_line(k, "terms-error", e.clone()));
    }
    card = card
        .child(
            Button::new(if busy {
                "Accepting\u{2026}"
            } else {
                "Accept and continue"
            })
            .disabled(busy)
            .on_press(|a: &mut App| a.act(|s| s.accept_terms()))
            .build(k)
            .key("terms-accept")
            .w_full(),
        )
        .child(
            row().w_full().justify_center().child(
                text("Sign out")
                    .key("terms-sign-out")
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .role("button")
                    .label("Sign out")
                    .focusable(true)
                    .focus_ring(2.0, 2.0, k.c.border_focus)
                    .radius(radius::XS)
                    .hover(t.hover.color())
                    .on_press(|a: &mut App| a.act(|s| s.sign_out())),
            ),
        );
    col()
        .key("terms")
        .w_full()
        .h_full()
        .center()
        .gap(16.0)
        .fill(t.bg.color())
        .child(card)
}

// ---- suspended ------------------------------------------------------------------------------------

/// The auth card's content when the sign-in was refused because the account is suspended.
pub fn suspended_card(a: &App, k: &Kit, s: &Suspension) -> Vec<El> {
    let t = theme_of(k);
    let mut out = vec![text("Your account is suspended")
        .key("suspended-title")
        .font(type_::at(k, type_::TITLE))
        .color(t.text.color())
        .wrap()
        .w_full()];
    let reason = s.reason.trim();
    if !reason.is_empty() {
        out.push(
            col()
                .key("suspended-reason")
                .w_full()
                .gap(2.0)
                .padding(12.0)
                .radius(radius::MD)
                .fill(t.danger_bg.color())
                .child(
                    text("Reason")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color()),
                )
                .child(
                    text(reason.to_owned())
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color())
                        .wrap()
                        .w_full(),
                ),
        );
    }
    let until = match s.until {
        Some(at) => format!("Your access is paused until {}.", date_text(&a.state, at)),
        None => "Your access is paused indefinitely, until an admin lifts the suspension.".to_owned(),
    };
    out.push(
        text(until)
            .key("suspended-until")
            .font(type_::at(k, type_::CONTROL))
            .color(t.text.color())
            .wrap()
            .w_full(),
    );
    out.push(
        muted(
            k,
            "If you think this is a mistake, contact the server admin.",
        )
        .key("suspended-hint"),
    );
    out.push(
        Button::new("Back to sign in")
            .variant(Variant::Secondary)
            .on_press(|a: &mut App| a.act(|s| s.dismiss_suspension()))
            .build(k)
            .key("suspended-back")
            .w_full(),
    );
    out
}

// ---- Settings → Security: export and delete -------------------------------------------------------

/// Export your data, then the Danger zone with "Delete account".
pub fn security_section(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let lc = &a.state.ui.lifecycle;
    let mut export = col().key("account-export").w_full().gap(8.0);
    let done = lc.export_sent_to.is_some();
    export = export.child(
        row().child(
            Button::new(match (done, lc.export_busy) {
                (true, _) => "Export requested, check your email",
                (false, true) => "Requesting\u{2026}",
                (false, false) => "Export your data",
            })
            .variant(Variant::Secondary)
            .disabled(done || lc.export_busy)
            .on_press(|a: &mut App| a.act(|s| s.export_data()))
            .build(k)
            .key("account-export-button"),
        ),
    );
    if let Some(address) = &lc.export_sent_to {
        export = export.child(
            muted(k, format!("The link is on its way to {address}. It works for 7 days. You can ask for another export in 24 hours."))
                .key("account-export-sent"),
        );
    }
    if let Some(e) = &lc.export_error {
        let limit = e.contains("24 hours");
        export = export.child(
            text(e.clone())
                .key("account-export-error")
                .font(type_::at(k, type_::CAPTION))
                .color(if limit { t.text_muted } else { t.danger }.color())
                .wrap()
                .w_full(),
        );
    }
    let delete = Button::new("Delete account\u{2026}")
        .variant(Variant::Danger)
        .on_press(|a: &mut App| a.act(|s| s.open_delete_account()))
        .build(k)
        .key("account-delete");
    let danger = col()
        .key("account-danger-zone")
        .w_full()
        .mt(16.0)
        .gap(8.0)
        .child(
            text("Danger zone")
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(t.danger.color()),
        )
        .child(
            col()
                .w_full()
                .px(16.0)
                .py(4.0)
                .radius(radius::LG)
                .border(1.0, t.danger_border.color())
                .child(toggle(
                    k,
                    "Delete account",
                    "Your messages stay, shown as \u{201c}Deleted user\u{201d}. Everything else about you is removed. This can't be undone.",
                    delete,
                )),
        );
    col()
        .key("account-section")
        .w_full()
        .child(setting(
            k,
            "Export your data",
            "A copy of your account data as a JSON file: your profile, the servers you're in and your messages. We email a download link to your address when it's ready.",
            export,
        ))
        .child(danger)
}

// ---- the Delete account dialog --------------------------------------------------------------------

fn password_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.account.delete.as_mut() {
        Some(d) => &mut d.password,
        None => &mut a.state.ui.idle_composer,
    }
}
fn email_code_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.account.delete.as_mut() {
        Some(d) => &mut d.email_code,
        None => &mut a.state.ui.idle_composer,
    }
}
fn totp_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.account.delete.as_mut() {
        Some(d) => &mut d.totp,
        None => &mut a.state.ui.idle_composer,
    }
}

fn close_delete(a: &mut App) {
    a.act(|s| s.close_delete_account());
}

fn submit_delete(a: &mut App, _tx: &mut Tx) {
    a.act(|s| s.submit_delete_account());
}

fn code_cells(
    k: &Kit,
    key: &str,
    label: &str,
    edit: &TextEdit,
    invalid: bool,
    busy: bool,
    field: fn(&mut App) -> &mut TextEdit,
) -> El {
    let t = theme_of(k);
    col()
        .key(key.to_owned())
        .w_full()
        .gap(6.0)
        .child(
            text(label.to_owned())
                .font(type_::at(k, type_::LABEL))
                .color(t.text_muted.color()),
        )
        .child(
            CodeInput::new(edit)
                .label(label)
                .invalid(invalid)
                .disabled(busy)
                .on_edit(field)
                .on_submit(submit_delete)
                .build(k),
        )
}

/// The servers that block a deletion, each with a way to its Overview page.
fn owned_list(k: &Kit, guilds: &[OwnedGuild], what: &str, openable: bool) -> El {
    let t = theme_of(k);
    let mut list = col()
        .key("delete-owned")
        .w_full()
        .gap(8.0)
        .padding(12.0)
        .radius(radius::MD)
        .fill(t.danger_bg.color());
    list = list
        .child(
            text(if what == "You" {
                format!("You still own {} server{}", guilds.len(), if guilds.len() == 1 { "" } else { "s" })
            } else {
                format!("{what} owns {} server{}", guilds.len(), if guilds.len() == 1 { "" } else { "s" })
            })
            .font(type_::at(k, type_::CONTROL_STRONG))
            .color(t.text.color()),
        )
        .child(muted(
        k,
        if openable {
            "Transfer or delete these servers first."
        } else {
            "They have to be handed over or deleted first."
        },
    ));
    for g in guilds {
        let id = g.id;
        list = list.child(
            row()
                .key(format!("delete-owned-row-{id}"))
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(
                            text(g.name.clone())
                                .font(type_::at(k, type_::CONTROL_STRONG))
                                .color(t.text.color())
                                .max_lines(1),
                        )
                        .child(
                            text(format!(
                                "{} member{}",
                                g.member_count,
                                if g.member_count == 1 { "" } else { "s" }
                            ))
                            .font(type_::at(k, type_::CAPTION))
                            .color(t.text_muted.color()),
                        ),
                )
                .children(openable.then(|| {
                    Button::new("Open settings")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .on_press(move |a: &mut App| a.act(|s| s.open_owned_server(id)))
                        .build(k)
                        .key(format!("delete-owned-open-{id}"))
                })),
        );
    }
    list
}

/// Called from the window's view, with the other dialog layers.
pub fn dialogs(a: &App, k: &Kit, layer: &mut Vec<El>, window_h: f32) {
    delete_dialog(a, k, layer, window_h);
}

fn delete_dialog(a: &App, k: &Kit, layer: &mut Vec<El>, window_h: f32) {
    let Some(d) = &a.state.ui.account.delete else {
        return;
    };
    let t = theme_of(k);
    let s = &a.state;
    let lc = &s.ui.lifecycle;
    let busy = lc.delete_busy;
    let owns = !lc.owned_guilds.is_empty();
    let mut body = col().key("delete-body").w_full().gap(16.0);
    if owns {
        body = body.child(owned_list(k, &lc.owned_guilds, "You", true));
    }
    if s.passwordless() {
        let sent = lc.deletion_code_sent;
        let send = Button::new(match (sent, lc.deletion_code_busy) {
            (_, true) => "Sending\u{2026}",
            (true, false) => "Send again",
            (false, false) => "Email me a code",
        })
        .variant(Variant::Secondary)
        .size(Size::Sm)
        .disabled(lc.deletion_code_busy || busy)
        .on_press(|a: &mut App| a.act(|s| s.request_deletion_code()))
        .build(k)
        .key("delete-send-code");
        body = body.child(
            col()
                .key("delete-code-ask")
                .w_full()
                .gap(8.0)
                .child(muted(k, if sent {
                    "We emailed you a 6-digit code. It works for 10 minutes."
                } else {
                    "Your account has no password, so we'll email you a code to confirm it's you."
                }))
                .child(row().child(send)),
        );
        if sent {
            body = body.child(code_cells(
                k,
                "delete-email-code",
                "Code from your email",
                &d.email_code,
                lc.delete_error.is_some() && !owns,
                busy,
                email_code_edit,
            ));
        }
    } else {
        let input = TextInput::new(&d.password)
            .secure()
            .label("Password")
            .width(None)
            .invalid(lc.delete_error.is_some() && !owns)
            .disabled(busy)
            .on_edit(password_edit)
            .on_submit(submit_delete);
        body = body.child(dialog_field(k, "Your password", input, None).w_full());
    }
    if s.has_totp() {
        body = body.child(code_cells(
            k,
            "delete-totp",
            "Authenticator code",
            &d.totp,
            false,
            busy,
            totp_edit,
        ));
    }
    if !owns && let Some(e) = &lc.delete_error {
        body = body.child(error_line(k, "delete-error", e.clone()));
    }
    let ready = s.delete_ready();
    let dlg = Dialog::new("delete-account-dlg", "Delete your account")
        .description("This can't be undone. Your messages stay, shown as \u{201c}Deleted user\u{201d}. Everything else is removed: your profile and picture, server memberships, sign-ins and passkeys.")
        .open(true)
        .body_max_h((window_h - 230.0).max(160.0))
        .child(body)
        .footer(
            Button::new("Cancel")
                .variant(Variant::Secondary)
                .disabled(busy)
                .on_press(close_delete)
                .build(k)
                .key("delete-cancel"),
        )
        .footer(
            Button::new(if busy { "Deleting\u{2026}" } else { "Delete account" })
                .variant(Variant::Danger)
                .disabled(busy || !ready)
                .on_press(|a: &mut App| a.act(|s| s.submit_delete_account()))
                .build(k)
                .key("delete-submit"),
        )
        .on_close(close_delete);
    let _ = t;
    dlg.build(k, layer);
}

// ---- admin: suspend and delete a user -------------------------------------------------------------

fn suspend_reason_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.account.admin_dialog.as_mut() {
        Some(AdminUserDialog::Suspend { reason, .. })
        | Some(AdminUserDialog::Delete { reason, .. }) => reason,
        None => &mut a.state.ui.idle_composer,
    }
}

fn cancel_admin(a: &mut App) {
    a.act(|s| s.cancel_admin_user_dialog());
}

/// The open actions menu of a Users row: Suspend (or Unsuspend) and Delete user. It anchors to
/// the row's button (`user-menu-<id>-trigger`), which lives in a virtual list that has no layer.
pub fn admin_user_menu(a: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(user) = a.state.ui.account.admin_menu else { return };
    let Some(r) = a.state.admin().and_then(|ad| ad.users.items.iter().find(|r| r.user.id == user)) else { return };
    let mut menu = Menu::new(format!("user-menu-{user}")).trigger(el());
    menu = if r.suspended {
        menu.item(
            MenuItem::new("Unsuspend")
                .icon(&icons::CHECK)
                .on_select(move |a: &mut App| {
                    a.act(|s| {
                        s.close_admin_menu();
                        s.admin_unsuspend(user)
                    })
                }),
        )
    } else {
        menu.item(
            MenuItem::new("Suspend\u{2026}")
                .icon(&icons::PROHIBIT)
                .on_select(move |a: &mut App| a.act(|s| s.ask_admin_suspend(user))),
        )
    };
    menu.item(
        MenuItem::new("Delete user\u{2026}")
            .icon(&icons::TRASH)
            .danger(true)
            .on_select(move |a: &mut App| a.act(|s| s.ask_admin_delete(user))),
    )
    .open(true)
    .on_close(|a: &mut App| a.act(|s| s.close_admin_menu()))
    .min_width(180.0)
    .build(k, layer);
}

/// The admin's Suspend and Delete-user dialogs (called from the admin page).
pub fn admin_dialogs(a: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(dialog) = &a.state.ui.account.admin_dialog else {
        return;
    };
    let t = theme_of(k);
    let s = &a.state;
    let user = dialog.user();
    let name = s
        .admin()
        .and_then(|ad| ad.users.items.iter().find(|r| r.user.id == user))
        .map_or_else(|| format!("user {user}"), |r| r.user.username.clone());
    let busy = s.admin().is_some_and(|ad| ad.busy_users.contains(&user));
    let error = s.admin().and_then(|ad| ad.user_errors.get(&user)).cloned();
    match dialog {
        AdminUserDialog::Suspend {
            reason,
            duration,
            open,
            ..
        } => {
            let input = TextInput::new(reason)
                .placeholder("Why is this account being suspended?")
                .large()
                .on_edit(suspend_reason_edit)
                .on_submit(|a: &mut App, _| a.act(|s| s.submit_admin_user_dialog()));
            let labels: Vec<&str> = SUSPEND_DURATIONS.iter().map(|d| d.0).collect();
            let select = Select::new("suspend-duration", &labels)
                .appearance(SelectAppearance::Field)
                .width(392.0)
                .label("For how long")
                .current(labels.get(*duration).copied())
                .open(*open)
                .on_toggle(|a: &mut App| {
                    if let Some(AdminUserDialog::Suspend { open, .. }) =
                        a.state.ui.account.admin_dialog.as_mut()
                    {
                        *open = !*open;
                    }
                })
                .on_close(|a: &mut App| {
                    if let Some(AdminUserDialog::Suspend { open, .. }) =
                        a.state.ui.account.admin_dialog.as_mut()
                    {
                        *open = false;
                    }
                })
                .on_choose(|a: &mut App, label: &str, _tx| {
                    if let Some(AdminUserDialog::Suspend { duration, open, .. }) =
                        a.state.ui.account.admin_dialog.as_mut()
                    {
                        *duration = SUSPEND_DURATIONS
                            .iter()
                            .position(|d| d.0 == label)
                            .unwrap_or(0);
                        *open = false;
                    }
                });
            let mut body = col()
                .key("suspend-body")
                .w_full()
                .gap(16.0)
                .child(dialog_field(k, "Reason", input, None).w_full())
                .child(col().key("suspend-duration-field").w_full().child(select.build(k, layer)))
                .child(muted(k, "The reason is shown to them when they try to sign in, and it's recorded in the audit log."));
            if let Some(e) = error {
                body = body.child(error_line(k, "suspend-error", e));
            }
            Dialog::new("admin-suspend-dlg", format!("Suspend {name}"))
                .description("They're signed out of every device and can't sign in until the suspension ends or you lift it.")
                .open(true)
                .child(body)
                .footer(
                    Button::new("Cancel")
                        .variant(Variant::Secondary)
                        .disabled(busy)
                        .on_press(cancel_admin)
                        .build(k)
                        .key("suspend-cancel"),
                )
                .footer(
                    Button::new(if busy { "Suspending\u{2026}" } else { "Suspend" })
                        .variant(Variant::Danger)
                        .disabled(busy)
                        .on_press(|a: &mut App| a.act(|s| s.submit_admin_user_dialog()))
                        .build(k)
                        .key("suspend-submit"),
                )
                .on_close(cancel_admin)
                .build(k, layer);
        }
        AdminUserDialog::Delete { reason, force, .. } => {
            let owned =
                s.ui.account
                    .admin_owned
                    .get(&user)
                    .cloned()
                    .unwrap_or_default();
            let input = TextInput::new(reason)
                .placeholder("Optional")
                .large()
                .on_edit(suspend_reason_edit)
                .on_submit(|a: &mut App, _| a.act(|s| s.submit_admin_user_dialog()));
            let mut body = col().key("delete-user-body").w_full().gap(16.0);
            if !owned.is_empty() {
                let mut list = owned_list(k, &owned, &name, false);
                list = list.child(
                    row()
                        .key("delete-user-transfer")
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .pt(4.0)
                        .child(
                            Checkbox::new(*force)
                                .label("Transfer their servers automatically")
                                .hide_label()
                                .on_toggle(|a: &mut App| {
                                    if let Some(AdminUserDialog::Delete { force, .. }) =
                                        a.state.ui.account.admin_dialog.as_mut()
                                    {
                                        *force = !*force;
                                    }
                                })
                                .build(k),
                        )
                        .child(
                            text("Transfer their servers automatically")
                                .font(type_::at(k, type_::CONTROL))
                                .color(t.text.color())
                                .a11y_hidden(),
                        ),
                );
                list = list.child(muted(k, "Each server goes to its member with the highest role (the longest-standing member breaks a tie). A server where they're the only member is deleted."));
                body = body.child(list);
            }
            body = body
                .child(dialog_field(k, "Reason (optional)", input, None).w_full())
                .child(muted(k, "Recorded in the audit log."));
            if let Some(e) = error.filter(|_| owned.is_empty()) {
                body = body.child(error_line(k, "delete-user-error", e));
            }
            let blocked = !owned.is_empty() && !*force;
            Dialog::new("admin-delete-user-dlg", format!("Delete {name}"))
                .description("This can't be undone. Their messages stay, shown as \u{201c}Deleted user\u{201d}. Everything else is removed: their profile and picture, server memberships, sign-ins and passkeys.")
                .open(true)
                .child(body)
                .footer(
                    Button::new("Cancel")
                        .variant(Variant::Secondary)
                        .disabled(busy)
                        .on_press(cancel_admin)
                        .build(k)
                        .key("delete-user-cancel"),
                )
                .footer(
                    Button::new(if busy { "Deleting\u{2026}" } else { "Delete user" })
                        .variant(Variant::Danger)
                        .disabled(busy || blocked)
                        .on_press(|a: &mut App| a.act(|s| s.submit_admin_user_dialog()))
                        .build(k)
                        .key("delete-user-submit"),
                )
                .on_close(cancel_admin)
                .build(k, layer);
        }
    }
    let _ = el();
}
