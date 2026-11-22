//! Headless proofs of the app, driven through the real view with pointer and key input against the
//! fake backend: the login flow, sending, retry, drafts, unread, dialogs, paging and the motion
//! the app adds. Entrances only play in a commit an input caused, so these use input.

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use thehideout_api::GatewayUpdate;
use thehideout_design::components::ConnectionStatus;
use thehideout_design::{Density, Metrics, TextSize};

use crate::app::{fake_now, on_key, poll, view, App};
use crate::backend::Command;
use crate::fake::{FakeBackend, ANA, BRUNO, CVETKA, DEV, GENERAL, RANDOM};
use crate::format::Tz;
use crate::scenes::booted;
use crate::state::{AppState, Delivery, Phase, Place, Screen};

const VIEWPORT: (f32, f32) = (1280.0, 800.0);

pub(crate) fn runtime(app: App) -> Runtime<App> {
    let mut rt = Runtime::new(
        app,
        view,
        VIEWPORT,
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    settle(&mut rt, 3);
    rt
}

/// A signed-in app on the fake world (the data is already in), with replies `latency` seconds late.
pub(crate) fn signed_in(latency: f64) -> (Runtime<App>, FakeBackend) {
    let (app, fake) = booted(true);
    fake.set_latency(latency);
    (runtime(app), fake)
}

pub(crate) fn settle(rt: &mut Runtime<App>, frames: usize) {
    for _ in 0..frames {
        rt.frame(1.0 / 60.0);
    }
}

pub(crate) fn click_at(rt: &mut Runtime<App>, (x, y): (f32, f32)) {
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
}

pub(crate) fn click(rt: &mut Runtime<App>, key: &str) {
    let at = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    click_at(rt, at);
}

pub(crate) fn key(rt: &mut Runtime<App>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
}

pub(crate) fn type_text(rt: &mut Runtime<App>, s: &str) {
    key(rt, Key::Text(s.into()));
}

pub(crate) fn rect_of(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    rt.presented(key)
        .unwrap_or_else(|| panic!("no element {key}"))
        .0
}

pub(crate) fn has(rt: &Runtime<App>, key: &str) -> bool {
    rt.find(key).is_some()
}

fn send(rt: &mut Runtime<App>, text: &str) {
    click(rt, "composer/scroll/input");
    type_text(rt, text);
    key(rt, Key::Enter);
}

fn general_contents(rt: &Runtime<App>, content: &str) -> usize {
    rt.state
        .state
        .log(GENERAL)
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.content == content)
        .count()
}

// ---- auth ------------------------------------------------------------------------------------

#[test]
fn logging_in_with_the_keyboard_reaches_the_main_screen() {
    let state = AppState::new(fake_now, Tz::Fixed(0));
    let app = App::new(Box::new(FakeBackend::new().with_latency(0.25)), state);
    let mut rt = runtime(app);
    assert_eq!(rt.state.state.screen, Screen::Auth);

    click(&mut rt, "auth-username/value/input");
    type_text(&mut rt, "tim");
    click(&mut rt, "auth-password/value/input");
    type_text(&mut rt, "hunter22");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 2);
    assert_eq!(
        rt.state.state.auth.phase,
        Phase::Busy,
        "the button waits while the request is out"
    );
    assert_eq!(rt.state.state.screen, Screen::Auth);

    settle(&mut rt, 60);
    assert_eq!(rt.state.state.screen, Screen::Main);
    assert_eq!(rt.state.state.me().unwrap().username, "tim");
    assert_eq!(rt.state.state.ui.place, Place::Guild(1));
    assert!(
        has(&rt, "channels-1-row-general"),
        "the sidebar lists the server's channels"
    );
}

#[test]
fn wrong_password_and_taken_name_show_inline_errors() {
    let state = AppState::new(fake_now, Tz::Fixed(0));
    let mut rt = runtime(App::new(Box::new(FakeBackend::new()), state));
    rt.update(|a, _| {
        a.state.auth.username.replace_text("tim");
        a.state.auth.password.replace_text("wrong");
    });
    click(&mut rt, "auth-submit");
    settle(&mut rt, 8);
    assert!(
        has(&rt, "auth-form-error"),
        "Unauthenticated lands under the button"
    );
    assert_eq!(rt.state.state.auth.phase, Phase::Idle);

    // Accounts come from invites: the invite tab, its code, then a name that is taken.
    settle(&mut rt, 30);
    click(&mut rt, "auth-link-invite");
    settle(&mut rt, 30);
    assert_eq!(rt.state.state.auth.mode, crate::forms::AuthMode::Invite);
    rt.update(|a, _| a.state.auth.invite.replace_text(crate::fake::account::INVITE_PENDING));
    click(&mut rt, "auth-submit");
    // The card grows to the account step; let it settle before pointing at its button.
    settle(&mut rt, 40);
    assert!(matches!(rt.state.state.auth.step, crate::forms::AuthStep::InviteAccept { .. }));
    rt.update(|a, _| {
        a.state.auth.username.replace_text("taken");
        a.state.auth.password.replace_text("secret123");
        a.state.auth.accept_terms = true;
        a.state.auth.confirm_age = true;
    });
    settle(&mut rt, 3);
    click(&mut rt, "auth-submit");
    settle(&mut rt, 8);
    assert!(
        has(&rt, "auth-username/error"),
        "AlreadyExists lands under the username"
    );
}

