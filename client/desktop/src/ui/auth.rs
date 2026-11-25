//! The auth screen: a centred card with the wordmark, the step's fields, one primary button and
//! quiet muted links to the other ways in (an emailed code, a passkey, a password reset). The two
//! top-level steps end with one quiet line to the other ("Have an invite? Join with a code" /
//! "Already have an account? Sign in"); a second step (the code, the second factor, the invite's
//! account) has a way back instead. Errors land under the field they belong to, a form-wide one
//! right above the button, and neither says whether an account exists. The button says what it is doing while it waits (no spinner).

use rune::{col, icon, row, text, El, TextEdit};
use rune_kit::Kit;
use thehideout_design::components::{Button, Checkbox, CodeInput, InviteCodeInput, TextInput, Variant};
use thehideout_design::icons::ARROW_LEFT;
use thehideout_design::{radius, theme_of, type_};

use crate::app::App;
use crate::format::{day_label, day_of, unix};
use crate::forms::{AuthField, AuthMode, AuthStep, Phase};

fn username_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.username
}
fn password_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.password
}
fn email_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.email
}
fn code_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.code
}
fn recovery_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.recovery
}
fn invite_edit(a: &mut App) -> &mut TextEdit {
    &mut a.state.auth.invite
}

fn submit(a: &mut App, _tx: &mut rune::Tx) {
    a.act(|s| s.submit_auth());
}

fn field(k: &Kit, key: &str, label: &str, input: TextInput, error: Option<&str>) -> El {
    labelled(
        k,
        key,
        label,
        None,
        input
            .width(None)
            .large()
            .invalid(error.is_some())
            .label(label.to_owned())
            .build(k),
        error,
    )
}

/// A label (with an optional link at its right, "Forgot password?"), the control and its error.
fn labelled(
    k: &Kit,
    key: &str,
    label: &str,
    aside: Option<El>,
    control: El,
    error: Option<&str>,
) -> El {
    let t = theme_of(k);
    let mut head = row().w_full().center_y().child(
        text(label.to_owned())
            .font(type_::at(k, type_::LABEL))
            .color(t.text_muted.color())
            .grow(1.0),
    );
    if let Some(aside) = aside {
        head = head.child(aside);
    }
    let c = col()
        .key(key.to_owned())
        .w_full()
        .gap(6.0)
        .child(head)
        .child(control);
    match error {
        Some(e) => c.child(
            text(e.to_owned())
                .key("error")
                .font(type_::at(k, type_::CONTROL))
                .color(t.danger.color())
                .wrap()
                .w_full(),
        ),
        None => c,
    }
}

/// A text link in the accent ink.
fn link(k: &Kit, key: &str, label: &str, f: impl Fn(&mut App) + 'static) -> El {
    link_in(k, key, label, theme_of(k).accent_text.color(), f)
}

/// A text link in the muted ink, for the ways in that are not the main one.
fn quiet_link(k: &Kit, key: &str, label: &str, f: impl Fn(&mut App) + 'static) -> El {
    link_in(k, key, label, theme_of(k).text_muted.color(), f)
}

fn link_in(
    k: &Kit,
    key: &str,
    label: &str,
    color: rune::Color,
    f: impl Fn(&mut App) + 'static,
) -> El {
    let t = theme_of(k);
    text(label.to_owned())
        .key(key.to_owned())
        .font(type_::at(k, type_::CONTROL))
        .color(color)
        .role("button")
        .label(label.to_owned())
        .focusable(true)
        .focus_ring(2.0, 2.0, k.c.border_focus)
        .radius(radius::XS)
        .hover(t.hover.color())
        .on_press(f)
}

fn muted(k: &Kit, s: impl Into<String>) -> El {
    text(s.into())
        .font(type_::at(k, type_::CONTROL))
        .color(theme_of(k).text_muted.color())
        .wrap()
        .w_full()
}

fn primary(k: &Kit, label: &str, busy: bool) -> El {
    Button::new(label)
        .disabled(busy)
        .on_press(|a: &mut App| a.act(|s| s.submit_auth()))
        .build(k)
        .key("auth-submit")
        .w_full()
        .mt(4.0)
}

