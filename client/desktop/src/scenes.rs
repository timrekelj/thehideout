//! Headless scenes for `--scene NAME --snap out.png`: each boots the fake backend, signs in
//! where the scene needs it and sets one situation up. `NAME-light` is the light twin.

use thehideout_design::components::ToastKind;
use thehideout_design::{AccentPreset, Appearance, BackgroundPreset, Density, TextSize, ThemeSpec};

use crate::admin::{AdminPage, Confirm};
use crate::app::{App, fake_now};
use crate::backend::Command;
use crate::emoji::PickerTarget;
use crate::fake::{
    ANA, ARCHIVE, BRUNO, CVETKA, DESIGN, DEV, DM_ANA, FakeBackend, GENERAL, HIDEOUT_DESIGNERS,
    HIDEOUT_MODS, MEMES, MUSIC, PLANS, RANDOM, SMUG_CAT,
};
use crate::format::Tz;
use crate::server_settings::ServerPage;
use crate::account::SecKind;
use crate::fake::account::INVITE_PENDING;
use crate::forms::AuthStep;
use crate::state::{AppState, AuthMode, DialogKind, Menu};

/// A signed-in app on the fake world, in the given theme, with The Hideout's #general open.
pub fn booted(dark: bool) -> (App, FakeBackend) {
    booted_as(dark, "tim")
}

/// The same, signed in as `who`: tim owns The Hideout, ana is a plain member there.
pub fn booted_as(dark: bool, who: &str) -> (App, FakeBackend) {
    let fake = FakeBackend::new();
    let mut state = AppState::new(fake_now, Tz::Fixed(0));
    state.ui.look.spec.appearance = if dark {
        Appearance::Dark
    } else {
        Appearance::Light
    };
    let mut app = App::new(Box::new(fake.clone()), state);
    app.state.auth.username.replace_text(who);
    app.state.auth.password.replace_text("hunter22");
    app.act(|s| s.submit_auth());
    app.pump();
    (app, fake)
}

fn signed_out(dark: bool) -> App {
    let mut state = AppState::new(fake_now, Tz::Fixed(0));
    state.ui.look.spec.appearance = if dark {
        Appearance::Dark
    } else {
        Appearance::Light
    };
    App::new(Box::new(FakeBackend::new()), state)
}

fn auth_login(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.state.auth.username.replace_text("tim");
    a.state.auth.username.focused = false;
    a.state.auth.password.replace_text("hunter22");
    a.state.auth.password.focused = true;
}

/// "Email me a code": the code was sent and is half typed.
fn auth_code(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.act(|s| s.auth_go(AuthStep::EmailCode { sent: false }));
    a.state.auth.email.replace_text("tim@example.com");
    a.act(|s| s.submit_auth());
    a.pump();
    a.state.auth.code.replace_text("123");
}

/// Dora has an authenticator app: the second step, after one wrong code.
fn auth_second_factor(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.state.auth.username.replace_text("dora");
    a.state.auth.password.replace_text("hunter22");
    a.act(|s| s.submit_auth());
    a.pump();
    a.state.auth.code.replace_text("111111");
    a.act(|s| s.submit_auth());
    a.pump();
}

/// "I have an invite" with the code from the email typed in.
fn auth_invite_lookup(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.act(|s| s.set_auth_mode(AuthMode::Invite));
    a.state.auth.invite.replace_text("WXYZ-2345");
}

/// The invite looked up: who sent it and where it leads, a name and password being chosen.
fn auth_invite_accept(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.act(|s| s.set_auth_mode(AuthMode::Invite));
    a.state.auth.invite.replace_text(INVITE_PENDING);
    a.act(|s| s.submit_auth());
    a.pump();
    a.state.auth.username.replace_text("erin");
    a.state.auth.password.replace_text("correct horse");
    a.state.auth.add_passkey = true;
}

/// A passkey sign-in waiting for the browser.
fn auth_passkey_waiting(a: &mut App, dark: bool) {
    *a = signed_out(dark);
    a.act(|s| s.auth_go(AuthStep::Passkey { waiting: false }));
    a.state.auth.username.replace_text("tim");
    a.act(|s| s.submit_auth());
    a.pump();
}