#[test]
fn the_shell_fades_in_after_login_and_nothing_else_animates_on_first_frame() {
    let state = AppState::new(fake_now, Tz::Fixed(0));
    let mut rt = runtime(App::new(
        Box::new(FakeBackend::new().with_latency(0.1)),
        state,
    ));
    rt.update(|a, _| {
        a.state.auth.username.replace_text("tim");
        a.state.auth.password.replace_text("hunter22");
    });
    click(&mut rt, "auth-submit");
    let mut seen = Vec::new();
    for _ in 0..40 {
        settle(&mut rt, 1);
        if let Some((_, o)) = rt.presented("screen-main") {
            seen.push(o);
        }
    }
    assert!(
        seen.iter().any(|&o| o > 0.0 && o < 1.0),
        "the shell crossfades in: {seen:?}"
    );
    assert!(*seen.last().unwrap() > 0.999);

    let (app, _) = booted(true);
    let mut rt = Runtime::new(
        app,
        view,
        VIEWPORT,
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.frame(0.0);
    assert_eq!(
        rt.presented("screen-main").unwrap().1,
        1.0,
        "a scene's first frame is fully drawn"
    );
    assert_eq!(rt.presented("stage-10").unwrap().1, 1.0);
}

// ---- sending ---------------------------------------------------------------------------------

#[test]
fn enter_sends_a_pending_message_that_becomes_sent_without_a_duplicate() {
    let (mut rt, _) = signed_in(2.0);
    send(&mut rt, "hello from the test");
    settle(&mut rt, 2);
    assert_eq!(
        rt.state.state.composer().text,
        "",
        "the field cleared with the send"
    );
    let o = rt.state.state.data.outgoing.clone();
    assert_eq!((o.len(), o[0].state), (1, Delivery::Pending));
    let row = format!("msg-n:{}", o[0].nonce);
    assert!(has(&rt, &row), "the pending message is on screen at once");
    let body = |rt: &Runtime<App>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("body") && e.path.contains(&format!("{row}/")))
            .unwrap()
            .opacity
    };
    settle(&mut rt, 40);
    assert!((body(&rt) - 0.55).abs() < 0.02, "pending rests dimmed");
    let before = rect_of(&rt, &row);

    // The reply and the gateway echo both arrive in the same poll.
    settle(&mut rt, 140);
    assert!(rt.state.state.data.outgoing.is_empty());
    assert_eq!(
        general_contents(&rt, "hello from the test"),
        1,
        "one message, not two"
    );
    assert!(has(&rt, &row), "the row keeps its id from pending to sent");
    settle(&mut rt, 60);
    assert!((body(&rt) - 1.0).abs() < 0.01, "sent is fully opaque");
    let after = rect_of(&rt, &row);
    assert!(
        (before[1] - after[1]).abs() < 1.0 && (before[3] - after[3]).abs() < 0.01,
        "the row doesn't move: {before:?} {after:?}"
    );
    assert_eq!(fake_sends(&rt), 1);
}

fn fake_sends(rt: &Runtime<App>) -> usize {
    rt.state
        .backend
        .sent()
        .iter()
        .filter(|c| matches!(c, Command::Send { .. }))
        .count()
}

#[test]
fn a_whitespace_message_is_not_sent() {
    let (mut rt, _) = signed_in(0.0);
    send(&mut rt, "   ");
    settle(&mut rt, 4);
    assert_eq!(fake_sends(&rt), 0);
    assert!(rt.state.state.data.outgoing.is_empty());
}

#[test]
fn a_failed_send_offers_retry_and_retry_sends_it() {
    let (mut rt, fake) = signed_in(0.0);
    fake.fail_sends(1);
    send(&mut rt, "second try");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.data.outgoing[0].state, Delivery::Failed);
    let retry = rt
        .elements()
        .into_iter()
        .find(|e| e.label.as_deref() == Some("Retry sending"))
        .expect("a Retry link");
    click_at(
        &mut rt,
        (
            retry.rect[0] + retry.rect[2] / 2.0,
            retry.rect[1] + retry.rect[3] / 2.0,
        ),
    );
    settle(&mut rt, 10);
    assert!(
        rt.state.state.data.outgoing.is_empty(),
        "the retry went through"
    );
    assert_eq!(general_contents(&rt, "second try"), 1);
    assert_eq!(fake_sends(&rt), 2);
}

#[test]
fn text_over_the_limit_is_kept_and_not_sent() {
    let (mut rt, _) = signed_in(0.0);
    let long = "x".repeat(thehideout_api::MAX_MESSAGE_CHARS + 1);
    rt.update(|a, _| a.state.composer_mut().replace_text(long.clone()));
    settle(&mut rt, 2);
    assert!(
        has(&rt, "composer-area/counter"),
        "the counter shows near the limit"
    );
    click(&mut rt, "composer/scroll/input");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 4);
    assert_eq!(fake_sends(&rt), 0);
    assert_eq!(rt.state.state.composer().text, long, "the draft comes back");
}

// ---- navigation and unread -------------------------------------------------------------------

#[test]
fn switching_channel_keeps_each_drafts() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, "half a thought");
    click(&mut rt, "channels-1-row-dev");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.current_channel(), Some(DEV));
    assert_eq!(rt.state.state.composer().text, "", "dev starts empty");
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, "dev note");
    click(&mut rt, "channels-1-row-general");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.composer().text, "half a thought");
    click(&mut rt, "channels-1-row-dev");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.composer().text, "dev note");
}

#[test]
fn alt_arrows_move_between_channels() {
    let (mut rt, _) = signed_in(0.0);
    rt.key(KeyEvent {
        key: Key::Down,
        mods: Mods {
            alt: true,
            ..Mods::default()
        },
    });
    settle(&mut rt, 4);
    assert_eq!(rt.state.state.current_channel(), Some(DEV));
    rt.key(KeyEvent {
        key: Key::Up,
        mods: Mods {
            alt: true,
            ..Mods::default()
        },
    });
    settle(&mut rt, 4);
    assert_eq!(rt.state.state.current_channel(), Some(GENERAL));
}

#[test]
fn an_incoming_message_elsewhere_counts_unread_and_visiting_clears_it() {
    let (mut rt, fake) = signed_in(0.0);
    fake.incoming(RANDOM, CVETKA, "look at this");
    fake.incoming(DEV, ANA, "@tim review please");
    fake.incoming(20, ANA, "rehearsal at seven");
    settle(&mut rt, 4);
    let s = &rt.state.state;
    assert_eq!(
        (s.unread_of(RANDOM).count, s.unread_of(DEV).mentions),
        (1, 1)
    );
    assert_eq!(s.guild_unread(2).count, 1);
    let under = |rt: &Runtime<App>, row: &str, label: &str| {
        rt.elements()
            .into_iter()
            .any(|e| e.label.as_deref() == Some(label) && e.path.contains(&format!("{row}/")))
    };
    assert!(
        under(&rt, "channels-1-row-random", "unread"),
        "an unread dot in the sidebar"
    );
    assert!(
        under(&rt, "channels-1-row-dev", "1 mention"),
        "a mention count in the sidebar"
    );
    let bar = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("rail-bar") && e.path.contains("guild-2/"))
        .expect("a bar on the other server's badge");
    assert_eq!(bar.target[3], 4.0, "the rail shows its unread dot");

    click(&mut rt, "channels-1-row-dev");
    settle(&mut rt, 60);
    assert_eq!(rt.state.state.unread_of(DEV).count, 0);
    assert!(!under(&rt, "channels-1-row-dev", "1 mention"));
    assert!(has(&rt, "stage-11"));
    assert_eq!(
        rt.state.state.guild_unread(1).count,
        1,
        "random is still unread"
    );
}