fn code_cells(a: &App, k: &Kit, label: &str) -> El {
    let auth = &a.state.auth;
    let busy = auth.phase == Phase::Busy;
    let error = auth.error_for(AuthField::Code);
    let input = CodeInput::new(&auth.code)
        .label(label)
        .invalid(error.is_some())
        .disabled(busy)
        .on_edit(code_edit)
        .on_change(|a: &mut App, _tx: &mut rune::Tx| a.act(|s| s.auth_code_changed()))
        .on_submit(submit)
        .build(k);
    labelled(k, "auth-code", label, None, input, error)
}

/// "← Back" over a second step.
fn back(k: &Kit, label: &str, f: impl Fn(&mut App) + 'static) -> El {
    let t = theme_of(k);
    row()
        .key("auth-back")
        .role("button")
        .label(label.to_owned())
        .gap(6.0)
        .center_y()
        .self_start()
        .px(4.0)
        .py(2.0)
        .ml(-4.0)
        .radius(radius::SM)
        .focusable(true)
        .focus_ring(2.0, 0.0, k.c.border_focus)
        .hover(t.hover.color())
        .on_press(f)
        .child(
            icon(&ARROW_LEFT, thehideout_design::icon::XS)
                .color(t.text_muted.color())
                .a11y_hidden(),
        )
        .child(
            text(label.to_owned())
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color())
                .a11y_hidden(),
        )
}

fn heading(k: &Kit, title: &str, body: impl Into<String>) -> El {
    let t = theme_of(k);
    col()
        .w_full()
        .gap(4.0)
        .child(
            text(title.to_owned())
                .font(type_::at(k, type_::TITLE))
                .color(t.text.color()),
        )
        .child(muted(k, body))
}

fn to_sign_in(a: &mut App) {
    a.act(|s| s.auth_go(AuthStep::Password));
}

pub fn build(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let auth = &a.state.auth;
    let suspension = auth.error.as_ref().and_then(|e| e.suspension.as_ref());
    let top_level = suspension.is_none() && matches!(auth.step, AuthStep::Password | AuthStep::InviteCode);

    // The full-width code field needs at least 316 at 100% text, 341 at 110% and 379 at 125%
    // (the boxes only shrink to three quarters of their size). The 400 card leaves 336 inside its
    // padding, so the invite step's card is 480 (416 inside) to keep the field in at every text size.
    let card_w = if matches!(auth.step, AuthStep::InviteCode) && suspension.is_none() { 480.0 } else { 400.0 };
    let mut card = col()
        .key("auth-card")
        .w(card_w)
        .max_w(card_w)
        .padding(32.0)
        .gap(16.0)
        .radius(radius::LG)
        .fill(t.surface.color())
        .border(1.0, t.border_subtle.color());
    let tagline = match auth.mode {
        AuthMode::Login => "Welcome back. Your friends are waiting.",
        AuthMode::Invite => "Joining? Use the invite someone sent you.",
    };
    card = card.child(
        col()
            .gap(4.0)
            .child(
                text("thehideout")
                    .font(type_::at(k, type_::HEADING))
                    .color(t.accent_text.color()),
            )
            .child(
                text(tagline)
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .wrap()
                    .w_full(),
            ),
    );
    let parts = match suspension {
        Some(s) => super::account::suspended_card(a, k, s),
        None => step(a, k),
    };
    for part in parts {
        card = card.child(part);
    }
    if top_level {
        card = card.child(switch_line(k, auth.mode));
    }

    col()
        .key("auth")
        .w_full()
        .h_full()
        .center()
        .gap(16.0)
        .fill(t.bg.color())
        .child(card)
        .child(
            row()
                .w(400.0)
                .center()
                .child(super::account::legal_links(k, "auth")),
        )
}

/// The form-wide error, which sits right above the step's button.
fn form_error(a: &App, k: &Kit) -> Option<El> {
    let msg = a.state.auth.error_for(AuthField::Form)?;
    Some(
        row().key("auth-form-error").w_full().child(
            text(msg.to_owned())
                .font(type_::at(k, type_::CONTROL))
                .color(theme_of(k).danger.color())
                .wrap()
                .w_full(),
        ),
    )
}

