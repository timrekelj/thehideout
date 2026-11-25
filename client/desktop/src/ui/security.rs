//! Settings → Security: the email, the password, two-factor authentication, passkeys and the
//! devices signed in, each a setting told apart by spacing (no rules). Changes happen in dialogs
//! (`dialogs`): the authenticator setup shows a QR code and the key to type, then asks for a code;
//! fresh recovery codes are shown once, with copy and save, and only go once "I saved them" is
//! ticked. Passkeys are made in the browser, which this app opens and waits for.

use rune::{col, icon, row, text, El, TextEdit, Tx};
use rune_kit::Kit;
use thehideout_api::{DeviceSession, Passkey};
use thehideout_design::components::{
    dialog_field, Badge, BadgeVariant, Button, Checkbox, CodeInput, Dialog, IconButton, QrCode,
    Size, TextInput, ToastKind, Variant,
};
use thehideout_design::icons::{COPY, DOWNLOAD, PENCIL, TRASH};
use thehideout_design::{metrics_of, radius, theme_of, type_};

use super::settings::{header, labels, setting, PAGE_W};
use crate::account::{SecDialog, SecKind};
use crate::app::App;
use crate::format::{clock_label, day_label, day_of, unix};
use crate::state::AppState;

fn when(s: &AppState, at: std::time::SystemTime) -> String {
    let ts = unix(at);
    let day = day_of(s.tz, ts);
    if day == s.today {
        format!("today at {}", clock_label(s.tz, ts))
    } else {
        day_label(day, s.today)
    }
}

fn muted(k: &Kit, s: impl Into<String>) -> El {
    text(s.into())
        .font(type_::at(k, type_::CAPTION))
        .color(theme_of(k).text_muted.color())
        .wrap()
}

/// A list row on the page: an icon tile, two lines and the actions.
fn item(
    k: &Kit,
    key: String,
    glyph: &'static rune::Icon,
    title: El,
    detail: String,
    actions: Vec<El>,
) -> El {
    let t = theme_of(k);
    let m = metrics_of(k);
    row()
        .key(key)
        .w_full()
        .min_h(m.table_row(56.0))
        .px(12.0)
        .py(m.pad(10.0))
        .gap(12.0)
        .center_y()
        .radius(radius::LG)
        .fill(t.bg.color())
        .child(
            row()
                .size(32.0, 32.0)
                .center()
                .radius(radius::MD)
                .fill(t.surface_raised.color())
                .child(
                    icon(glyph, thehideout_design::icon::SM)
                        .color(t.text_muted.color())
                        .a11y_hidden(),
                ),
        )
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(title)
                .child(muted(k, detail).max_lines(1)),
        )
        .children(actions)
}