#[test]
fn a_new_mention_badge_pops_in_and_a_plain_one_does_not_bounce() {
    let (mut rt, fake) = signed_in(0.0);
    fake.incoming(DEV, BRUNO, "@tim ping");
    settle(&mut rt, 2);
    let pill = |rt: &Runtime<App>| {
        rt.elements().into_iter().find(|e| {
            e.label.as_deref() == Some("1 mention") && e.path.contains("channels-1-row-dev/")
        })
    };
    let mid = pill(&rt).expect("the pill mounted");
    assert!(
        mid.scale < 1.0 && mid.scale >= 0.6 || mid.opacity < 1.0,
        "pop in progress: scale {} opacity {}",
        mid.scale,
        mid.opacity
    );
    settle(&mut rt, 60);
    let rest = pill(&rt).unwrap();
    assert!((rest.scale - 1.0).abs() < 1e-3 && rest.opacity == 1.0);
}

#[test]
fn switching_channel_crossfades_the_transcript_without_sliding() {
    let (mut rt, _) = signed_in(0.0);
    let from = rect_of(&rt, "stage-10");
    click(&mut rt, "channels-1-row-dev");
    settle(&mut rt, 3);
    let (new_rect, new_opacity) = rt.presented("stage-11").expect("the new stage mounts");
    let (old_rect, old_opacity) = rt
        .presented("stage-10")
        .expect("the old stage is still fading");
    assert!(
        new_opacity > 0.0 && new_opacity < 1.0,
        "fading in: {new_opacity}"
    );
    assert!(
        old_opacity > 0.0 && old_opacity < 1.0,
        "fading out: {old_opacity}"
    );
    assert_eq!((new_rect, old_rect), (from, from), "neither slides");
    settle(&mut rt, 60);
    assert!(!has(&rt, "stage-10"));
    assert_eq!(rt.presented("stage-11").unwrap().1, 1.0);
}

#[test]
fn switching_server_crossfades_the_sidebar() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "guild-2/face");
    settle(&mut rt, 3);
    let new = rt
        .presented("sidebar-guild-2")
        .expect("the new body mounts")
        .1;
    let old = rt
        .presented("sidebar-guild-1")
        .expect("the old body is fading")
        .1;
    assert!(
        new > 0.0 && new < 1.0 && old > 0.0 && old < 1.0,
        "{new} {old}"
    );
    settle(&mut rt, 60);
    assert!(has(&rt, "channels-2-row-gigs"));
    assert_eq!(rt.state.state.ui.place, Place::Guild(2));
}

// ---- dialogs ---------------------------------------------------------------------------------

#[test]
fn creating_a_server_adds_it_to_the_rail_and_selects_it() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "rail-add");
    settle(&mut rt, 8);
    click(&mut rt, "rail-menu-item0");
    settle(&mut rt, 8);
    assert!(
        rt.state.state.ui.dialog.is_some(),
        "Create server opens its dialog"
    );
    assert!(has(&rt, "dlg-dialog"));

    key(&mut rt, Key::Enter);
    settle(&mut rt, 3);
    assert!(
        rt.state.state.ui.dialog.as_ref().unwrap().error.is_some(),
        "an empty name is refused inline"
    );
    assert_eq!(
        rt.state
            .backend
            .sent()
            .iter()
            .filter(|c| matches!(c, Command::CreateGuild { .. }))
            .count(),
        0
    );

    type_text(&mut rt, "Climbers");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 20);
    let s = &rt.state.state;
    let guild = s.data.guilds.last().unwrap().clone();
    assert_eq!(guild.name, "Climbers");
    assert!(s.ui.dialog.is_none(), "the dialog closes on success");
    assert_eq!(
        s.ui.place,
        Place::Guild(guild.id),
        "and the new server is open"
    );
    assert!(s.current_channel().is_some(), "on its first channel");
    assert_eq!(
        s.ui.toasts.front().map(|t| t.message.as_str()),
        Some("Server created")
    );
    assert!(
        has(&rt, &format!("guild-{}", guild.id)),
        "it is in the rail"
    );
    assert!(has(&rt, "toast-1"));
}

#[test]
fn escape_closes_a_dialog() {
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.state.open_dialog(crate::state::DialogKind::JoinGuild));
    settle(&mut rt, 6);
    assert!(has(&rt, "dlg-dialog"));
    key(&mut rt, Key::Escape);
    settle(&mut rt, 6);
    assert!(rt.state.state.ui.dialog.is_none());
}

#[test]
fn joining_with_a_bad_code_shows_the_error_and_stays_open() {
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.state.open_dialog(crate::state::DialogKind::JoinGuild));
    settle(&mut rt, 6);
    type_text(&mut rt, "NOPE");
    key(&mut rt, Key::Enter);
    settle(&mut rt, 8);
    let d = rt.state.state.ui.dialog.as_ref().expect("still open");
    assert!(d.error.as_deref().unwrap().contains("invite code"));
    assert_eq!(d.phase, Phase::Idle);
}

#[test]
fn a_new_message_dialog_opens_a_dm() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "home/face");
    settle(&mut rt, 6);
    click(&mut rt, "new-dm");
    settle(&mut rt, 8);
    click(&mut rt, "dm-user-3");
    settle(&mut rt, 20);
    let s = &rt.state.state;
    assert!(s.ui.dialog.is_none());
    assert_eq!(s.ui.place, Place::Home);
    assert_eq!(s.current_channel(), Some(crate::fake::DM_BRUNO));
}

// ---- transcript ------------------------------------------------------------------------------

/// Wheels up until the transcript asks for an older page.
fn scroll_until_older_requested(rt: &mut Runtime<App>) {
    let (x, y) = rt.find("transcript-area").unwrap();
    for _ in 0..12 {
        rt.wheel(x + 100.0, y + 100.0, 1500.0);
        settle(rt, 4);
        if rt.state.backend.sent().iter().any(|c| {
            matches!(
                c,
                Command::LoadMessages {
                    before: Some(_),
                    ..
                }
            )
        }) {
            return;
        }
    }
}

#[test]
fn scrolling_to_the_top_loads_the_next_older_page_and_holds_the_reader_place() {
    let (mut rt, _) = signed_in(0.0);
    settle(&mut rt, 10);
    let first = rt
        .state
        .state
        .log(GENERAL)
        .unwrap()
        .messages
        .first()
        .unwrap()
        .id;
    assert_eq!(
        rt.state.state.log(GENERAL).unwrap().messages.len(),
        50,
        "one page at first"
    );
    assert!(rt.state.state.log(GENERAL).unwrap().has_more);
    scroll_until_older_requested(&mut rt);
    let row = format!("msg-m:{first}");
    let held = rect_of(&rt, &row)[1];
    let asked = rt.state.backend.sent().into_iter().find(|c| {
        matches!(
            c,
            Command::LoadMessages {
                channel: GENERAL,
                before: Some(_)
            }
        )
    });
    assert_eq!(
        asked,
        Some(Command::LoadMessages {
            channel: GENERAL,
            before: Some(first)
        })
    );
    settle(&mut rt, 30);
    assert_eq!(
        rt.state.state.log(GENERAL).unwrap().messages.len(),
        100,
        "the older page is in, and only one page"
    );
    // The row the reader was looking at is still exactly where it was, not shoved down a page.
    let after = rect_of(&rt, &row)[1];
    assert!(
        (after - held).abs() < 1.0,
        "row {first} moved from {held} to {after}"
    );
}