/// Bruno's account has no email: the prompt under the channel header.
fn add_email_banner(a: &mut App, dark: bool) {
    *a = booted_as(dark, "bruno").0;
}

/// Settings → Security with its lists in.
fn security_page(dark: bool) -> App {
    let (mut app, _fake) = booted(dark);
    app.act(|s| s.open_security());
    app.pump();
    app
}

fn settings_security(a: &mut App, dark: bool) {
    *a = security_page(dark);
}

/// The authenticator setup: QR code, key and the code half typed.
fn settings_security_totp_setup(a: &mut App, dark: bool) {
    let mut app = security_page(dark);
    app.act(|s| s.open_sec(SecKind::TotpSetup { setup: None }));
    app.pump();
    if let Some(d) = app.state.ui.security.dialog.as_mut() {
        d.code.replace_text("482");
    }
    *a = app;
}

/// The setup confirmed: the recovery codes, shown once.
fn settings_security_recovery_codes(a: &mut App, dark: bool) {
    let mut app = security_page(dark);
    app.act(|s| s.open_sec(SecKind::TotpSetup { setup: None }));
    app.pump();
    if let Some(d) = app.state.ui.security.dialog.as_mut() {
        d.code.replace_text("123456");
    }
    app.act(|s| s.submit_sec());
    app.pump();
    *a = app;
}

/// The server's invite dialog: an invite just sent (its code to copy) and the pending ones.
fn invite_by_email(a: &mut App, dark: bool) {
    let (mut app, _fake) = booted(dark);
    app.act(|s| s.open_dialog(DialogKind::InviteByEmail { guild: 1 }));
    app.pump();
    if let Some(d) = app.state.ui.dialog.as_mut() {
        d.input.replace_text("dana@example.com");
    }
    app.act(|s| s.submit_dialog());
    app.pump();
    *a = app;
}

fn admin_invites(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Invites);
}

/// The invite looked up and an account being chosen, the name already taken.
fn auth_signup_error(a: &mut App, dark: bool) {
    auth_invite_accept(a, dark);
    a.state.auth.username.replace_text("taken");
    a.state.auth.password.replace_text("secret123");
    a.act(|s| s.submit_auth());
    a.pump();
}

/// The busy main screen: a read channel, a channel with unread messages (one mentions you), a
/// DM waiting, another server with news, and a "New" divider in #general.
fn main_screen(dark: bool) -> (App, FakeBackend) {
    let (mut a, fake) = booted(dark);
    a.act(|s| s.select_channel(DEV));
    a.pump();
    fake.incoming(
        GENERAL,
        BRUNO,
        "Anyone up for lunch? The ramen place opens at noon.",
    );
    fake.incoming(GENERAL, ANA, "@tim bring the speaker on Friday!");
    a.pump();
    a.act(|s| s.select_channel(GENERAL));
    a.pump();
    fake.incoming(
        DEV,
        CVETKA,
        "@tim could you review the backoff test when you have a minute?",
    );
    fake.incoming(RANDOM, CVETKA, "look what I found");
    fake.incoming(20, ANA, "rehearsal at seven");
    fake.incoming(DM_ANA, ANA, "did you see my message?");
    a.pump();
    (a, fake)
}

fn main_(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
}

/// The busy main screen in a chosen look (accent, background, tint, contrast), for review.
fn main_look(
    a: &mut App,
    dark: bool,
    accent: AccentPreset,
    background: BackgroundPreset,
    tint: bool,
) {
    *a = main_screen(dark).0;
    a.state.ui.look.spec = ThemeSpec {
        accent,
        background,
        tint,
        appearance: a.state.ui.look.spec.appearance,
        ..ThemeSpec::default()
    };
}

/// The Appearance page of the settings, over a look that is not the default, so the selected
/// swatch and tile are not the first ones.
fn settings_appearance(a: &mut App, dark: bool) {
    main_look(a, dark, AccentPreset::Ocean, BackgroundPreset::Cool, true);
    a.act(|s| s.open_settings());
    a.pump();
}

/// The busy main screen with the member panel, at a density and text size.
fn main_sized(a: &mut App, dark: bool, density: Density, text_size: TextSize) {
    main_members(a, dark);
    a.state.ui.look.spec.density = density;
    a.state.ui.look.spec.text_size = text_size;
}