pub fn page(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let sec = &s.ui.security;
    let me = s.me();
    let mut page = col()
        .key("page-security")
        .w_full()
        .max_w(PAGE_W)
        .px(32.0)
        .pb(32.0)
        .child(header(k, "Security", |a: &mut App| {
            a.act(|s| s.close_settings())
        }));

    // Email.
    let email = me.and_then(|u| u.email.clone());
    let email_line = row()
        .key("sec-email")
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            text(email.clone().unwrap_or_else(|| "No email yet".into()))
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(
                    if email.is_some() {
                        t.text
                    } else {
                        t.text_muted
                    }
                    .color(),
                )
                .grow(1.0)
                .max_lines(1),
        )
        .child(
            Button::new(if email.is_some() {
                "Change email"
            } else {
                "Add email"
            })
            .variant(Variant::Secondary)
            .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::Email { sent: false })))
            .build(k)
            .key("sec-email-change"),
        );
    page = page.child(setting(k, "Email", "Where sign-in codes, password resets and security notices go. A new address is confirmed with a code.", email_line));

    // Password.
    page = page.child(action_row(
        k,
        "sec-password",
        "Password",
        "Changing it signs out your other devices.",
        Button::new("Change password")
            .variant(Variant::Secondary)
            .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::Password)))
            .build(k)
            .key("sec-password-change"),
    ));

    // Two-factor authentication.
    let on = sec.totp == Some(true);
    let status = match sec.totp {
        Some(true) => Badge::new("On").variant(BadgeVariant::Success).build(k),
        Some(false) => Badge::new("Off").variant(BadgeVariant::Neutral).build(k),
        None => Badge::new("Checking\u{2026}")
            .variant(BadgeVariant::Neutral)
            .build(k),
    };
    let mut totp = row()
        .key("sec-totp")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(status.key("sec-totp-status"))
        .child(rune::el().grow(1.0));
    if on {
        totp = totp
            .child(
                Button::new("New recovery codes")
                    .variant(Variant::Secondary)
                    .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::Regenerate)))
                    .build(k)
                    .key("sec-totp-regenerate"),
            )
            .child(
                Button::new("Turn off")
                    .variant(Variant::Ghost)
                    .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::DisableTotp)))
                    .build(k)
                    .key("sec-totp-disable"),
            );
    } else {
        totp = totp.child(
            Button::new("Set up authenticator app")
                .variant(Variant::Secondary)
                .disabled(sec.totp.is_none() && sec.loading)
                .on_press(|a: &mut App| a.act(|s| s.open_sec(SecKind::TotpSetup { setup: None })))
                .build(k)
                .key("sec-totp-enable"),
        );
    }
    page = page.child(setting(
        k,
        "Two-factor authentication",
        "After your password or an emailed code, also ask for a code from an authenticator app. Recovery codes get you in if you lose it.",
        totp,
    ));

    // Passkeys and devices need the lists.
    if let Some(e) = &sec.error {
        page = page.child(
            row()
                .key("sec-error")
                .w_full()
                .gap(12.0)
                .py(12.0)
                .center_y()
                .child(
                    text(e.clone())
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.danger.color())
                        .grow(1.0)
                        .wrap(),
                )
                .child(
                    Button::new("Try again")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .on_press(|a: &mut App| a.act(|s| s.load_security()))
                        .build(k)
                        .key("sec-retry"),
                ),
        );
    }
    let info = sec.info.as_ref();
    let mut keys = col().key("sec-passkeys").w_full().gap(6.0);
    match info {
        None => keys = keys.child(muted(k, if sec.loading { "Loading\u{2026}" } else { "" })),
        Some(i) if i.passkeys.is_empty() => {
            keys = keys.child(muted(k, "No passkeys yet.").key("sec-passkeys-empty"))
        }
        Some(i) => {
            for p in &i.passkeys {
                keys = keys.child(passkey_row(a, k, p));
            }
        }
    }
    keys = keys.child(
        row().pt(6.0).child(
            Button::new("Add a passkey")
                .variant(Variant::Secondary)
                .icon(&thehideout_design::icons::PLUS)
                .on_press(|a: &mut App| {
                    a.act(|s| s.open_sec(SecKind::AddPasskey { waiting: false }))
                })
                .build(k)
                .key("sec-passkey-add"),
        ),
    );
    page = page.child(setting(k, "Passkeys", "Sign in with your fingerprint, face or a security key instead of a password. No second factor needed.", keys));

    let mut devices = col().key("sec-sessions").w_full().gap(6.0);
    if let Some(i) = info {
        let mut list: Vec<&DeviceSession> = i.sessions.iter().collect();
        list.sort_by_key(|d| !d.current);
        for d in list {
            devices = devices.child(session_row(a, k, d));
        }
    } else if sec.loading {
        devices = devices.child(muted(k, "Loading\u{2026}"));
    }
    let others = s.other_sessions().len();
    devices = devices.child(
        row().pt(6.0).child(
            Button::new(if sec.others_busy {
                "Signing out\u{2026}"
            } else {
                "Sign out of all other devices"
            })
            .variant(Variant::Secondary)
            .disabled(others == 0 || sec.others_busy)
            .on_press(|a: &mut App| a.act(|s| s.revoke_other_sessions()))
            .build(k)
            .key("sec-sessions-revoke-others"),
        ),
    );
    page = page.child(setting(
        k,
        "Devices",
        "Where your account is signed in. Sign out of anything you don't recognise.",
        devices,
    ));
    page = page.child(super::account::security_section(a, k));
    col().w_full().items_center().child(page)
}