#[test]
fn a_message_while_scrolled_up_shows_the_pill_and_the_pill_goes_to_the_end() {
    let (mut rt, fake) = signed_in(0.0);
    let (x, y) = rt.find("transcript-area").unwrap();
    rt.wheel(x + 100.0, y + 100.0, 600.0);
    settle(&mut rt, 10);
    assert!(
        !rt.state.state.ui.at_bottom,
        "the reader is away from the end"
    );
    fake.incoming(GENERAL, BRUNO, "are you still there?");
    settle(&mut rt, 4);
    assert!(has(&rt, "new-pill"), "a pill says there is more below");
    click(&mut rt, "new-pill");
    settle(&mut rt, 80);
    assert!(rt.state.state.ui.at_bottom);
    assert!(!has(&rt, "new-pill"));
    let last = rt
        .state
        .state
        .log(GENERAL)
        .unwrap()
        .messages
        .last()
        .unwrap()
        .id;
    let r = rect_of(&rt, &format!("msg-m:{last}"));
    let area = rect_of(&rt, "transcript-area");
    assert!(
        r[1] + r[3] <= area[1] + area[3] + 1.0,
        "the newest message is in view"
    );
}

#[test]
fn at_the_bottom_a_new_message_sticks_and_shows_no_pill() {
    let (mut rt, fake) = signed_in(0.0);
    fake.incoming(GENERAL, BRUNO, "right behind you");
    settle(&mut rt, 60);
    assert!(!has(&rt, "new-pill"));
    let last = rt
        .state
        .state
        .log(GENERAL)
        .unwrap()
        .messages
        .last()
        .unwrap()
        .id;
    let r = rect_of(&rt, &format!("msg-m:{last}"));
    let area = rect_of(&rt, "transcript-area");
    assert!(
        r[1] + r[3] <= area[1] + area[3] + 1.0 && r[1] >= area[1],
        "stuck to the bottom: {r:?} in {area:?}"
    );
}

#[test]
fn an_empty_channel_shows_the_empty_state() {
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.act(|s| s.select_channel(crate::fake::PLANS)));
    settle(&mut rt, 10);
    assert!(rt.state.state.log(crate::fake::PLANS).unwrap().loaded);
    assert!(rt
        .elements()
        .into_iter()
        .any(|e| e.text.as_deref() == Some("No messages yet")));
}

#[test]
fn a_slow_load_shows_the_placeholder_and_a_fast_one_never_does() {
    let (mut rt, fake) = signed_in(0.0);
    fake.set_latency(0.05);
    click(&mut rt, "channels-1-row-dev");
    settle(&mut rt, 20);
    assert!(rt.state.state.log(DEV).unwrap().loaded);
    assert!(
        !has(&rt, "stage-11/loading"),
        "a 50 ms load never flashes the placeholder"
    );

    fake.set_latency(0.6);
    click(&mut rt, "channels-1-row-random");
    settle(&mut rt, 20);
    assert!(
        rt.elements()
            .into_iter()
            .any(|e| e.key.as_deref() == Some("loading")),
        "a slow one shows it after the delay"
    );
    settle(&mut rt, 60);
    assert!(rt.state.state.log(RANDOM).unwrap().loaded);
    assert!(!rt
        .elements()
        .into_iter()
        .any(|e| e.key.as_deref() == Some("loading")));
}

// ---- connection ------------------------------------------------------------------------------

#[test]
fn losing_the_connection_blocks_the_composer_and_reconnecting_refetches() {
    let (mut rt, fake) = signed_in(0.0);
    fake.disconnect(5);
    settle(&mut rt, 4);
    assert!(
        has(&rt, "composer/reason"),
        "the box says why it can't send"
    );
    let before = rt.state.backend.sent().len();
    fake.gateway(GatewayUpdate::Connected);
    settle(&mut rt, 10);
    assert!(!has(&rt, "composer/reason"));
    let refetch = rt.state.backend.sent()[before..].iter().any(|c| {
        matches!(
            c,
            Command::LoadMessages {
                channel: GENERAL,
                before: None
            }
        )
    });
    assert!(refetch, "events missed while away are fetched again");
}

fn ids_of(rt: &Runtime<App>, channel: u64) -> Vec<u64> {
    rt.state
        .state
        .log(channel)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.id)
        .collect()
}

fn assert_no_duplicates(ids: &[u64]) {
    let mut sorted = ids.to_vec();
    sorted.dedup();
    assert_eq!(sorted, ids, "ids are unique and in order");
}

/// The sequence the real gateway produces across an outage: every retry is announced with
/// `Connecting`, which must not make the app forget that it was away.
fn outage(rt: &mut Runtime<App>, fake: &FakeBackend) {
    fake.disconnect(5);
    settle(rt, 3);
    fake.failed_retry(5);
    settle(rt, 3);
    assert!(rt.state.state.ui.connection != ConnectionStatus::Connected);
}

#[test]
fn messages_missed_during_an_outage_show_after_the_real_reconnect_sequence() {
    let (mut rt, fake) = signed_in(0.0);
    rt.update(|a, _| a.act(|s| s.load_older(GENERAL)));
    settle(&mut rt, 4);
    let loaded = ids_of(&rt, GENERAL);
    assert_eq!(loaded.len(), 100, "two pages are loaded");

    outage(&mut rt, &fake);
    fake.missed(GENERAL, ANA, "posted while you were away");
    fake.missed(GENERAL, BRUNO, "and another one");
    settle(&mut rt, 3);
    assert_eq!(
        general_contents(&rt, "posted while you were away"),
        0,
        "nothing arrives while away"
    );

    let before = rt.state.backend.sent().len();
    fake.reconnect();
    settle(&mut rt, 10);
    let sent = &rt.state.backend.sent()[before..];
    assert!(
        sent.contains(&Command::LoadMessages {
            channel: GENERAL,
            before: None
        }),
        "the open channel is fetched again: {sent:?}"
    );
    assert!(
        sent.contains(&Command::Refresh),
        "the lists are fetched again"
    );
    assert_eq!(rt.state.state.ui.connection, ConnectionStatus::Connected);

    assert_eq!(general_contents(&rt, "posted while you were away"), 1);
    assert_eq!(general_contents(&rt, "and another one"), 1);
    let now = ids_of(&rt, GENERAL);
    assert_no_duplicates(&now);
    assert_eq!(
        now.len(),
        102,
        "the older page stays and only the two new ones are added"
    );
    assert!(now.starts_with(&loaded), "what was loaded is untouched");
    assert_eq!(rt.state.state.ui.place, Place::Guild(1));
    assert_eq!(
        rt.state.state.current_channel(),
        Some(GENERAL),
        "the selection stays"
    );
}