fn main_members(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.state.ui.members_open = true;
}

fn home_dms(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.act(|s| {
        s.go_home();
        s.select_channel(DM_ANA);
    });
    a.pump();
}

fn home_empty(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.act(|s| s.go_home());
    a.pump();
}

/// A toast while a channel is open: it must sit above the composer.
fn toast(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.state.toast(
        ToastKind::Danger,
        "Message not sent. Can't reach the server.",
    );
}

fn empty_channel(a: &mut App, dark: bool) {
    *a = booted(dark).0;
    a.act(|s| s.select_channel(PLANS));
    a.pump();
}

fn dialog_create_server(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.state.open_dialog(DialogKind::CreateGuild);
    if let Some(d) = &mut a.state.ui.dialog {
        d.input.replace_text("Weekend Climbers");
    }
}

fn reconnecting(a: &mut App, dark: bool) {
    let (mut app, fake) = main_screen(dark);
    fake.disconnect(5);
    app.pump();
    *a = app;
}

/// The busy sidebar: top-level channels, three sections (one folded), coloured icons, unread
/// and a mention, #design selected.
fn sections(a: &mut App, dark: bool) {
    let (mut app, fake) = main_screen(dark);
    fake.incoming(MUSIC, CVETKA, "new playlist is up");
    app.pump();
    app.act(|s| {
        s.toggle_section(1, ARCHIVE);
        s.select_channel(DESIGN);
    });
    app.pump();
    *a = app;
}

/// The channel settings dialog for #dev with the icon picker open on its Icons tab.
/// The look a scene is switched to after it is set up: Ocean on Cool, tinted. Pickers hold a
/// clone of the kit, so these scenes prove a clone still draws the chosen look.
fn ocean_cool(a: &mut App) {
    let spec = &mut a.state.ui.look.spec;
    (spec.accent, spec.background, spec.tint) = (AccentPreset::Ocean, BackgroundPreset::Cool, true);
}

fn channel_settings(a: &mut App, dark: bool, search: &str) {
    *a = main_screen(dark).0;
    a.act(|s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: DEV,
        })
    });
    a.act(|s| s.toggle_picker());
    if let Some(f) = a.state.channel_form() {
        f.search.replace_text(search);
        f.research();
    }
}

/// A channel's context menu, open where a right-click landed.
fn reorder_menu(a: &mut App, dark: bool) {
    *a = main_screen(dark).0;
    a.state.ui.menu = Menu::Channel {
        id: DESIGN,
        at: Some((110, 330)),
    };
}

/// The busy main screen with pictures: every member has an avatar, The Hideout an icon, role
/// colours show in the transcript and the member panel, and a mention of coloured members.
fn avatars_screen(dark: bool) -> (App, FakeBackend) {
    let (mut a, fake) = main_screen(dark);
    fake.give_avatars();
    fake.give_icon(1);
    fake.incoming(
        GENERAL,
        BRUNO,
        "@ana @cvetka the mockups are in #design, could you both take a look?",
    );
    fake.incoming(GENERAL, CVETKA, "on it. @bruno thanks for the heads up");
    fake.incoming(GENERAL, ANA, "@tim you can merge when ready");
    a.pump();
    a.state.data.media.settle();
    (a, fake)
}

fn main_avatars(a: &mut App, dark: bool) {
    let (mut app, _fake) = avatars_screen(dark);
    app.state.ui.members_open = true;
    *a = app;
}

/// The server settings page on `page`, for the owner of The Hideout.
fn server_settings(a: &mut App, dark: bool, page: ServerPage, role: Option<u64>) {
    let (mut app, _fake) = avatars_screen(dark);
    app.act(|s| {
        s.open_server_settings(1);
        s.server_page(page);
        if let Some(r) = role {
            s.select_role(r);
        }
    });
    app.pump();
    *a = app;
}

/// The same as Tim, a Roadie in Garage Band: roles above his own are locked.
fn server_settings_limited(a: &mut App, dark: bool) {
    let (mut app, fake) = booted(dark);
    fake.give_avatars();
    app.act(|s| s.go_guild(2));
    app.pump();
    app.act(|s| {
        s.open_server_settings(2);
        s.select_role(crate::fake::BAND_DRUMMER);
    });
    app.pump();
    app.state.data.media.settle();
    *a = app;
}