/// A setting whose control sits at the right of its text.
fn action_row(k: &Kit, key: &str, title: &str, description: &str, control: El) -> El {
    let m = metrics_of(k);
    row()
        .key(key.to_owned())
        .w_full()
        .gap(16.0)
        .py(m.pad(14.0))
        .center_y()
        .child(labels(k, title, description).grow(1.0).shrink())
        .child(control)
}

fn passkey_row(a: &App, k: &Kit, p: &Passkey) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let id = p.id;
    let used = p.last_used_at.map_or_else(
        || "never used".to_owned(),
        |at| format!("last used {}", when(s, at)),
    );
    let detail = format!("Added {} \u{00b7} {used}", when(s, p.created_at));
    let glyph = &thehideout_design::icons::FINGERPRINT;
    let title = text(p.name.clone())
        .font(type_::at(k, type_::CONTROL_STRONG))
        .color(t.text.color())
        .max_lines(1);
    item(
        k,
        format!("passkey-{id}"),
        glyph,
        title,
        detail,
        vec![
            IconButton::new(&PENCIL, format!("Rename {}", p.name))
                .on_press(move |a: &mut App| a.act(|s| s.open_sec(SecKind::RenamePasskey { id })))
                .build(k)
                .key(format!("passkey-rename-{id}")),
            IconButton::new(&TRASH, format!("Remove {}", p.name))
                .on_press(move |a: &mut App| a.act(|s| s.open_sec(SecKind::DeletePasskey { id })))
                .build(k)
                .key(format!("passkey-delete-{id}")),
        ],
    )
}

fn session_row(a: &App, k: &Kit, d: &DeviceSession) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let id = d.id;
    let name = if d.device_name.is_empty() {
        "Unknown device".to_owned()
    } else {
        d.device_name.clone()
    };
    let lower = name.to_lowercase();
    let glyph = if ["iphone", "android", "phone", "pixel"]
        .iter()
        .any(|w| lower.contains(w))
    {
        &thehideout_design::icons::DEVICE_MOBILE
    } else {
        &thehideout_design::icons::DESKTOP
    };
    let seen = d.last_seen_at.map_or_else(String::new, |at| {
        format!(" \u{00b7} active {}", when(s, at))
    });
    let detail = format!("Signed in {}{seen}", when(s, d.created_at));
    let mut title = row().gap(8.0).center_y().child(
        text(name.clone())
            .font(type_::at(k, type_::CONTROL_STRONG))
            .color(t.text.color())
            .max_lines(1)
            .shrink(),
    );
    let mut actions = Vec::new();
    if d.current {
        title = title.child(
            Badge::new("This device")
                .variant(BadgeVariant::Accent)
                .build(k)
                .key("session-current"),
        );
    } else {
        let busy = s.ui.security.busy.contains(&id);
        actions.push(
            Button::new(if busy {
                "Signing out\u{2026}"
            } else {
                "Sign out"
            })
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .disabled(busy)
            .on_press(move |a: &mut App| a.act(|s| s.revoke_session(id)))
            .build(k)
            .key(format!("session-revoke-{id}")),
        );
    }
    item(k, format!("session-{id}"), glyph, title, detail, actions)
}

// ---- dialogs -----------------------------------------------------------------------------------