#[test]
fn another_channel_is_refetched_when_next_visited_and_a_clean_reconnect_refetches_nothing() {
    let (mut rt, fake) = signed_in(0.0);
    rt.update(|a, _| a.act(|s| s.select_channel(DEV)));
    settle(&mut rt, 4);
    rt.update(|a, _| a.act(|s| s.select_channel(GENERAL)));
    settle(&mut rt, 4);
    let dev = ids_of(&rt, DEV);

    outage(&mut rt, &fake);
    fake.missed(DEV, ANA, "dev news while away");
    fake.reconnect();
    settle(&mut rt, 10);
    assert_eq!(
        ids_of(&rt, DEV),
        dev,
        "a channel out of sight isn't fetched yet"
    );
    assert!(rt.state.state.log(DEV).unwrap().stale);

    rt.update(|a, _| a.act(|s| s.select_channel(DEV)));
    settle(&mut rt, 10);
    let now = ids_of(&rt, DEV);
    assert_eq!(now.len(), dev.len() + 1);
    assert_no_duplicates(&now);
    assert!(!rt.state.state.log(DEV).unwrap().stale);

    // Connecting then Connected with no Disconnected in between is not a gap.
    let before = rt.state.backend.sent().len();
    fake.reconnect();
    settle(&mut rt, 6);
    assert!(
        rt.state.backend.sent()[before..].is_empty(),
        "no gap, no refetch"
    );
}

#[test]
fn the_member_panel_width_springs_and_its_content_keeps_its_width() {
    let (mut rt, _) = signed_in(0.0);
    assert_eq!(rect_of(&rt, "members")[2], 0.0);
    click(&mut rt, "members-toggle");
    settle(&mut rt, 4);
    let mid = rect_of(&rt, "members")[2];
    assert!(
        mid > 0.0 && mid < crate::ui::members::WIDTH,
        "mid-flight width {mid}"
    );
    assert_eq!(
        rect_of(&rt, "members-inner")[2],
        crate::ui::members::WIDTH,
        "the content does not squash"
    );
    settle(&mut rt, 80);
    assert!((rect_of(&rt, "members")[2] - crate::ui::members::WIDTH).abs() < 0.5);
    click(&mut rt, "members-toggle");
    settle(&mut rt, 80);
    assert!(rect_of(&rt, "members")[2] < 0.5);
}

#[test]
fn signing_out_returns_to_auth() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "user-menu-button");
    settle(&mut rt, 8);
    click(&mut rt, "user-menu-item3");
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.screen, Screen::Auth);
    assert!(rt.state.state.session.is_none());
    assert!(
        rt.state.state.data.guilds.is_empty(),
        "nothing of the old session lingers"
    );
}

#[test]
fn the_guild_menu_shows_the_invite_code_and_copy_toasts() {
    let (mut rt, _) = signed_in(0.0);
    click(&mut rt, "guild-header");
    settle(&mut rt, 8);
    click(&mut rt, "guild-menu-item0");
    settle(&mut rt, 8);
    assert!(has(&rt, "invite-code"), "the invite dialog shows the code");
    assert!(rt
        .elements()
        .into_iter()
        .any(|e| e.text.as_deref() == Some("HIDEOUT1")));
    click(&mut rt, "invite-copy");
    settle(&mut rt, 6);
    assert_eq!(
        rt.state.state.ui.toasts.front().map(|t| t.message.as_str()),
        Some("Invite code copied")
    );
}

#[test]
fn the_server_url_field_expands_from_the_server_line() {
    let state = AppState::new(fake_now, Tz::Fixed(0));
    let mut rt = runtime(App::new(Box::new(FakeBackend::new()), state));
    assert!(!has(&rt, "auth-server"));
    click(&mut rt, "auth-server-toggle");
    settle(&mut rt, 4);
    assert!(has(&rt, "auth-server"));
    rt.update(|a, _| a.state.auth.server.replace_text("http://10.0.0.5:9000"));
    settle(&mut rt, 2);
    assert_eq!(rt.state.state.server_url(), "http://10.0.0.5:9000");
}

#[test]
fn headers_and_footers_line_up_across_sidebar_and_panel() {
    // Deliberate geometry: sidebar header == channel header == member header (HEADER_H) and user
    // strip == composer zone (FOOTER_H), so titles share a baseline and the scroll-edge lines of
    // both columns sit on one y.
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.state.ui.members_open = true);
    settle(&mut rt, 80);
    let banner = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "banner")
        .expect("the channel header")
        .rect;
    let side_header = rect_of(&rt, "guild-header");
    assert_eq!(side_header[1], banner[1], "header tops");
    assert_eq!(side_header[3], banner[3], "header heights");
    assert_eq!(banner[3], Metrics::default().header_h());
    let members = rect_of(&rt, "members-inner");
    assert_eq!(members[1], banner[1], "the member panel starts with the header");
    let (strip, zone) = (rect_of(&rt, "user-strip"), rect_of(&rt, "composer-area"));
    assert_eq!(strip[1], zone[1], "footer tops");
    assert_eq!(strip[3], zone[3]);
    assert_eq!(strip[3], Metrics::default().footer_h());
}

#[test]
fn no_permanent_divider_is_drawn_between_regions() {
    // Borders separate structure, not content: no header or footer carries a rule at rest.
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.state.ui.members_open = true);
    settle(&mut rt, 80);
    let panel = rect_of(&rt, "main-panel");
    let composer = rect_of(&rt, "composer-area");
    let banner = rt.elements().into_iter().find(|e| e.role == "banner").unwrap().rect;
    // The composer card floats with even margins inside its zone.
    let card = rect_of(&rt, "composer");
    assert_eq!(card[1] - composer[1], 12.0, "top margin");
    assert_eq!(composer[1] + composer[3] - (card[1] + card[3]), 12.0, "bottom margin");
    assert_eq!(card[0] - composer[0], 12.0, "left margin");
    assert!(banner[1] >= panel[1]);
}

/// The two scroll-edge targets of a region: `(top, bottom)`.
fn edges_of(rt: &Runtime<App>, which: fn(&crate::state::Edges) -> &thehideout_design::components::ScrollEdges) -> (f32, f32) {
    let e = which(&rt.state.state.ui.edges);
    (e.top(), e.bottom())
}