/// The quiet line at the bottom of a top-level step, to the other one.
fn switch_line(k: &Kit, mode: AuthMode) -> El {
    let (lead, key, label, to) = match mode {
        AuthMode::Login => (
            "Have an invite?",
            "auth-link-invite",
            "Join with a code",
            AuthMode::Invite,
        ),
        AuthMode::Invite => (
            "Already have an account?",
            "auth-link-signin",
            "Sign in",
            AuthMode::Login,
        ),
    };
    row()
        .key("auth-switch")
        .w_full()
        .gap(4.0)
        .center()
        .flex_wrap()
        .child(
            text(lead)
                .font(type_::at(k, type_::CONTROL))
                .color(theme_of(k).text_muted.color()),
        )
        .child(link(k, key, label, move |a: &mut App| {
            a.act(|s| s.set_auth_mode(to))
        }))
}

/// The step's heading, fields, button and links.
fn step(a: &App, k: &Kit) -> Vec<El> {
    let t = theme_of(k);
    let auth = &a.state.auth;
    let busy = auth.phase == Phase::Busy;
    let notice = auth.notice.as_ref().map(|n| {
        row()
            .key("auth-notice")
            .w_full()
            .padding(10.0)
            .radius(radius::MD)
            .fill(t.info_bg.color())
            .child(
                text(n.clone())
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text.color())
                    .wrap()
                    .w_full(),
            )
    });
    let mut out: Vec<El> = Vec::new();
    match &auth.step {
        AuthStep::Password => {
            out.extend(notice);
            let username = TextInput::new(&auth.username)
                .on_edit(username_edit)
                .on_submit(submit);
            let password = TextInput::new(&auth.password)
                .secure()
                .on_edit(password_edit)
                .on_submit(submit);
            let forgot = link(k, "auth-link-forgot", "Forgot password?", |a: &mut App| {
                a.act(|s| s.auth_go(AuthStep::Reset { sent: false }))
            });
            out.push(field(
                k,
                "auth-username",
                "Username or email",
                username,
                auth.error_for(AuthField::Username),
            ));
            let error = auth.error_for(AuthField::Password);
            out.push(labelled(
                k,
                "auth-password",
                "Password",
                Some(forgot),
                password
                    .width(None)
                    .large()
                    .invalid(error.is_some())
                    .label("Password")
                    .build(k),
                error,
            ));
            out.extend(form_error(a, k));
            out.push(primary(
                k,
                if busy {
                    "Signing in\u{2026}"
                } else {
                    "Sign in"
                },
                busy,
            ));
            out.push(
                row()
                    .key("auth-other")
                    .w_full()
                    .gap(8.0)
                    .center()
                    .flex_wrap()
                    .child(quiet_link(
                        k,
                        "auth-link-code",
                        "Email me a code",
                        |a: &mut App| a.act(|s| s.auth_go(AuthStep::EmailCode { sent: false })),
                    ))
                    .child(
                        text("\u{00b7}")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.text_faint.color()),
                    )
                    .child(quiet_link(
                        k,
                        "auth-link-passkey",
                        "Use a passkey",
                        |a: &mut App| a.act(|s| s.auth_go(AuthStep::Passkey { waiting: false })),
                    )),
            );
        }
        AuthStep::EmailCode { sent: false } | AuthStep::Reset { sent: false } => {
            let reset = matches!(auth.step, AuthStep::Reset { .. });
            out.push(back(k, "Back to sign in", to_sign_in));
            out.push(if reset {
                heading(
                    k,
                    "Reset your password",
                    "We'll email you a code to set a new one.",
                )
            } else {
                heading(
                    k,
                    "Sign in with a code",
                    "We'll email you a 6-digit code. No password needed.",
                )
            });
            let email = TextInput::new(&auth.email)
                .placeholder("you@example.com")
                .on_edit(email_edit)
                .on_submit(submit);
            out.push(field(
                k,
                "auth-email",
                "Email",
                email,
                auth.error_for(AuthField::Email),
            ));
            out.extend(form_error(a, k));
            out.push(primary(
                k,
                if busy { "Sending\u{2026}" } else { "Send code" },
                busy,
            ));
        }
        AuthStep::EmailCode { sent: true } | AuthStep::Reset { sent: true } => {
            let reset = matches!(auth.step, AuthStep::Reset { .. });
            out.push(back(k, "Use a different email", move |a: &mut App| {
                a.act(|s| {
                    s.auth_go(if reset {
                        AuthStep::Reset { sent: false }
                    } else {
                        AuthStep::EmailCode { sent: false }
                    })
                })
            }));
            out.push(heading(
                k,
                "Check your email",
                "Enter the 6-digit code we sent.",
            ));
            out.extend(notice);
            out.push(code_cells(a, k, "Code"));
            if reset {
                let password = TextInput::new(&auth.password)
                    .placeholder("At least 8 characters")
                    .secure()
                    .on_edit(password_edit)
                    .on_submit(submit);
                out.push(field(
                    k,
                    "auth-password",
                    "New password",
                    password,
                    auth.error_for(AuthField::Password),
                ));
            }
            out.extend(form_error(a, k));
            out.push(primary(
                k,
                match (busy, reset) {
                    (true, true) => "Saving\u{2026}",
                    (true, false) => "Signing in\u{2026}",
                    (false, true) => "Set new password",
                    (false, false) => "Sign in",
                },
                busy,
            ));
            let wait = a.state.resend_wait();
            let resend = if wait > 0 {
                text(format!("Resend code in {wait} s"))
                    .key("auth-resend")
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_faint.color())
            } else {
                link(k, "auth-resend", "Resend code", |a: &mut App| {
                    a.act(|s| s.resend_code())
                })
            };
            out.push(row().w_full().justify_center().child(resend));
        }
        AuthStep::Passkey { waiting: false } => {
            out.push(back(k, "Back to sign in", to_sign_in));
            out.push(heading(
                k,
                "Sign in with a passkey",
                if cfg!(target_arch = "wasm32") {
                    "Follow your browser's prompt to confirm it with your fingerprint, face or security key."
                } else {
                    "Your browser opens to confirm it with your fingerprint, face or security key."
                },
            ));
            let username = TextInput::new(&auth.username)
                .on_edit(username_edit)
                .on_submit(submit);
            out.push(field(
                k,
                "auth-username",
                "Username",
                username,
                auth.error_for(AuthField::Username),
            ));
            out.extend(form_error(a, k));
            out.push(primary(
                k,
                if busy {
                    "Opening your browser\u{2026}"
                } else {
                    "Continue with a passkey"
                },
                busy,
            ));
        }
        AuthStep::Passkey { waiting: true } => {
            let glyph = &thehideout_design::icons::FINGERPRINT;
            out.push(
                col()
                    .key("auth-passkey-waiting")
                    .w_full()
                    .gap(12.0)
                    .py(8.0)
                    .items_center()
                    .child(row().size(48.0, 48.0).center().radius(radius::LG).fill(t.accent_subtle.color()).child(icon(glyph, 28.0).color(t.accent_text.color())))
                    .child(text("Finish in your browser").font(type_::at(k, type_::TITLE)).color(t.text.color()))
                    .child(
                        text("We opened a page in your browser. Confirm with your passkey there and this window signs you in.")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.text_muted.color())
                            .wrap()
                            .w_full()
                            .align(rune::Align::Center),
                    )
                    .child(text("Waiting\u{2026}").key("auth-waiting").font(type_::at(k, type_::CAPTION)).color(t.text_faint.color())),
            );
            out.extend(form_error(a, k));
            out.push(
                Button::new("Cancel")
                    .variant(Variant::Secondary)
                    .on_press(|a: &mut App| a.act(|s| s.cancel_passkey_login()))
                    .build(k)
                    .key("auth-passkey-cancel")
                    .w_full(),
            );
        }
        AuthStep::SecondFactor { passkey: true, .. } => {
            let glyph = &thehideout_design::icons::FINGERPRINT;
            out.push(
                col()
                    .key("auth-2fa-passkey-waiting")
                    .w_full()
                    .gap(12.0)
                    .py(8.0)
                    .items_center()
                    .child(row().size(48.0, 48.0).center().radius(radius::LG).fill(t.accent_subtle.color()).child(icon(glyph, 28.0).color(t.accent_text.color())))
                    .child(text("Finish in your browser").font(type_::at(k, type_::TITLE)).color(t.text.color()))
                    .child(
                        text("We opened a page in your browser. Confirm with your passkey there to finish signing in.")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.text_muted.color())
                            .wrap()
                            .w_full()
                            .align(rune::Align::Center),
                    )
                    .child(text("Waiting\u{2026}").key("auth-waiting").font(type_::at(k, type_::CAPTION)).color(t.text_faint.color())),
            );
            out.extend(form_error(a, k));
            out.push(
                Button::new("Cancel")
                    .variant(Variant::Secondary)
                    .on_press(|a: &mut App| a.act(|s| s.cancel_passkey_login()))
                    .build(k)
                    .key("auth-passkey-cancel")
                    .w_full(),
            );
        }
        AuthStep::SecondFactor {
            recovery,
            locked,
            methods,
            ..
        } => {
            out.push(back(
                k,
                if *locked {
                    "Start again"
                } else {
                    "Back to sign in"
                },
                to_sign_in,
            ));
            if *locked {
                out.push(heading(
                    k,
                    "Two-factor authentication",
                    "This sign-in was stopped. Start again to get a new one.",
                ));
                out.extend(form_error(a, k));
                out.push(
                    Button::new("Start again")
                        .on_press(to_sign_in)
                        .build(k)
                        .key("auth-restart")
                        .w_full(),
                );
            } else if *recovery {
                out.push(heading(k, "Use a recovery code", "Enter one of the codes you saved when you set up two-factor authentication. Each works once."));
                let input = TextInput::new(&auth.recovery)
                    .placeholder("XXXX-XXXX")
                    .on_edit(recovery_edit)
                    .on_submit(submit);
                out.push(field(
                    k,
                    "auth-recovery",
                    "Recovery code",
                    input,
                    auth.error_for(AuthField::Code),
                ));
                out.extend(form_error(a, k));
                out.push(primary(
                    k,
                    if busy { "Checking\u{2026}" } else { "Sign in" },
                    busy,
                ));
            } else {
                out.push(heading(
                    k,
                    "Two-factor authentication",
                    "Enter the 6-digit code from your authenticator app.",
                ));
                out.push(code_cells(a, k, "Authenticator code"));
                out.extend(form_error(a, k));
                out.push(primary(
                    k,
                    if busy { "Checking\u{2026}" } else { "Sign in" },
                    busy,
                ));
            }
            if !*locked {
                let has_passkey = methods.contains(&thehideout_api::SecondFactorMethod::Passkey);
                let toggle = quiet_link(
                    k,
                    "auth-toggle-recovery",
                    if *recovery {
                        "Use your authenticator app"
                    } else {
                        "Use a recovery code"
                    },
                    |a: &mut App| a.act(|s| s.toggle_recovery()),
                );
                let mut links = row().w_full().gap(8.0).center().flex_wrap().child(toggle);
                // A passkey answers this challenge (offered when the account has one).
                if has_passkey {
                    links = links
                        .child(
                            text("\u{00b7}")
                                .font(type_::at(k, type_::CONTROL))
                                .color(t.text_faint.color()),
                        )
                        .child(quiet_link(
                            k,
                            "auth-2fa-passkey",
                            "Use a passkey",
                            |a: &mut App| a.act(|s| s.second_factor_passkey()),
                        ));
                }
                out.push(links);
            }
        }
        AuthStep::InviteCode => {
            let error = auth.error_for(AuthField::Invite);
            let control = col()
                .w_full()
                .gap(6.0)
                .child(
                    InviteCodeInput::new(&auth.invite)
                        .label("Invite code")
                        .invalid(error.is_some())
                        .disabled(busy)
                        .full_width()
                        .key("value")
                        .on_edit(invite_edit)
                        .on_submit(submit)
                        .build(k),
                )
                .child(
                    text("It's in your invite email.")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color())
                        .wrap()
                        .w_full(),
                );
            out.push(labelled(
                k,
                "auth-invite",
                "Invite code",
                None,
                control,
                error,
            ));
            out.extend(form_error(a, k));
            out.push(primary(
                k,
                if busy { "Checking\u{2026}" } else { "Continue" },
                busy,
            ));
        }
        AuthStep::InviteAccept { preview, code } => {
            let _ = code;
            out.push(back(k, "Use a different invite", |a: &mut App| {
                a.act(|s| s.set_auth_mode(AuthMode::Invite))
            }));
            let who = preview
                .inviter
                .as_ref()
                .map_or_else(|| "thehideout".to_owned(), |u| u.username.clone());
            let places = match preview.guild_names.as_slice() {
                [] => "thehideout".to_owned(),
                [one] => one.clone(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            };
            let s = &a.state;
            let expires = day_label(day_of(s.tz, unix(preview.expires_at)), s.today);
            out.push(
                col()
                    .key("auth-invite-summary")
                    .w_full()
                    .gap(4.0)
                    .padding(12.0)
                    .radius(radius::MD)
                    .fill(t.accent_subtle.color())
                    .child(
                        text(format!("{who} invited you to join {places}"))
                            .font(type_::at(k, type_::CONTROL_STRONG))
                            .color(t.text.color())
                            .wrap()
                            .w_full(),
                    )
                    .child(
                        text(format!(
                            "For {} \u{00b7} expires {expires}",
                            preview.masked_email
                        ))
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color())
                        .wrap()
                        .w_full(),
                    ),
            );
            let username = TextInput::new(&auth.username)
                .placeholder("ana")
                .on_edit(username_edit)
                .on_submit(submit);
            let password = TextInput::new(&auth.password)
                .placeholder("At least 8 characters")
                .secure()
                .on_edit(password_edit)
                .on_submit(submit);
            out.push(field(
                k,
                "auth-username",
                "Choose a username",
                username,
                auth.error_for(AuthField::Username),
            ));
            out.push(field(
                k,
                "auth-password",
                "Choose a password",
                password,
                auth.error_for(AuthField::Password),
            ));
            out.push(
                row()
                    .key("auth-add-passkey")
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(
                        Checkbox::new(auth.add_passkey)
                            .label("Add a passkey after signing up")
                            .hide_label()
                            .on_toggle(|a: &mut App| {
                                a.state.auth.add_passkey = !a.state.auth.add_passkey
                            })
                            .build(k),
                    )
                    .child(
                        text("Add a passkey after signing up")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.text.color())
                            .a11y_hidden(),
                    ),
            );
            out.push(agree_row(a, k));
            out.push(age_row(a, k, preview.minimum_age));
            let ready = auth.accept_terms && auth.confirm_age;
            if !ready && !busy {
                out.push(
                    text("Tick both boxes to create your account.")
                        .key("auth-agree-hint")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color())
                        .wrap()
                        .w_full(),
                );
            }
            out.extend(form_error(a, k));
            out.push(create_button(k, busy, !ready));
        }
    }
    out
}