fn dialog(a: &mut App) -> Option<&mut SecDialog> {
    a.state.ui.security.dialog.as_mut()
}
fn first_edit(a: &mut App) -> &mut TextEdit {
    if a.state.ui.security.dialog.is_none() {
        return &mut a.state.ui.idle_composer;
    }
    &mut dialog(a).expect("checked").first
}
fn second_edit(a: &mut App) -> &mut TextEdit {
    if a.state.ui.security.dialog.is_none() {
        return &mut a.state.ui.idle_composer;
    }
    &mut dialog(a).expect("checked").second
}
fn third_edit(a: &mut App) -> &mut TextEdit {
    if a.state.ui.security.dialog.is_none() {
        return &mut a.state.ui.idle_composer;
    }
    &mut dialog(a).expect("checked").third
}
fn code_edit(a: &mut App) -> &mut TextEdit {
    if a.state.ui.security.dialog.is_none() {
        return &mut a.state.ui.idle_composer;
    }
    &mut dialog(a).expect("checked").code
}

fn close(a: &mut App) {
    a.act(|s| s.close_sec());
}
fn submit(a: &mut App) {
    a.act(|s| s.submit_sec());
}
fn submit_tx(a: &mut App, _tx: &mut Tx) {
    submit(a);
}

fn cancel(k: &Kit, label: &str) -> El {
    Button::new(label)
        .variant(Variant::Secondary)
        .on_press(close)
        .build(k)
        .key("sec-cancel")
}

fn go(k: &Kit, label: (&str, &str), busy: bool, variant: Variant, enabled: bool) -> El {
    Button::new(if busy { label.1 } else { label.0 })
        .variant(variant)
        .disabled(busy || !enabled)
        .on_press(submit)
        .build(k)
        .key("sec-submit")
}

fn code_field(k: &Kit, d: &SecDialog, label: &str) -> El {
    let t = theme_of(k);
    let input = CodeInput::new(&d.code)
        .label(label)
        .invalid(d.error.is_some())
        .disabled(d.busy)
        .on_edit(code_edit)
        .on_change(|a: &mut App, _tx: &mut Tx| a.act(|s| s.sec_code_changed()))
        .on_submit(submit_tx)
        .build(k);
    col()
        .key("sec-code")
        .w_full()
        .gap(6.0)
        .child(
            text(label.to_owned())
                .font(type_::at(k, type_::LABEL))
                .color(t.text_muted.color()),
        )
        .child(input)
}

fn error(k: &Kit, d: &SecDialog) -> Option<El> {
    d.error.as_ref().map(|e| {
        text(e.clone())
            .key("sec-error")
            .font(type_::at(k, type_::CONTROL))
            .color(theme_of(k).danger.color())
            .wrap()
            .w_full()
    })
}