#[test]
fn the_transcript_shows_its_top_line_only_when_scrolled_and_its_bottom_line_only_when_not_at_the_end() {
    let (mut rt, _) = signed_in(0.0);
    settle(&mut rt, 20);
    // Opens at the bottom: messages sit under the header, nothing is below the viewport.
    assert_eq!(edges_of(&rt, |e| &e.transcript), (1.0, 0.0), "at the bottom");
    let (x, y) = rt.find("transcript-area").unwrap();
    rt.wheel(x + 100.0, y + 100.0, 600.0);
    settle(&mut rt, 10);
    let (top, bottom) = edges_of(&rt, |e| &e.transcript);
    assert_eq!((top, bottom), (1.0, 1.0), "scrolled up a little: both lines");
    // Back to the end: the bottom line goes again.
    let tx = rt.state.state.current_channel().unwrap();
    rt.update(move |_, tx_| tx_.scroll_to_bottom(format!("tx-{tx}")));
    settle(&mut rt, 80);
    assert_eq!(edges_of(&rt, |e| &e.transcript), (1.0, 0.0), "back at the bottom");
    // A short channel has nothing under either edge.
    rt.update(|a, _| a.act(|s| s.select_channel(RANDOM)));
    settle(&mut rt, 40);
    let scrolls = rt.state.state.current_channel() == Some(RANDOM);
    assert!(scrolls);
}

#[test]
fn the_edge_lines_sit_where_the_header_ends_and_the_zone_begins_in_both_columns() {
    let (mut rt, _) = signed_in(0.0);
    settle(&mut rt, 40);
    let banner = rt.elements().into_iter().find(|e| e.role == "banner").unwrap().rect;
    let header_end = banner[1] + banner[3];
    let area = rect_of(&rt, "transcript-area");
    assert_eq!(area[1], header_end, "the transcript starts under the header");
    let sidebar_body = rect_of(&rt, "sidebar-body");
    assert_eq!(sidebar_body[1], header_end, "so does the sidebar list: one y for both top lines");
    let zone = rect_of(&rt, "composer-area");
    let strip = rect_of(&rt, "user-strip");
    assert_eq!(area[1] + area[3], zone[1], "the transcript ends where the zone begins");
    assert_eq!(sidebar_body[1] + sidebar_body[3], strip[1], "so does the sidebar list");
    // Both lines are pinned to the edges of those regions.
    let top = rt.presented("edge-top").expect("a top line").0;
    assert_eq!(top[3], 1.0);
}

#[test]
fn scrolling_runs_no_view_and_changes_the_lines_by_live_values() {
    let (mut rt, _) = signed_in(0.0);
    settle(&mut rt, 20);
    let (x, y) = rt.find("transcript-area").unwrap();
    // Pin the pill so no state flips during the scroll: wheel a little, settle, then count.
    rt.wheel(x + 100.0, y + 100.0, 100.0);
    settle(&mut rt, 30);
    let before = crate::app::views();
    for _ in 0..6 {
        rt.wheel(x + 100.0, y + 100.0, 40.0);
        settle(&mut rt, 2);
    }
    // The lines are bound to live values: the only view runs allowed are the ones the at-bottom
    // transition itself asks for (none here: the reader is already away from the end).
    assert_eq!(crate::app::views(), before, "a scroll frame re-ran the view");
}

#[test]
fn the_sidebar_and_member_lists_report_their_edges_too() {
    let (mut rt, _) = signed_in(0.0);
    rt.update(|a, _| a.state.ui.members_open = true);
    settle(&mut rt, 80);
    // Fewer rows than the viewport: nothing is cut off at either edge.
    assert_eq!(edges_of(&rt, |e| &e.members), (0.0, 0.0), "the member list fits");
    assert_eq!(edges_of(&rt, |e| &e.sidebar), (0.0, 0.0), "the channel list fits");
    // A short window cuts the channel list off below.
    rt.set_viewport((1280.0, 300.0), 1.0);
    settle(&mut rt, 20);
    let (top, bottom) = edges_of(&rt, |e| &e.sidebar);
    assert_eq!((top, bottom), (0.0, 1.0), "list taller than the window: more below");
    let (x, y) = rt.find("channels-1-row-general").expect("a channel row");
    rt.wheel(x, y, -800.0);
    settle(&mut rt, 60);
    assert_eq!(edges_of(&rt, |e| &e.sidebar), (1.0, 0.0), "scrolled to its end");
}

#[test]
fn the_sidebar_narrows_and_the_member_panel_hides_in_small_windows() {
    use crate::ui::shell::Layout;
    assert_eq!(Layout::of(1280.0, 800.0), Layout { sidebar: 240.0, members: true, height: 800.0, width: 1280.0 });
    assert_eq!(Layout::of(1050.0, 800.0), Layout { sidebar: 208.0, members: true, height: 800.0, width: 1050.0 });
    assert_eq!(Layout::of(800.0, 600.0), Layout { sidebar: 208.0, members: false, height: 600.0, width: 800.0 });
}

// ---- appearance ------------------------------------------------------------------------------

mod appearance {
    use super::*;
    use crate::look::{Look, ReduceMotion};
    use rune::prefs::Prefs;
    use rune::theme::srgb_encode;
    use std::sync::{Arc, Mutex};
    use thehideout_design::{
        theme_for, AccentPreset, Appearance, BackgroundPreset, Contrast, Density, TextSize, ThemeSpec,
    };

    fn prefs_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("thehideout-look-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir.join("prefs.json")
    }

    fn signed_in_with(prefs: &std::path::Path) -> Runtime<App> {
        let (mut app, fake) = booted(true);
        let state = std::mem::replace(&mut app.state, AppState::new(fake_now, Tz::Fixed(0)));
        // The fake world is already signed in: start the app the way the window does, over it.
        let mut started = App::start(Box::new(fake), state, Arc::new(Mutex::new(Prefs::at(prefs))));
        started.state.ui.look.spec.appearance = Appearance::Dark;
        runtime(started)
    }

    /// `bg` of `spec` drawn `dark`, sRGB-encoded as `stage_fill` reports it.
    fn bg_of(spec: &ThemeSpec, dark: bool) -> [f32; 3] {
        let c = theme_for(spec, dark).bg.color();
        [srgb_encode(c.r), srgb_encode(c.g), srgb_encode(c.b)]
    }