/// The profile dialog with a picture chosen and waiting to be saved.
fn profile_dialog(a: &mut App, dark: bool) {
    let (mut app, _fake) = avatars_screen(dark);
    let path = std::env::temp_dir().join(format!(
        "thehideout-profile-scene-{}.png",
        std::process::id()
    ));
    std::fs::write(&path, crate::media::generated_png(41, 96)).ok();
    app.act(|s| {
        s.open_profile();
        s.pick_image(crate::backend::ImagePurpose::Avatar, &path);
    });
    app.pump();
    app.state.data.media.settle();
    if let Some(d) = app
        .state
        .ui
        .dialog
        .as_ref()
        .and_then(|d| d.pick.draft.as_ref())
    {
        let _ = d.preview.wait();
    }
    std::fs::remove_file(&path).ok();
    *a = app;
}

// ---- emojis ----------------------------------------------------------------------------------

/// #memes open with the emojis fetched and decoded: custom ones in lines, jumbo messages, chips.
fn memes(dark: bool) -> (App, FakeBackend) {
    let (mut app, fake) = booted(dark);
    app.act(|s| s.select_channel(MEMES));
    app.pump();
    app.state.data.media.settle();
    (app, fake)
}

fn emoji_picker(a: &mut App, dark: bool) {
    let (mut app, _fake) = memes(dark);
    app.act(|s| s.open_picker(PickerTarget::Composer, "composer-emoji"));
    // A few recents, as if the user had been reacting.
    app.act(|s| {
        for key in [
            "u:\u{1f44d}",
            "c:7002",
            "u:\u{1f602}",
            "u:\u{2764}\u{fe0f}",
            "c:7001",
            "u:\u{1f389}",
        ]
        .iter()
        .rev()
        {
            if let Some(p) = thehideout_design::components::Pick::from_key(key, &s.server_emojis())
            {
                s.record_recent(&p);
            }
        }
    });
    app.state.data.media.settle();
    *a = app;
}

fn emoji_autocomplete(a: &mut App, dark: bool) {
    let (mut app, _fake) = memes(dark);
    let edit = app.state.composer_mut();
    edit.replace_text("that deserves a :sm");
    let end = edit.text.len();
    edit.set_caret(end);
    edit.focused = true;
    *a = app;
}

/// #general's bottom with reactions from several people: chips, one of them yours, and one picker
/// open from a message's "+" chip.
fn reactions(a: &mut App, dark: bool, picker: bool) {
    let (mut app, _fake) = memes(dark);
    app.act(|s| s.select_channel(GENERAL));
    app.pump();
    if picker {
        let target = app
            .state
            .current_channel()
            .and_then(|c| app.state.log(c))
            .and_then(|l| l.messages.iter().rev().nth(2))
            .map(|m| (m.channel_id, m.id));
        if let Some((channel, message)) = target {
            let anchor = format!("react-add-m:{message}");
            app.act(|s| s.open_picker(PickerTarget::Reaction { channel, message }, anchor));
        }
    }
    *a = app;
}

/// #memes with a run of jumbo messages at the bottom.
fn jumbo_emoji(a: &mut App, dark: bool) {
    let (mut app, fake) = memes(dark);
    fake.incoming(MEMES, ANA, "<:blob_love:7004>");
    fake.incoming(
        MEMES,
        ANA,
        "<a:dance_cat:7005><a:dance_cat:7005><a:dance_cat:7005>",
    );
    fake.incoming(MEMES, BRUNO, "\u{1f680} \u{2728} \u{1f525}");
    fake.incoming(MEMES, BRUNO, "<:smug_cat:7001> \u{1f60e}");
    app.pump();
    app.state.data.media.settle();
    *a = app;
}