/// "Create account": disabled while busy or while the terms or age box is unticked.
fn create_button(k: &Kit, busy: bool, unticked: bool) -> El {
    let label = if busy { "Creating account\u{2026}" } else { "Create account" };
    Button::new(label)
        .disabled(busy || unticked)
        .on_press(|a: &mut App| a.act(|s| s.submit_auth()))
        .build(k)
        .key("auth-submit")
        .w_full()
        .mt(4.0)
}

/// A checkbox with its label to the right (the label may hold links, so it is not the
/// checkbox's own; the box carries the name for assistive technology).
fn check_row(k: &Kit, key: &str, name: &str, on: bool, toggle: fn(&mut App), label: El) -> El {
    row()
        .key(key.to_owned())
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            Checkbox::new(on)
                .label(name.to_owned())
                .hide_label()
                .on_toggle(toggle)
                .build(k),
        )
        .child(label.grow(1.0).shrink().min_w(0.0))
}

/// "I agree to the Terms and Privacy Policy", the two names being links to the server's pages.
fn agree_row(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let plain = |s: &str| {
        text(s.to_owned())
            .font(type_::at(k, type_::CONTROL))
            .color(t.text.color())
    };
    let label = row()
        .gap(4.0)
        .center_y()
        .flex_wrap()
        .child(plain("I agree to the"))
        .child(super::account::legal_link(k, "auth-agree-terms", "Terms", "terms", true).font(type_::at(k, type_::CONTROL)))
        .child(plain("and"))
        .child(super::account::legal_link(k, "auth-agree-privacy", "Privacy Policy", "privacy", true).font(type_::at(k, type_::CONTROL)));
    check_row(
        k,
        "auth-agree",
        "I agree to the Terms and Privacy Policy",
        a.state.auth.accept_terms,
        |a: &mut App| a.state.auth.accept_terms = !a.state.auth.accept_terms,
        label,
    )
}

/// "I'm 16 or older" (the server's minimum age).
fn age_row(a: &App, k: &Kit, minimum_age: u32) -> El {
    let t = theme_of(k);
    let words = format!("I'm {minimum_age} or older");
    let label = text(words.clone())
        .font(type_::at(k, type_::CONTROL))
        .color(t.text.color())
        .a11y_hidden();
    check_row(
        k,
        "auth-age",
        &words,
        a.state.auth.confirm_age,
        |a: &mut App| a.state.auth.confirm_age = !a.state.auth.confirm_age,
        label,
    )
}