    /// The colour the window's `stage` is being drawn with this frame (mid-flight while it crosses).
    fn clear(rt: &Runtime<App>) -> [f32; 3] {
        let c = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("stage"))
            .expect("the stage is drawn")
            .fill;
        [srgb_encode(c.r), srgb_encode(c.g), srgb_encode(c.b)]
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.004)
    }

    fn saved(path: &std::path::Path) -> Option<Look> {
        Prefs::at(path).get("appearance")
    }

    fn open_settings(rt: &mut Runtime<App>) {
        // The page is taller than the default window: every control stays on screen to be clicked.
        rt.set_viewport((1280.0, 1400.0), 1.0);
        settle(rt, 2);
        click(rt, "user-menu-button");
        settle(rt, 8);
        click(rt, "user-menu-item1");
        settle(rt, 6);
        assert!(has(rt, "settings"), "Settings… opens the settings page");
    }

    #[test]
    fn settings_open_from_the_gear_menu_and_close_with_escape_or_the_button() {
        let mut rt = signed_in(0.0).0;
        open_settings(&mut rt);
        assert!(has(&rt, "page-appearance") && !has(&rt, "shell"), "the page replaces the shell");
        key(&mut rt, Key::Escape);
        settle(&mut rt, 6);
        assert!(!has(&rt, "settings") && has(&rt, "shell"), "Esc leaves settings");
        open_settings(&mut rt);
        click(&mut rt, "settings-close");
        settle(&mut rt, 6);
        assert!(!has(&rt, "settings") && has(&rt, "shell"), "the close button leaves settings");
    }

    #[test]
    fn every_control_changes_the_look_at_once() {
        let mut rt = signed_in(0.0).0;
        open_settings(&mut rt);
        click(&mut rt, "accent-coral");
        click(&mut rt, "background-paper");
        click(&mut rt, "set-contrast/segment1");
        click(&mut rt, "set-appearance/segment1");
        click(&mut rt, "set-motion/segment1");
        click(&mut rt, "set-tint");
        settle(&mut rt, 4);
        let look = rt.state.state.ui.look;
        assert_eq!(look.spec.accent, AccentPreset::Coral);
        assert_eq!(look.spec.background, BackgroundPreset::Paper);
        assert_eq!(look.spec.contrast, Contrast::High);
        assert_eq!(look.spec.appearance, Appearance::Light);
        assert_eq!(look.reduce_motion, ReduceMotion::On);
        assert!(look.spec.tint);
        settle(&mut rt, 60);
        assert!(near(clear(&rt), bg_of(&look.spec, false)), "the window is drawn in the new look");
    }

    #[test]
    fn a_new_look_crosses_on_the_calm_spring_and_nothing_else_moves() {
        let mut rt = signed_in(0.0).0;
        open_settings(&mut rt);
        let before = clear(&rt);
        let panel = rect_of(&rt, "settings-panel");
        click(&mut rt, "background-cool");
        settle(&mut rt, 1);
        let target = bg_of(&rt.state.state.ui.look.spec, true);
        assert!(!near(before, target));
        settle(&mut rt, 5);
        let mid = clear(&rt);
        assert!(!near(mid, before) && !near(mid, target), "mid-crossing: {mid:?}");
        settle(&mut rt, 90);
        assert!(near(clear(&rt), target), "it lands on the new background");
        assert_eq!(rect_of(&rt, "settings-panel"), panel, "no layout moved");
    }

    #[test]
    fn reduce_motion_on_shortens_the_crossing_to_a_quick_fade() {
        let crossing = |reduce: ReduceMotion| {
            let mut rt = signed_in(0.0).0;
            rt.update(|a, _| a.state.ui.look.reduce_motion = reduce);
            open_settings(&mut rt);
            click(&mut rt, "background-cool");
            settle(&mut rt, 1);
            let target = bg_of(&rt.state.state.ui.look.spec, true);
            settle(&mut rt, 13);
            near(clear(&rt), target)
        };
        assert!(crossing(ReduceMotion::On), "On lands within the 0.15s fade");
        assert!(!crossing(ReduceMotion::Off), "Off keeps the calm spring, still crossing at 0.23s");
    }

    #[test]
    fn the_look_is_written_to_prefs_and_loads_back_beside_the_session() {
        let path = prefs_file("persist");
        {
            // The session lives in the same file: both writers share one store.
            let mut p = Prefs::at(&path);
            p.set("session", &serde_json::json!({"server_url": "http://x", "token": "t"})).unwrap();
        }
        let mut rt = signed_in_with(&path);
        open_settings(&mut rt);
        click(&mut rt, "accent-ocean");
        click(&mut rt, "background-deep");
        click(&mut rt, "set-tint");
        click(&mut rt, "set-contrast/segment1");
        click(&mut rt, "set-motion/segment2");
        settle(&mut rt, 4);
        let expected = rt.state.state.ui.look;
        drop(rt); // the saver finishes what it was handed
        let loaded = saved(&path).expect("the look was written under `appearance`");
        assert_eq!(loaded, expected);
        assert_eq!(loaded.spec.accent, AccentPreset::Ocean);
        assert_eq!(loaded.spec.background, BackgroundPreset::Deep);
        assert!(loaded.spec.tint && loaded.spec.contrast == Contrast::High);
        assert_eq!(loaded.reduce_motion, ReduceMotion::Off);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"token\""), "the session is still in the file: {raw}");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn density_and_text_size_are_controls_that_apply_at_once_and_are_saved() {
        let path = prefs_file("sizes");
        let mut rt = signed_in_with(&path);
        open_settings(&mut rt);
        click(&mut rt, "set-density/segment1");
        click(&mut rt, "set-text-size/segment3");
        settle(&mut rt, 4);
        let look = rt.state.state.ui.look;
        assert_eq!((look.spec.density, look.spec.text_size), (Density::Compact, TextSize::P125));
        drop(rt);
        let loaded = saved(&path).expect("written");
        assert_eq!(loaded, look);
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["appearance"]["density"], "compact");
        assert_eq!(raw["appearance"]["text_size"], "125");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_look_saved_before_density_and_text_size_still_loads() {
        let path = prefs_file("legacy");
        let old = serde_json::json!({
            "accent": "ocean", "background": "cool", "appearance": "light",
            "tint": true, "contrast": "high", "reduce_motion": "on"
        });
        Prefs::at(&path).set("appearance", &old).unwrap();
        let look = saved(&path).expect("an old look parses");
        assert_eq!(look.spec.accent, AccentPreset::Ocean);
        assert_eq!(look.reduce_motion, ReduceMotion::On);
        assert_eq!((look.spec.density, look.spec.text_size), (Density::Comfortable, TextSize::P100));
        let app = App::start(Box::new(FakeBackend::new()), AppState::new(fake_now, Tz::Fixed(0)), Arc::new(Mutex::new(Prefs::at(&path))));
        assert_eq!(app.state.ui.look, look, "the app starts on it");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn startup_draws_the_saved_look_on_the_first_frame() {
        let path = prefs_file("startup");
        let saved_look = Look {
            spec: ThemeSpec {
                accent: AccentPreset::Graphite,
                background: BackgroundPreset::Deep,
                appearance: Appearance::Dark,
                tint: false,
                contrast: Contrast::High,
                density: Density::Compact,
                text_size: TextSize::P110,
            },
            reduce_motion: ReduceMotion::System,
        };
        Prefs::at(&path).set("appearance", &saved_look).unwrap();

        // The window's path: start over the saved prefs, then the very first frame.
        let state = AppState::new(fake_now, Tz::Fixed(0));
        let app = App::start(
            Box::new(FakeBackend::new()),
            state,
            Arc::new(Mutex::new(Prefs::at(&path))),
        );
        assert_eq!(app.state.ui.look, saved_look, "read before any frame");
        let mut rt = Runtime::new(
            app,
            view,
            VIEWPORT,
            Env { system_dark: false, scale: 1.0 },
        );
        rt.frame(0.0);
        // The system says light; the saved look says dark, Deep: that is what the first frame shows.
        assert!(near(clear(&rt), bg_of(&saved_look.spec, true)), "first frame is the saved look");
        assert!(
            !near(clear(&rt), bg_of(&ThemeSpec::default(), false)),
            "not the default theme"
        );
        drop(rt);

        // No file, a file that doesn't parse, and a file from another version all give a look.
        let none = prefs_file("none");
        let app = App::start(
            Box::new(FakeBackend::new()),
            AppState::new(fake_now, Tz::Fixed(0)),
            Arc::new(Mutex::new(Prefs::at(&none))),
        );
        assert_eq!(app.state.ui.look, Look::default());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}

// ---- density and text size -------------------------------------------------------------------

mod sizing {
    use super::*;

    fn set(rt: &mut Runtime<App>, m: Metrics) {
        rt.update(move |a, _| {
            a.state.ui.look.spec.density = m.density;
            a.state.ui.look.spec.text_size = m.text;
        });
        settle(rt, 80);
    }

    fn banner(rt: &Runtime<App>) -> [f32; 4] {
        rt.elements().into_iter().find(|e| e.role == "banner").expect("the channel header").rect
    }

    #[test]
    fn headers_and_footers_line_up_for_every_density_and_text_size() {
        let (mut rt, _) = signed_in(0.0);
        rt.update(|a, _| a.state.ui.members_open = true);
        for m in Metrics::all() {
            set(&mut rt, m);
            let b = banner(&rt);
            let side = rect_of(&rt, "guild-header");
            assert_eq!((side[1], side[3]), (b[1], b[3]), "{m:?}: sidebar and channel headers");
            assert_eq!(b[3], m.header_h(), "{m:?}");
            assert_eq!(rect_of(&rt, "members-inner")[1], b[1], "{m:?}: member panel");
            let (strip, zone) = (rect_of(&rt, "user-strip"), rect_of(&rt, "composer-area"));
            assert_eq!((strip[1], strip[3]), (zone[1], zone[3]), "{m:?}: footers");
            assert_eq!(strip[3], m.footer_h(), "{m:?}");
            let card = rect_of(&rt, "composer");
            assert_eq!(card[1] - zone[1], m.zone_pad(), "{m:?}: top margin");
            assert_eq!(zone[1] + zone[3] - (card[1] + card[3]), m.zone_pad(), "{m:?}: bottom margin");
            let area = rect_of(&rt, "transcript-area");
            assert_eq!(area[1], b[1] + b[3], "{m:?}: the transcript starts under the header");
            assert_eq!(area[1] + area[3], zone[1], "{m:?}: and ends where the zone begins");
        }
    }

    #[test]
    fn switching_density_changes_the_row_height_and_text_size_the_text() {
        let (mut rt, _) = signed_in(0.0);
        settle(&mut rt, 20);
        let row = "channels-1-row-general";
        assert_eq!(rect_of(&rt, row)[3], 32.0);
        let first = rect_of(&rt, row)[1];
        let second = rect_of(&rt, "channels-1-row-dev")[1];
        assert_eq!(second - first, 34.0, "rows are 32 apart by 2");
        set(&mut rt, Metrics::new(Density::Compact, TextSize::P100));
        assert_eq!(rect_of(&rt, row)[3], 28.0, "compact rows");
        assert_eq!(rect_of(&rt, "channels-1-row-dev")[1] - rect_of(&rt, row)[1], 30.0);
        let side = rect_of(&rt, "guild-header");
        assert_eq!(side[3], 40.0);
        set(&mut rt, Metrics::new(Density::Comfortable, TextSize::P100));
        assert_eq!(rect_of(&rt, row)[3], 32.0, "and back");
        let title = |rt: &Runtime<App>| rt.elements().into_iter().find(|e| e.text.as_deref() == Some("general") && e.rect[0] > 280.0).expect("the title").rect;
        let small = title(&rt);
        set(&mut rt, Metrics::new(Density::Comfortable, TextSize::P125));
        let big = title(&rt);
        assert!(big[2] > small[2] * 1.2 && big[3] > small[3] * 1.2, "{small:?} -> {big:?}");
    }

    #[test]
    fn the_header_title_stays_inside_its_header_at_every_size_and_window() {
        for (w, h) in [(1280.0, 800.0), (1000.0, 700.0)] {
            let (mut rt, _) = signed_in(0.0);
            rt.set_viewport((w, h), 1.0);
            rt.update(|a, _| a.state.ui.members_open = true);
            for m in Metrics::all() {
                set(&mut rt, m);
                let b = banner(&rt);
                let title = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.text.as_deref() == Some("general") && e.rect[0] > 280.0)
                    .unwrap_or_else(|| panic!("{m:?}: no title"));
                let t = title.rect;
                assert!(t[1] >= b[1] - 0.01 && t[1] + t[3] <= b[1] + b[3] + 0.01, "{m:?} at {w}x{h}: title {t:?} vertically outside {b:?}");
                assert!(t[0] >= b[0] - 0.01 && t[0] + t[2] <= b[0] + b[2] + 0.01, "{m:?} at {w}x{h}: title {t:?} horizontally outside {b:?}");
                // Rows keep their text, too.
                let row = rect_of(&rt, "channels-1-row-general");
                let label = rt.elements().into_iter().find(|e| e.text.as_deref() == Some("general") && e.rect[0] < 280.0 && e.rect[1] >= row[1]).unwrap();
                assert!(label.rect[3] <= row[3] + 0.01, "{m:?}: the row's label {:?} in {row:?}", label.rect);
            }
        }
    }
}