fn server_settings_emojis(a: &mut App, dark: bool) {
    let (mut app, _fake) = memes(dark);
    // A GIF waiting to be named, as if just picked.
    let path = std::env::temp_dir().join(format!("Party Cat-{}.gif", std::process::id()));
    std::fs::write(&path, crate::media::generated_animated_emoji(77, 64, 8)).ok();
    app.act(|s| {
        s.open_server_settings(1);
        s.server_page(ServerPage::Emojis);
        s.pick_emoji_file(&path);
    });
    app.pump();
    app.state.data.media.settle();
    if let Some(d) = app
        .state
        .ui
        .server_settings
        .as_ref()
        .and_then(|s| s.emojis.draft.as_ref())
    {
        let _ = d.image.preview.wait();
    }
    std::fs::remove_file(&path).ok();
    *a = app;
}

/// #dev's settings dialog with the icon picker open on `tab` (1 Emoji, 2 Server emoji); the
/// sidebar already shows a Unicode emoji icon on #general and a custom one on #memes.
fn channel_icon_emoji(a: &mut App, dark: bool, tab: usize) {
    let (mut app, _fake) = memes(dark);
    let set = |channel: u64, icon: thehideout_api::ChannelIconRef| Command::UpdateChannel {
        guild: 1,
        channel,
        name: None,
        icon: Some(icon),
        color: None,
        topic: None,
    };
    app.state.outbox.push(set(
        GENERAL,
        thehideout_api::ChannelIconRef::Unicode("\u{1f4ac}".into()),
    ));
    app.state
        .outbox
        .push(set(MEMES, thehideout_api::ChannelIconRef::Custom(SMUG_CAT)));
    app.act(|s| s.select_channel(DEV));
    app.pump();
    app.state.data.media.settle();
    app.act(|s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: DEV,
        })
    });
    app.act(|s| s.toggle_picker());
    app.act(|s| s.set_picker_tab(tab));
    app.state.data.media.settle();
    *a = app;
}

// ---- site admin ------------------------------------------------------------------------------

/// The admin dashboard signed in as cvetka (the fake site admin), on `page` with its data in.
fn admin_at(dark: bool, page: AdminPage) -> App {
    let (mut app, _fake) = booted_as(dark, "cvetka");
    app.act(|s| s.open_admin());
    app.pump();
    app.act(|s| s.admin_page(page));
    app.pump();
    app.state.data.media.settle();
    app
}

fn admin_overview(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Overview);
}

fn admin_servers(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Servers);
}

fn admin_files(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Files);
}

fn admin_users(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Users);
}

fn admin_audit(a: &mut App, dark: bool) {
    *a = admin_at(dark, AdminPage::Audit);
}

/// The Files page with the delete confirmation open on the largest stored file, reason typed.
fn admin_files_delete(a: &mut App, dark: bool) {
    let mut app = admin_at(dark, AdminPage::Files);
    app.act(|s| {
        let key = s
            .admin()
            .and_then(|ad| ad.files.items.first().map(|r| r.key.clone()))
            .unwrap_or_default();
        s.ask_delete_files(vec![key]);
        if let Some(Confirm::DeleteFiles(d)) =
            s.ui.admin.as_mut().and_then(|ad| ad.confirm.as_mut())
        {
            d.reason.replace_text("spam");
        }
    });
    app.pump();
    *a = app;
}

/// A server without the admin service: the dashboard says so.
fn admin_unavailable(a: &mut App, dark: bool) {
    let (mut app, fake) = booted_as(dark, "cvetka");
    fake.admin_unavailable(true);
    app.act(|s| s.open_admin());
    app.pump();
    *a = app;
}

// ---- attachments and embeds ------------------------------------------------------------------