/// The secret in groups of four, as authenticator apps show it.
pub fn grouped(secret: &str) -> String {
    secret
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn dialogs(a: &App, k: &Kit, layer: &mut Vec<El>, window_h: f32) {
    let Some(d) = &a.state.ui.security.dialog else {
        return;
    };
    let t = theme_of(k);
    let busy = d.busy;
    let max_h = (window_h - 230.0).max(160.0);
    let (title, description, body, footer): (String, String, El, Vec<El>) = match &d.kind {
        SecKind::Email { sent: false } => {
            let adding = a.state.me().is_none_or(|u| u.email.is_none());
            let email = TextInput::new(&d.first)
                .placeholder("you@example.com")
                .on_edit(first_edit)
                .on_submit(submit_tx);
            let password = TextInput::new(&d.second)
                .placeholder("Leave empty if you have none")
                .secure()
                .on_edit(second_edit)
                .on_submit(submit_tx);
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(16.0)
                .child(dialog_field(
                    k,
                    if adding { "Email" } else { "New email" },
                    email,
                    None,
                ))
                .child(dialog_field(k, "Current password", password, None))
                .children(error(k, d));
            (
                if adding {
                    "Add an email"
                } else {
                    "Change email"
                }
                .into(),
                "We'll send a code to the new address to confirm it's yours.".into(),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(
                        k,
                        ("Send code", "Sending\u{2026}"),
                        busy,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
        SecKind::Email { sent: true } => {
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(code_field(k, d, "Code"))
                .children(error(k, d));
            (
                "Check your email".into(),
                format!("Enter the 6-digit code we sent to {}.", d.first.text.trim()),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(
                        k,
                        ("Confirm", "Confirming\u{2026}"),
                        busy,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
        SecKind::Password => {
            let current = TextInput::new(&d.first)
                .placeholder("Leave empty if you have none")
                .secure()
                .on_edit(first_edit)
                .on_submit(submit_tx);
            let new = TextInput::new(&d.second)
                .placeholder("At least 8 characters")
                .secure()
                .on_edit(second_edit)
                .on_submit(submit_tx);
            let again = TextInput::new(&d.third)
                .secure()
                .on_edit(third_edit)
                .on_submit(submit_tx);
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(16.0)
                .child(dialog_field(k, "Current password", current, None))
                .child(dialog_field(k, "New password", new, None))
                .child(dialog_field(k, "New password again", again, None))
                .children(error(k, d));
            (
                "Change password".into(),
                "Your other devices are signed out; this one stays signed in.".into(),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(
                        k,
                        ("Change password", "Changing\u{2026}"),
                        busy,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
        SecKind::TotpSetup { setup } => {
            let body = match setup {
                None => col()
                    .key("sec-body")
                    .w_full()
                    .py(24.0)
                    .items_center()
                    .child(muted(k, "Starting\u{2026}"))
                    .children(error(k, d)),
                Some(setup) => {
                    let secret = setup.secret.clone();
                    let copy = Button::new("Copy key")
                        .icon(&COPY)
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .build(k)
                        .key("totp-copy")
                        .on_press_tx(move |a: &mut App, tx: &mut Tx| {
                            tx.copy(secret.clone());
                            a.state.toast(ToastKind::Success, "Key copied");
                        });
                    let scan = row()
                        .key("totp-scan")
                        .w_full()
                        .gap(16.0)
                        .items_start()
                        .child(
                            QrCode::new(setup.otpauth_uri.clone())
                                .label("QR code for your authenticator app")
                                .build(k),
                        )
                        .child(
                            col()
                                .grow(1.0)
                                .shrink()
                                .min_w(0.0)
                                .gap(8.0)
                                .child(
                                    text("1. Scan this with your authenticator app.")
                                        .font(type_::at(k, type_::CONTROL))
                                        .color(t.text.color())
                                        .wrap()
                                        .w_full(),
                                )
                                .child(muted(k, "Can't scan it? Enter this key instead:"))
                                .child(
                                    row()
                                        .key("totp-secret")
                                        .w_full()
                                        .px(10.0)
                                        .py(6.0)
                                        .radius(radius::MD)
                                        .fill(t.bg.color())
                                        .child(
                                            text(grouped(&setup.secret))
                                                .font(type_::at(k, type_::CODE))
                                                .color(t.text.color())
                                                .wrap()
                                                .w_full(),
                                        ),
                                )
                                .child(row().child(copy)),
                        );
                    col()
                        .key("sec-body")
                        .w_full()
                        .gap(16.0)
                        .child(scan)
                        .child(code_field(k, d, "2. Enter the 6-digit code it shows"))
                        .children(error(k, d))
                }
            };
            (
                "Set up two-factor authentication".into(),
                "Use an app such as 1Password, Google Authenticator or Aegis.".into(),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(
                        k,
                        ("Turn on", "Checking\u{2026}"),
                        busy,
                        Variant::Primary,
                        setup.is_some(),
                    ),
                ],
            )
        }
        SecKind::RecoveryCodes { codes } => {
            let all = codes.join("\n");
            let mut grid = col()
                .key("recovery-codes")
                .w_full()
                .gap(4.0)
                .padding(12.0)
                .radius(radius::MD)
                .fill(t.bg.color());
            for pair in codes.chunks(2) {
                let mut line = row().w_full().gap(16.0);
                for c in pair {
                    line = line.child(
                        text(c.clone())
                            .font(type_::at(k, type_::CODE))
                            .color(t.text.color())
                            .grow(1.0)
                            .w(0.0),
                    );
                }
                grid = grid.child(line);
            }
            let to_copy = all.clone();
            let copy = Button::new("Copy")
                .icon(&COPY)
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .build(k)
                .key("recovery-copy")
                .on_press_tx(move |a: &mut App, tx: &mut Tx| {
                    tx.copy(to_copy.clone());
                    a.state.toast(ToastKind::Success, "Recovery codes copied");
                });
            let to_save = all;
            let save = Button::new("Save to a file\u{2026}")
                .icon(&DOWNLOAD)
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .build(k)
                .key("recovery-save")
                .on_press_tx(move |a: &mut App, tx: &mut Tx| save_codes(a, tx, to_save.clone()));
            let saved = row()
                .key("recovery-saved")
                .gap(8.0)
                .center_y()
                .child(
                    Checkbox::new(d.saved)
                        .label("I saved these codes somewhere safe")
                        .hide_label()
                        .on_toggle(|a: &mut App| a.act(|s| s.toggle_saved()))
                        .build(k),
                )
                .child(
                    text("I saved these codes somewhere safe")
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color())
                        .a11y_hidden(),
                );
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(grid)
                .child(row().gap(8.0).child(copy).child(save))
                .child(saved)
                .children(error(k, d));
            (
                "Save your recovery codes".into(),
                "Each code signs you in once if you lose your authenticator app. This is the only time they are shown.".into(),
                body,
                vec![go(k, ("Done", "Done"), false, Variant::Primary, d.saved)],
            )
        }
        SecKind::DisableTotp => {
            let input = TextInput::new(&d.code)
                .placeholder("123456 or XXXX-XXXX")
                .on_edit(code_edit)
                .on_submit(submit_tx);
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(dialog_field(
                    k,
                    "Code from your app, or a recovery code",
                    input,
                    None,
                ))
                .children(error(k, d));
            (
                "Turn off two-factor authentication".into(),
                "Signing in will only need your password or an emailed code. Your recovery codes stop working.".into(),
                body,
                vec![cancel(k, "Cancel"), go(k, ("Turn off", "Turning off\u{2026}"), busy, Variant::Danger, true)],
            )
        }
        SecKind::Regenerate => {
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(code_field(k, d, "Code from your authenticator app"))
                .children(error(k, d));
            (
                "New recovery codes".into(),
                "Your old recovery codes stop working as soon as the new ones are made.".into(),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(
                        k,
                        ("Make new codes", "Making\u{2026}"),
                        busy,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
        SecKind::AddPasskey { waiting: false } => {
            let input = TextInput::new(&d.first)
                .placeholder("MacBook")
                .on_edit(first_edit)
                .on_submit(submit_tx);
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(dialog_field(k, "Name", input, None))
                .children(error(k, d));
            (
                "Add a passkey".into(),
                if cfg!(target_arch = "wasm32") {
                    "Follow your browser's prompt to make it with your fingerprint, face or a security key."
                } else {
                    "Your browser opens to make it with your fingerprint, face or a security key."
                }
                .into(),
                body,
                vec![
                    cancel(
                        k,
                        if a.state.ui.security.next.is_some() {
                            "Skip"
                        } else {
                            "Cancel"
                        },
                    ),
                    go(
                        k,
                        ("Continue in browser", "Opening\u{2026}"),
                        busy,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
        SecKind::AddPasskey { waiting: true } => {
            let glyph = &thehideout_design::icons::FINGERPRINT;
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .py(8.0)
                .items_center()
                .child(
                    row()
                        .size(48.0, 48.0)
                        .center()
                        .radius(radius::LG)
                        .fill(t.accent_subtle.color())
                        .child(icon(glyph, 28.0).color(t.accent_text.color())),
                )
                .child(
                    text("Finish in your browser")
                        .font(type_::at(k, type_::TITLE))
                        .color(t.text.color()),
                )
                .child(muted(
                    k,
                    "Confirm the new passkey there. This dialog closes once it's saved.",
                ))
                .children(error(k, d));
            (
                "Add a passkey".into(),
                String::new(),
                body,
                vec![cancel(k, "Cancel")],
            )
        }
        SecKind::RenamePasskey { .. } => {
            let input = TextInput::new(&d.first)
                .on_edit(first_edit)
                .on_submit(submit_tx);
            let body = col()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .child(dialog_field(k, "Name", input, None))
                .children(error(k, d));
            (
                "Rename passkey".into(),
                String::new(),
                body,
                vec![
                    cancel(k, "Cancel"),
                    go(k, ("Save", "Saving\u{2026}"), busy, Variant::Primary, true),
                ],
            )
        }
        SecKind::DeletePasskey { id } => {
            let name = a
                .state
                .passkey(*id)
                .map_or_else(String::new, |p| p.name.clone());
            let body = col().key("sec-body").w_full().children(error(k, d));
            (
                "Remove passkey".into(),
                format!("\u{201c}{name}\u{201d} will no longer sign you in. Remove it from your device's password manager too."),
                body,
                vec![cancel(k, "Cancel"), go(k, ("Remove passkey", "Removing\u{2026}"), busy, Variant::Danger, true)],
            )
        }
        SecKind::Welcome => {
            let glyph =
                &thehideout_design::icons::SHIELD_CHECK;
            let body = row()
                .key("sec-body")
                .w_full()
                .gap(12.0)
                .items_start()
                .child(row().size(40.0, 40.0).center().radius(radius::LG).fill(t.accent_subtle.color()).child(icon(glyph, 22.0).color(t.accent_text.color())))
                .child(muted(k, "Two-factor authentication asks for a code from an app on your phone when you sign in, so a stolen password isn't enough. It takes a minute; you can also do it later in Settings \u{2192} Security.").grow(1.0).shrink());
            (
                "Welcome to thehideout".into(),
                "Your account is ready. Protect it now?".into(),
                body,
                vec![
                    cancel(k, "Skip for now"),
                    go(
                        k,
                        ("Set up two-factor", "Set up two-factor"),
                        false,
                        Variant::Primary,
                        true,
                    ),
                ],
            )
        }
    };
    let mut dlg = Dialog::new("sec-dlg", title)
        .open(true)
        .body_max_h(max_h)
        .child(body)
        .on_close(close);
    if !description.is_empty() {
        dlg = dlg.description(description);
    }
    for f in footer {
        dlg = dlg.footer(f);
    }
    dlg.build(k, layer);
}

/// The save dialog for the recovery codes; the text is written where the user chose (a few
/// hundred bytes, written in the answer).
#[cfg(target_arch = "wasm32")]
fn save_codes(a: &mut App, _tx: &mut Tx, codes: String) {
    let body = format!("thehideout recovery codes\nEach code works once.\n\n{codes}\n");
    crate::web::save_text("thehideout-recovery-codes.txt", &body);
    a.state.toast(ToastKind::Success, "Recovery codes saved");
}

#[cfg(not(target_arch = "wasm32"))]
fn save_codes(_a: &mut App, tx: &mut Tx, codes: String) {
    let options = rune::FileDialogOptions {
        title: "Save recovery codes".into(),
        file_name: Some("thehideout-recovery-codes.txt".into()),
        ..Default::default()
    };
    tx.save_file(options, move |a: &mut App, result, _tx| match result {
        Ok(Some(path)) => {
            let body = format!("thehideout recovery codes\nEach code works once.\n\n{codes}\n");
            match std::fs::write(&path, body) {
                Ok(()) => a.state.toast(ToastKind::Success, "Recovery codes saved"),
                Err(e) => a
                    .state
                    .toast(ToastKind::Danger, format!("Couldn't save the codes. {e}")),
            }
        }
        Ok(None) => {}
        Err(e) => a.state.toast(
            ToastKind::Danger,
            format!("Can't open the save dialog. {e}"),
        ),
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn secrets_are_shown_in_groups_of_four() {
        assert_eq!(super::grouped("JBSWY3DPEHPK3PXP"), "JBSW Y3DP EHPK 3PXP");
    }
}