/// A file of `size` bytes (sparse) in the temp directory, for the composer scenes.
fn temp_file(name: &str, size: u64, picture: Option<(u64, u32, u32)>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("thehideout-scene-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join(name);
    match picture {
        Some((seed, w, h)) => std::fs::write(&path, crate::media::generated_picture(seed, w, h)).ok(),
        None => std::fs::File::create(&path).and_then(|f| f.set_len(size)).ok(),
    };
    path
}

/// #plans (empty in the seed) with its composer holding files at every stage: a picture and a
/// PDF that are up, a zip at 60%, and one that failed.
fn attachments_composer(a: &mut App, dark: bool) {
    let (mut app, fake) = booted(dark);
    app.act(|s| s.select_channel(PLANS));
    app.pump();
    let photo = temp_file("harbour-evening.png", 0, Some((5, 640, 420)));
    let pdf = temp_file("Q4-roadmap.pdf", 2_516_582, None);
    app.act(|s| s.attach_paths(&[photo, pdf]));
    app.pump();
    fake.stall_uploads(Some(60));
    app.act(|s| s.attach_paths(&[temp_file("asset-bundle.zip", 18_874_368, None)]));
    app.pump();
    fake.stall_uploads(None);
    fake.fail_uploads(1);
    app.act(|s| s.attach_paths(&[temp_file("screen-recording.mov", 6_291_456, None)]));
    app.pump();
    app.state.data.media.settle();
    app.state.composer_mut().replace_text("Here is everything for the review");
    *a = app;
}

/// #plans with `parts` of the seeded attachments, the picture caches filled.
fn attachments_in_plans(dark: bool, parts: &[&str]) -> (App, FakeBackend) {
    let (mut app, fake) = booted(dark);
    fake.seed_attachments(PLANS, parts);
    app.act(|s| s.select_channel(PLANS));
    app.pump();
    app.state.data.media.settle();
    (app, fake)
}

fn attachments_messages(a: &mut App, dark: bool) {
    *a = attachments_in_plans(dark, &["one", "video", "docs", "audio", "removed"]).0;
}

fn attachments_images(a: &mut App, dark: bool) {
    *a = attachments_in_plans(dark, &["three", "four"]).0;
}

fn lightbox(a: &mut App, dark: bool) {
    let (mut app, _fake) = attachments_in_plans(dark, &["three"]);
    let (message, attachment) = {
        let s = &app.state;
        let m = s.log(PLANS).and_then(|l| l.messages.last()).expect("the seeded message");
        (m.id, m.attachments[1].id)
    };
    app.act(|s| s.open_lightbox_attachment(message, attachment));
    app.pump();
    app.state.data.media.settle();
    *a = app;
}

/// #plans with `which` embeds, the picture caches filled.
fn embeds_in_plans(dark: bool, which: &[&str]) -> (App, FakeBackend) {
    let (mut app, fake) = booted(dark);
    fake.seed_embeds(PLANS, which);
    app.act(|s| s.select_channel(PLANS));
    app.pump();
    app.state.data.media.settle();
    (app, fake)
}

fn embeds(a: &mut App, dark: bool) {
    *a = embeds_in_plans(dark, &["link", "x"]).0;
}

fn embeds_gallery(a: &mut App, dark: bool) {
    *a = embeds_in_plans(dark, &["reddit", "big"]).0;
}

fn embeds_collapsed(a: &mut App, dark: bool) {
    let (mut app, _fake) = embeds_in_plans(dark, &[]);
    let ids: Vec<u64> = app.state.log(PLANS).map(|l| l.messages.iter().map(|m| m.id).collect()).unwrap_or_default();
    app.act(|s| ids.iter().for_each(|id| s.toggle_embed(*id, 0)));
    *a = app;
}

/// The X video started: the scrub bar, the time and the mute button over the picture.
fn video_embed(a: &mut App, dark: bool) {
    let (mut app, _fake) = embeds_in_plans(dark, &["x"]);
    // Playback itself can't be decoded headless, so the controls are pinned mid-clip.
    app.state.ui.attach.pinned.insert(
        "fx-clip-x".into(),
        thehideout_design::components::VideoUi { playing: true, time: 1.4, duration: 3.0, muted: true, loading: false, failed: false },
    );
    app.pump();
    *a = app;
}

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("auth-signin", |a| auth_login(a, true)),
    ("auth-signin-light", |a| auth_login(a, false)),
    ("auth-signup-error", |a| auth_signup_error(a, true)),
    ("auth-signup-error-light", |a| auth_signup_error(a, false)),
    ("auth-code", |a| auth_code(a, true)),
    ("auth-code-light", |a| auth_code(a, false)),
    ("auth-second-factor", |a| auth_second_factor(a, true)),
    ("auth-second-factor-light", |a| auth_second_factor(a, false)),
    ("auth-invite-lookup", |a| auth_invite_lookup(a, true)),
    ("auth-invite-lookup-light", |a| auth_invite_lookup(a, false)),
    ("auth-invite-accept", |a| auth_invite_accept(a, true)),
    ("auth-invite-accept-light", |a| auth_invite_accept(a, false)),
    ("auth-passkey-waiting", |a| auth_passkey_waiting(a, true)),
    ("auth-passkey-waiting-light", |a| auth_passkey_waiting(a, false)),
    ("add-email-banner", |a| add_email_banner(a, true)),
    ("add-email-banner-light", |a| add_email_banner(a, false)),
    ("settings-security", |a| settings_security(a, true)),
    ("settings-security-light", |a| settings_security(a, false)),
    ("settings-security-totp-setup", |a| settings_security_totp_setup(a, true)),
    ("settings-security-totp-setup-light", |a| settings_security_totp_setup(a, false)),
    ("settings-security-recovery-codes", |a| settings_security_recovery_codes(a, true)),
    ("settings-security-recovery-codes-light", |a| settings_security_recovery_codes(a, false)),
    ("invite-by-email", |a| invite_by_email(a, true)),
    ("invite-by-email-light", |a| invite_by_email(a, false)),
    ("admin-invites", |a| admin_invites(a, true)),
    ("admin-invites-light", |a| admin_invites(a, false)),
    ("main", |a| main_(a, true)),
    ("main-light", |a| main_(a, false)),
    ("main-members", |a| main_members(a, true)),
    ("main-members-light", |a| main_members(a, false)),
    ("home-dms", |a| home_dms(a, true)),
    ("home-dms-light", |a| home_dms(a, false)),
    ("home-empty", |a| home_empty(a, true)),
    ("home-empty-light", |a| home_empty(a, false)),
    ("toast", |a| toast(a, true)),
    ("toast-light", |a| toast(a, false)),
    ("empty-channel", |a| empty_channel(a, true)),
    ("empty-channel-light", |a| empty_channel(a, false)),
    ("dialog-create-server", |a| dialog_create_server(a, true)),
    ("dialog-create-server-light", |a| {
        dialog_create_server(a, false)
    }),
    ("reconnecting", |a| reconnecting(a, true)),
    ("reconnecting-light", |a| reconnecting(a, false)),
    ("settings-appearance", |a| settings_appearance(a, true)),
    ("settings-appearance-light", |a| {
        settings_appearance(a, false)
    }),
    ("main-compact", |a| main_sized(a, true, Density::Compact, TextSize::P100)),
    ("main-compact-light", |a| main_sized(a, false, Density::Compact, TextSize::P100)),
    ("main-text-125", |a| main_sized(a, true, Density::Comfortable, TextSize::P125)),
    ("main-text-125-light", |a| main_sized(a, false, Density::Comfortable, TextSize::P125)),
    ("main-ocean-cool", |a| {
        main_look(a, true, AccentPreset::Ocean, BackgroundPreset::Cool, true)
    }),
    ("main-coral-paper-light", |a| {
        main_look(a, false, AccentPreset::Coral, BackgroundPreset::Paper, true)
    }),
    ("main-graphite-deep", |a| {
        main_look(
            a,
            true,
            AccentPreset::Graphite,
            BackgroundPreset::Deep,
            false,
        )
    }),
    ("sections", |a| sections(a, true)),
    ("sections-light", |a| sections(a, false)),
    ("channel-settings", |a| channel_settings(a, true, "")),
    ("channel-settings-ocean", |a| {
        channel_settings(a, true, "");
        ocean_cool(a);
    }),
    ("channel-settings-light", |a| channel_settings(a, false, "")),
    ("icon-picker-search", |a| channel_settings(a, true, "game")),
    ("icon-picker-search-light", |a| {
        channel_settings(a, false, "game")
    }),
    ("reorder-menu", |a| reorder_menu(a, true)),
    ("reorder-menu-light", |a| reorder_menu(a, false)),
    ("main-avatars", |a| main_avatars(a, true)),
    ("main-avatars-light", |a| main_avatars(a, false)),
    ("server-settings-overview", |a| {
        server_settings(a, true, ServerPage::Overview, None)
    }),
    ("server-settings-overview-light", |a| {
        server_settings(a, false, ServerPage::Overview, None)
    }),
    ("server-settings-roles", |a| {
        server_settings(a, true, ServerPage::Roles, Some(HIDEOUT_MODS))
    }),
    ("server-settings-roles-light", |a| {
        server_settings(a, false, ServerPage::Roles, Some(HIDEOUT_DESIGNERS))
    }),
    ("server-settings-roles-limited", |a| {
        server_settings_limited(a, true)
    }),
    ("server-settings-roles-limited-light", |a| {
        server_settings_limited(a, false)
    }),
    ("server-settings-members", |a| {
        server_settings(a, true, ServerPage::Members, None)
    }),
    ("server-settings-members-light", |a| {
        server_settings(a, false, ServerPage::Members, None)
    }),
    ("profile-dialog", |a| profile_dialog(a, true)),
    ("profile-dialog-light", |a| profile_dialog(a, false)),
    ("emoji-picker", |a| emoji_picker(a, true)),
    ("emoji-picker-ocean", |a| {
        emoji_picker(a, true);
        ocean_cool(a);
    }),
    ("emoji-picker-light", |a| emoji_picker(a, false)),
    ("emoji-autocomplete", |a| emoji_autocomplete(a, true)),
    ("emoji-autocomplete-light", |a| emoji_autocomplete(a, false)),
    ("reactions", |a| reactions(a, true, false)),
    ("reactions-light", |a| reactions(a, false, false)),
    ("reactions-picker", |a| reactions(a, true, true)),
    ("jumbo-emoji", |a| jumbo_emoji(a, true)),
    ("jumbo-emoji-light", |a| jumbo_emoji(a, false)),
    ("server-settings-emojis", |a| {
        server_settings_emojis(a, true)
    }),
    ("server-settings-emojis-light", |a| {
        server_settings_emojis(a, false)
    }),
    ("channel-icon-emoji-tabs", |a| {
        channel_icon_emoji(a, true, 2)
    }),
    ("channel-icon-emoji-tabs-light", |a| {
        channel_icon_emoji(a, false, 2)
    }),
    ("channel-icon-unicode-tab", |a| {
        channel_icon_emoji(a, true, 1)
    }),
    ("attachments-composer", |a| attachments_composer(a, true)),
    ("attachments-composer-light", |a| attachments_composer(a, false)),
    ("attachments-messages", |a| attachments_messages(a, true)),
    ("attachments-messages-light", |a| attachments_messages(a, false)),
    ("attachments-images", |a| attachments_images(a, true)),
    ("attachments-images-light", |a| attachments_images(a, false)),
    ("lightbox", |a| lightbox(a, true)),
    ("lightbox-light", |a| lightbox(a, false)),
    ("embeds", |a| embeds(a, true)),
    ("embeds-light", |a| embeds(a, false)),
    ("embeds-gallery", |a| embeds_gallery(a, true)),
    ("embeds-gallery-light", |a| embeds_gallery(a, false)),
    ("embeds-collapsed", |a| embeds_collapsed(a, true)),
    ("embeds-collapsed-light", |a| embeds_collapsed(a, false)),
    ("video-embed", |a| video_embed(a, true)),
    ("video-embed-light", |a| video_embed(a, false)),
    ("main-small", |a| main_(a, true)),
    ("main-small-light", |a| main_(a, false)),
    ("admin-overview", |a| admin_overview(a, true)),
    ("admin-overview-light", |a| admin_overview(a, false)),
    ("admin-servers", |a| admin_servers(a, true)),
    ("admin-servers-light", |a| admin_servers(a, false)),
    ("admin-files", |a| admin_files(a, true)),
    ("admin-files-light", |a| admin_files(a, false)),
    ("admin-files-delete", |a| admin_files_delete(a, true)),
    ("admin-files-delete-light", |a| admin_files_delete(a, false)),
    ("admin-users", |a| admin_users(a, true)),
    ("admin-users-light", |a| admin_users(a, false)),
    ("admin-audit", |a| admin_audit(a, true)),
    ("admin-audit-light", |a| admin_audit(a, false)),
    ("admin-unavailable", |a| admin_unavailable(a, true)),
    ("admin-unavailable-light", |a| admin_unavailable(a, false)),
];
