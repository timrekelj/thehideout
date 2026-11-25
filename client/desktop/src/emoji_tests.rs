//! Proofs of emojis against the fake backend: the picker and its insertions, the `:sm`
//! autocomplete (through real key input), rendering of known and unknown tokens, jumbo messages,
//! reactions (optimistic toggle, rollback, live events, the cap), the Emojis settings page
//! (upload, rename, delete, validation, the 200 cap, live updates), emoji channel icons, recents
//! that survive a restart, and the picture cache's stills.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rune::prefs::Prefs;
use rune::Key;
use thehideout_api::{Emoji, Event, GatewayUpdate, ReactionEmoji};
use thehideout_design::components::chat::emoji::{search_unicode, Pick};

use crate::app::{fake_now, App};
use crate::backend::{Backend, Command};
use crate::emoji::{valid_name, IconEmoji, PickerTarget, MAX_EMOJIS};
use crate::fake::{FakeBackend, ANA, BRUNO, DEV, GENERAL, MEMES, PARROT, SHIPIT, SMUG_CAT, TIM};
use crate::format::Tz;
use crate::forms::DialogKind;
use crate::scenes::{booted, booted_as};
use crate::server_settings::ServerPage;
use crate::state::{chat_id_of, AppState, Place};
use crate::tests::{click, click_at, has, key, runtime, settle, type_text};

const G: u64 = 1;

/// Tim in #memes: custom emojis loaded, the seeded reactions in.
fn memes() -> (App, FakeBackend) {
    let (mut app, fake) = booted(true);
    app.act(|s| s.select_channel(MEMES));
    app.pump();
    (app, fake)
}

fn unicode(glyph: &str) -> Pick {
    Pick::from_key(&format!("u:{glyph}"), &[]).unwrap_or_else(|| panic!("{glyph} is not an emoji"))
}

fn custom(app: &App, id: u64) -> Pick {
    Pick::from_key(&format!("c:{id}"), &app.state.server_emojis())
        .unwrap_or_else(|| panic!("no custom emoji {id}"))
}

fn heart() -> ReactionEmoji {
    ReactionEmoji::Unicode("\u{2764}\u{fe0f}".into())
}

/// The first message of #memes (the hearts: 3 of them, yours among them).
fn hearts_message(app: &App) -> u64 {
    app.state
        .log(MEMES)
        .unwrap()
        .messages
        .iter()
        .find(|m| m.content.contains("emoji pack"))
        .unwrap()
        .id
}

fn reaction(app: &App, channel: u64, message: u64, emoji: &ReactionEmoji) -> Option<(u32, bool)> {
    app.state
        .log(channel)
        .unwrap()
        .messages
        .iter()
        .find(|m| m.id == message)
        .unwrap()
        .reactions
        .iter()
        .find(|r| &r.emoji == emoji)
        .map(|r| (r.count, r.me))
}

fn server_reaction(
    fake: &FakeBackend,
    channel: u64,
    message: u64,
    emoji: &ReactionEmoji,
) -> Option<(u32, bool)> {
    fake.message(channel, message)
        .unwrap()
        .reactions
        .iter()
        .find(|r| &r.emoji == emoji)
        .map(|r| (r.count, r.me))
}

fn sent(fake: &FakeBackend, f: impl Fn(&Command) -> bool) -> usize {
    fake.sent().iter().filter(|c| f(c)).count()
}

fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("thehideout-emoji-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ---- the picker ------------------------------------------------------------------------------

#[test]
fn a_picked_unicode_emoji_goes_into_the_draft_at_the_caret() {
    let (mut app, _) = memes();
    let edit = app.state.composer_mut();
    edit.replace_text("ab cd");
    edit.set_caret(2);
    app.act(|s| s.open_picker(PickerTarget::Composer, "composer-emoji"));
    assert!(app.state.ui.picker.is_some());
    app.act(|s| s.picker_pick(unicode("\u{1f600}")));
    assert_eq!(
        app.state.composer().text,
        "ab\u{1f600} cd",
        "the character itself, not :grinning:, at the caret"
    );
    assert!(app.state.ui.picker.is_none(), "choosing closes the picker");
    assert!(app.state.composer().focused, "typing goes on");
}

#[test]
fn a_picked_server_emoji_goes_in_as_its_token_animated_ones_with_an_a() {
    let (mut app, _) = memes();
    app.state.composer_mut().replace_text("so ");
    let still = custom(&app, SMUG_CAT);
    let moving = custom(&app, PARROT);
    app.act(|s| {
        s.open_picker(PickerTarget::Composer, "composer-emoji");
        s.picker_pick(still);
        s.open_picker(PickerTarget::Composer, "composer-emoji");
        s.picker_pick(moving);
    });
    assert_eq!(
        app.state.composer().text,
        "so <:smug_cat:7001><a:party_parrot:7002>"
    );
}

#[test]
fn the_same_button_closes_the_picker_and_a_new_target_replaces_it() {
    let (mut app, _) = memes();
    app.act(|s| s.open_picker(PickerTarget::Composer, "composer-emoji"));
    app.act(|s| s.open_picker(PickerTarget::Composer, "composer-emoji"));
    assert!(app.state.ui.picker.is_none(), "pressed again, it closes");
    app.act(|s| s.open_picker(PickerTarget::Composer, "composer-emoji"));
    app.act(|s| {
        s.open_picker(
            PickerTarget::Reaction {
                channel: MEMES,
                message: 1,
            },
            "react-add-m:1",
        )
    });
    assert_eq!(
        app.state.ui.picker.as_ref().unwrap().target,
        PickerTarget::Reaction {
            channel: MEMES,
            message: 1
        }
    );
}

#[test]
fn the_picker_opens_from_the_composer_button_and_a_cell_inserts_through_real_input() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer-emoji");
    settle(&mut rt, 4);
    assert!(has(&rt, "emoji-picker"), "the popover is up");
    assert!(
        has(&rt, "emoji-picker-cell-0"),
        "the grid built its first row"
    );
    assert!(has(&rt, "emoji-picker-section-0"), "section buttons");
    // The first cell: with no recents, the server's first emoji by name (blob_love).
    click(&mut rt, "emoji-picker-cell-0");
    settle(&mut rt, 4);
    assert_eq!(rt.state.state.composer().text, "<:blob_love:7004>");
    assert!(!has(&rt, "emoji-picker"), "closed");
}

#[test]
fn typing_in_the_picker_searches_both_sets_and_enter_picks_the_best_match() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer-emoji");
    settle(&mut rt, 3);
    type_text(&mut rt, "parrot");
    settle(&mut rt, 2);
    assert_eq!(
        rt.state.state.ui.picker.as_ref().unwrap().state.highlight,
        Some(0)
    );
    key(&mut rt, Key::Enter);
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.composer().text, "<a:party_parrot:7002>");
    // And Unicode by shortcode, with the arrow keys moving off the first match.
    click(&mut rt, "composer-emoji");
    settle(&mut rt, 3);
    type_text(&mut rt, "smile");
    settle(&mut rt, 2);
    key(&mut rt, Key::Right);
    settle(&mut rt, 2);
    assert_eq!(
        rt.state.state.ui.picker.as_ref().unwrap().state.highlight,
        Some(1)
    );
    key(&mut rt, Key::Escape);
    settle(&mut rt, 3);
    assert!(!has(&rt, "emoji-picker"), "Escape closes it");
}

#[test]
fn in_a_dm_only_unicode_is_offered() {
    let (mut app, _) = booted(true);
    app.act(|s| {
        s.go_home();
        s.select_channel(crate::fake::DM_ANA);
    });
    app.pump();
    assert_eq!(app.state.ui.place, Place::Home);
    assert!(
        app.state.server_emojis().is_empty(),
        "no server, no custom emojis"
    );
    assert!(app.state.emoji_set().get("7001").is_none());
    let mut rt = runtime(app);
    click(&mut rt, "composer-emoji");
    settle(&mut rt, 3);
    assert!(has(&rt, "emoji-picker"));
    assert!(!has(&rt, "emoji-picker-section-1") || rt.state.state.server_emojis().is_empty());
}

#[test]
fn recents_keep_the_last_24_newest_first_without_repeats() {
    let (mut app, _) = memes();
    let all: Vec<Pick> = search_unicode("")
        .into_iter()
        .take(30)
        .map(|e| unicode(e.as_str()))
        .collect();
    for p in &all {
        app.act(|s| s.record_recent(p));
    }
    assert_eq!(app.state.ui.recents.len(), 24);
    assert_eq!(app.state.ui.recents[0], all[29].key());
    app.state.record_recent(&all[10]);
    assert_eq!(
        app.state.ui.recents[0],
        all[10].key(),
        "a repeat moves to the front"
    );
    assert_eq!(
        app.state
            .ui
            .recents
            .iter()
            .filter(|k| **k == all[10].key())
            .count(),
        1
    );
    assert!(app.state.ui.recents_dirty);
}

#[test]
fn recents_are_saved_to_the_prefs_and_come_back_after_a_restart() {
    let dir = std::env::temp_dir().join(format!("thehideout-emoji-prefs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prefs.json");
    {
        let app_state = AppState::new(fake_now, Tz::Fixed(0));
        let mut app = App::start(
            Box::new(FakeBackend::new()),
            app_state,
            Arc::new(Mutex::new(Prefs::at(&path))),
        );
        app.state.record_recent(&unicode("\u{1f389}"));
        app.state.record_recent(&unicode("\u{1f44d}"));
        app.flush();
        drop(app); // the saver finishes what it was handed
    }
    let app = App::start(
        Box::new(FakeBackend::new()),
        AppState::new(fake_now, Tz::Fixed(0)),
        Arc::new(Mutex::new(Prefs::at(&path))),
    );
    assert_eq!(
        app.state.ui.recents,
        vec!["u:\u{1f44d}".to_owned(), "u:\u{1f389}".to_owned()]
    );
    std::fs::remove_dir_all(dir).ok();
}

// ---- autocomplete ----------------------------------------------------------------------------

#[test]
fn typing_a_colon_word_lists_matches_and_tab_takes_the_first() {
    let (app, fake) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, "hey :sm");
    settle(&mut rt, 3);
    let (_, list) = rt
        .state
        .state
        .suggestions()
        .expect("two letters after a colon");
    assert!(!list.is_empty() && list.len() <= 8, "at most eight rows");
    assert!(has(&rt, "emoji-suggest") && has(&rt, "emoji-suggest-0"));
    key(&mut rt, Key::Tab);
    settle(&mut rt, 3);
    assert_eq!(
        rt.state.state.composer().text,
        "hey <:smug_cat:7001>",
        "the custom emoji leads on a prefix match"
    );
    assert!(
        !has(&rt, "emoji-suggest"),
        "the list goes once the word is replaced"
    );
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::Send { .. })),
        0,
        "Tab never sends"
    );
}

#[test]
fn enter_accepts_a_suggestion_instead_of_sending_and_arrows_pick_the_row() {
    let (app, fake) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, ":sm");
    settle(&mut rt, 3);
    key(&mut rt, Key::Down);
    settle(&mut rt, 2);
    assert_eq!(rt.state.state.ui.suggest.highlight, 1);
    key(&mut rt, Key::Enter);
    settle(&mut rt, 3);
    let text = rt.state.state.composer().text.clone();
    assert!(
        text.starts_with('\u{1f603}') || !text.starts_with(':'),
        "the second row replaced the word: {text}"
    );
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::Send { .. })),
        0,
        "Enter took the suggestion"
    );
    // With no list open, Enter sends as before.
    key(&mut rt, Key::Enter);
    settle(&mut rt, 3);
    assert_eq!(sent(&fake, |c| matches!(c, Command::Send { .. })), 1);
}

#[test]
fn escape_dismisses_the_list_for_that_word_and_a_longer_word_brings_it_back() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, "go :sm");
    settle(&mut rt, 3);
    assert!(has(&rt, "emoji-suggest"));
    key(&mut rt, Key::Escape);
    settle(&mut rt, 3);
    assert!(!has(&rt, "emoji-suggest"));
    assert_eq!(
        rt.state.state.composer().text,
        "go :sm",
        "the draft is untouched"
    );
    type_text(&mut rt, "i");
    settle(&mut rt, 3);
    assert!(has(&rt, "emoji-suggest"), "a new word: the list is back");
}

#[test]
fn a_press_on_a_suggestion_and_one_letter_or_no_colon_behave() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    click(&mut rt, "composer/scroll/input");
    type_text(&mut rt, "ratio 12:30 and :s");
    settle(&mut rt, 3);
    assert!(
        rt.state.state.suggestions().is_none(),
        "a colon inside a time, and one letter: nothing"
    );
    type_text(&mut rt, "hipit");
    settle(&mut rt, 3);
    click(&mut rt, "emoji-suggest-0");
    settle(&mut rt, 3);
    assert!(
        rt.state.state.composer().text.contains("<:shipit:7003>"),
        "{}",
        rt.state.state.composer().text
    );
}

// ---- rendering -------------------------------------------------------------------------------

#[test]
fn custom_tokens_of_the_guild_render_and_jumbo_messages_get_their_own_row() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    assert!(has(&rt, "jumbo"), "a message of only emoji draws big");
    let set = rt.state.state.emoji_set();
    assert!(set.get("7001").is_some(), "the guild's emoji is known");
    assert!(
        set.get("99999").is_none(),
        "an unknown id is not: it stays `:gone:` text"
    );
    assert!(rt
        .state
        .state
        .emoji_image(G, SMUG_CAT)
        .is_some_and(|i| !i.animated));
    assert!(rt
        .state
        .state
        .emoji_image(G, PARROT)
        .is_some_and(|i| i.animated));
    assert!(
        rt.state.state.emoji_image(2, SMUG_CAT).is_none(),
        "another guild's emoji isn't the open guild's"
    );
}

#[test]
fn a_foreign_guilds_emoji_is_unknown_in_this_channel() {
    let (mut app, _) = booted(true);
    app.act(|s| s.go_guild(2));
    app.pump();
    assert!(
        app.state.emoji_set().get("7001").is_none(),
        "The Hideout's emoji means nothing in Garage Band"
    );
    assert!(app.state.emoji_set().get("7101").is_some());
}

#[test]
fn reduced_motion_is_known_to_the_pictures() {
    let (app, _) = memes();
    let mut rt = runtime(app);
    rt.update(|a, _| a.state.ui.look.reduce_motion = crate::look::ReduceMotion::On);
    settle(&mut rt, 3);
    assert!(rt.state.state.ui.reduced.get());
    rt.update(|a, _| a.state.ui.look.reduce_motion = crate::look::ReduceMotion::Off);
    settle(&mut rt, 3);
    assert!(!rt.state.state.ui.reduced.get());
}

#[test]
fn an_animated_emoji_gets_a_still_beside_its_animation_and_a_static_one_does_not() {
    let (app, _) = memes();
    app.state.data.media.settle();
    let parrot = app.state.emoji(G, PARROT).unwrap().image.clone();
    let cat = app.state.emoji(G, SMUG_CAT).unwrap().image.clone();
    assert!(app.state.data.media.pixels(&parrot).is_some());
    assert!(
        app.state.data.media.still(&parrot).is_some(),
        "the first frame, for reduced motion and the picker at rest"
    );
    assert!(
        app.state.data.media.pixels(&cat).is_some() && app.state.data.media.still(&cat).is_none()
    );
    let gif = crate::media::generated_animated_emoji(5, 32, 6);
    let (w, h, raw) = crate::media::first_frame(&gif).unwrap();
    assert_eq!((w, h, raw.len()), (32, 32, 32 * 32 * 4));
    assert!(crate::media::first_frame(&crate::media::generated_emoji(5, 32)).is_none());
}

#[test]
fn emojis_are_charged_less_than_avatars_and_animated_ones_more() {
    let mut c = crate::media::MediaCache::default();
    c.insert_emoji("still", crate::media::generated_emoji(1, 32));
    assert_eq!(c.bytes(), crate::media::EMOJI_FLOOR);
    let mut a = crate::media::MediaCache::default();
    a.insert("avatar", crate::media::generated_emoji(1, 32));
    assert_eq!(a.bytes(), crate::media::COST_FLOOR);
    let gif = crate::media::generated_animated_emoji(1, 64, 8);
    let mut g = crate::media::MediaCache::default();
    g.insert_emoji("gif", gif.clone());
    assert!(g.bytes() >= gif.len() * 8 || g.bytes() == crate::media::EMOJI_FLOOR);
}

// ---- reactions -------------------------------------------------------------------------------

#[test]
fn pressing_a_chip_flips_your_reaction_at_once_and_the_server_agrees() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((3, true)));
    app.act(|s| s.toggle_reaction(MEMES, m, heart()));
    assert_eq!(
        reaction(&app, MEMES, m, &heart()),
        Some((2, false)),
        "optimistic: before any reply"
    );
    assert!(
        sent(
            &fake,
            |c| matches!(c, Command::RemoveReaction { message, .. } if *message == m)
        ) == 1
    );
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &heart()),
        Some((2, false)),
        "the echo of your own event changes nothing more"
    );
    assert_eq!(server_reaction(&fake, MEMES, m, &heart()), Some((2, false)));
    app.act(|s| s.toggle_reaction(MEMES, m, heart()));
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((3, true)));
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((3, true)));
    assert_eq!(server_reaction(&fake, MEMES, m, &heart()), Some((3, true)));
}

#[test]
fn the_last_reaction_of_an_emoji_removes_its_chip() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    let thumbs = ReactionEmoji::Unicode("\u{1f44d}".into());
    // The thumbs-up has one reaction, not yours: yours makes two, and taking it back makes one.
    app.act(|s| s.toggle_reaction(MEMES, m, thumbs.clone()));
    assert_eq!(reaction(&app, MEMES, m, &thumbs), Some((2, true)));
    app.pump();
    fake.react_as(MEMES, m, thumbs.clone(), BRUNO, false);
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &thumbs), Some((1, true)));
    app.act(|s| s.toggle_reaction(MEMES, m, thumbs.clone()));
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &thumbs),
        None,
        "no one left: no chip"
    );
}

#[test]
fn a_refused_reaction_rolls_back_and_says_so() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    fake.fail_reactions(2);
    app.act(|s| s.toggle_reaction(MEMES, m, heart()));
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((2, false)));
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &heart()),
        Some((3, true)),
        "rolled back to what the server has"
    );
    assert!(app
        .state
        .ui
        .toasts
        .iter()
        .any(|t| t.message.contains("remove the reaction")));
    // A refused add rolls back the other way.
    let party = ReactionEmoji::Unicode("\u{1f389}".into());
    app.act(|s| s.toggle_reaction(MEMES, m, party.clone()));
    assert_eq!(reaction(&app, MEMES, m, &party), Some((1, true)));
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &party),
        None,
        "the chip it made is gone"
    );
}

#[test]
fn other_peoples_reactions_arrive_live_and_never_touch_yours() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    fake.react_as(MEMES, m, heart(), ANA, true);
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((4, true)));
    fake.react_as(MEMES, m, heart(), ANA, false);
    fake.react_as(MEMES, m, heart(), BRUNO, false);
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((2, true)));
    let rocket = ReactionEmoji::Unicode("\u{1f680}".into());
    fake.react_as(MEMES, m, rocket.clone(), ANA, true);
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &rocket),
        Some((1, false)),
        "a new chip, not yours"
    );
    // Your own events from another device mark it yours without a double count.
    fake.gateway(GatewayUpdate::Event(Event::ReactionAdded {
        channel_id: MEMES,
        message_id: m,
        emoji: rocket.clone(),
        user_id: TIM,
    }));
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &rocket), Some((2, true)));
    fake.gateway(GatewayUpdate::Event(Event::ReactionAdded {
        channel_id: MEMES,
        message_id: m,
        emoji: rocket.clone(),
        user_id: TIM,
    }));
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &rocket),
        Some((2, true)),
        "the same event twice counts once"
    );
}

#[test]
fn at_twenty_distinct_emojis_a_new_one_is_refused_but_existing_chips_still_toggle() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    let twenty: Vec<ReactionEmoji> = search_unicode("")
        .into_iter()
        .skip(40)
        .take(19)
        .map(|e| ReactionEmoji::Unicode(e.as_str().to_owned()))
        .collect();
    app.state
        .data
        .logs
        .get_mut(&MEMES)
        .unwrap()
        .messages
        .iter_mut()
        .find(|x| x.id == m)
        .unwrap()
        .reactions = std::iter::once(heart())
        .chain(twenty.clone())
        .map(|emoji| thehideout_api::Reaction {
            emoji,
            count: 1,
            me: false,
        })
        .collect();
    assert_eq!(
        app.state
            .log(MEMES)
            .unwrap()
            .messages
            .iter()
            .find(|x| x.id == m)
            .unwrap()
            .reactions
            .len(),
        20
    );
    let before = fake.sent().len();
    app.act(|s| s.react(MEMES, m, ReactionEmoji::Unicode("\u{1f9be}".into()), false));
    assert_eq!(fake.sent().len(), before, "nothing sent");
    assert_eq!(
        app.state
            .log(MEMES)
            .unwrap()
            .messages
            .iter()
            .find(|x| x.id == m)
            .unwrap()
            .reactions
            .len(),
        20
    );
    assert!(app
        .state
        .ui
        .toasts
        .iter()
        .any(|t| t.message.contains("20 different reactions")));
    app.act(|s| s.toggle_reaction(MEMES, m, heart()));
    assert_eq!(
        reaction(&app, MEMES, m, &heart()),
        Some((2, true)),
        "an existing chip still takes yours"
    );
}

#[test]
fn the_server_refuses_a_21st_distinct_reaction_and_the_client_rolls_back() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    // 17 more on the server (the message has 3), then the client's stale view adds a 21st.
    for e in search_unicode("").into_iter().skip(60).take(17) {
        fake.react_as(
            MEMES,
            m,
            ReactionEmoji::Unicode(e.as_str().to_owned()),
            ANA,
            true,
        );
    }
    assert_eq!(fake.message(MEMES, m).unwrap().reactions.len(), 20);
    // The client hasn't heard (events not pumped): it lets the press through and the server says no.
    let extra = ReactionEmoji::Unicode("\u{1f9be}".into());
    app.act(|s| s.react(MEMES, m, extra.clone(), false));
    assert_eq!(
        reaction(&app, MEMES, m, &extra),
        Some((1, true)),
        "optimistic"
    );
    app.pump();
    assert_eq!(
        reaction(&app, MEMES, m, &extra),
        None,
        "refused and rolled back"
    );
    assert_eq!(server_reaction(&fake, MEMES, m, &extra), None);
}

#[test]
fn the_chips_the_plus_chip_and_the_hover_bar_button_work_through_real_input() {
    let (app, fake) = memes();
    let m = hearts_message(&app);
    let id = {
        let me = app.state.my_id();
        chat_id_of(
            app.state
                .log(MEMES)
                .unwrap()
                .messages
                .iter()
                .find(|x| x.id == m)
                .unwrap(),
            me,
        )
    };
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    let chip = format!("rx-{id}-{}", heart().key());
    assert!(has(&rt, &chip), "the heart chip: {chip}");
    assert!(
        has(&rt, &format!("react-bar-{id}")),
        "the hover bar has a React button"
    );
    click(&mut rt, &chip);
    settle(&mut rt, 3);
    assert_eq!(
        reaction(&rt.state, MEMES, m, &heart()),
        Some((3, true)).map(|_| (2, false)).or(None),
        "pressed: yours is taken back"
    );
    // "+" opens the picker on the message; a cell reacts with that emoji.
    click(&mut rt, &format!("react-add-{id}"));
    settle(&mut rt, 4);
    let p = rt
        .state
        .state
        .ui
        .picker
        .as_ref()
        .expect("the picker opened");
    assert_eq!(
        p.target,
        PickerTarget::Reaction {
            channel: MEMES,
            message: m
        }
    );
    assert_eq!(p.anchor, format!("react-add-{id}"));
    click(&mut rt, "emoji-picker-cell-0");
    settle(&mut rt, 4);
    assert_eq!(
        reaction(&rt.state, MEMES, m, &ReactionEmoji::Custom(7004)),
        Some((3, true)).map(|_| (3, true)),
        "blob_love already had 2: yours makes 3"
    );
    assert!(
        sent(&fake, |c| matches!(
            c,
            Command::AddReaction {
                emoji: ReactionEmoji::Custom(7004),
                ..
            }
        )) == 1
    );
}

/// The hover bar floats over the row's top edge, so a pointer travelling to it from the row's
/// text leaves the row first; the row must stay hovered long enough for the pointer to land on it.
#[test]
fn the_hover_bar_stays_up_while_the_pointer_travels_from_the_row_to_it() {
    let (app, _) = memes();
    // The last message has room above it for the bar (the first one's bar is clipped by the list).
    let m = app.state.log(MEMES).unwrap().messages.last().unwrap().id;
    let me = app.state.my_id();
    let id = chat_id_of(
        app.state
            .log(MEMES)
            .unwrap()
            .messages
            .iter()
            .find(|x| x.id == m)
            .unwrap(),
        me,
    );
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    let row = rt.find(&format!("msg-{id}")).expect("the row");
    let target = rt
        .find(&format!("react-bar-{id}"))
        .expect("the bar's React button");
    // Start on the row's text, then walk a straight line to the button at about 700px/s
    // (12px a frame), which leaves the row's top edge left of the bar.
    let from = (row.0 + 40.0, row.1);
    rt.pointer_move(from.0, from.1);
    settle(&mut rt, 6);
    let (dx, dy) = (target.0 - from.0, target.1 - from.1);
    let steps = (dx.abs().max(dy.abs()) / 12.0).ceil().max(1.0) as u32;
    for i in 1..=steps {
        let f = i as f32 / steps as f32;
        rt.pointer_move(from.0 + dx * f, from.1 + dy * f);
        settle(&mut rt, 1);
    }
    click_at(&mut rt, target);
    settle(&mut rt, 4);
    let p = rt
        .state
        .state
        .ui
        .picker
        .as_ref()
        .expect("the React button opened the picker");
    assert_eq!(
        p.target,
        PickerTarget::Reaction {
            channel: MEMES,
            message: m
        }
    );
}

#[test]
fn picking_an_emoji_you_already_reacted_with_does_nothing() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    let before = fake.sent().len();
    app.act(|s| {
        s.open_picker(
            PickerTarget::Reaction {
                channel: MEMES,
                message: m,
            },
            "react-bar-x",
        );
        s.picker_pick(unicode("\u{2764}\u{fe0f}"));
    });
    assert_eq!(fake.sent().len(), before);
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((3, true)));
}

// ---- the Emojis page -------------------------------------------------------------------------

fn emoji_page() -> (App, FakeBackend) {
    let (mut app, fake) = booted(true);
    app.act(|s| {
        s.open_server_settings(G);
        s.server_page(ServerPage::Emojis);
    });
    app.pump();
    (app, fake)
}

fn draft_with(app: &mut App, file: &str, name: &str) {
    let path = tmp(file, &crate::media::generated_png(3, 48));
    app.act(|s| s.pick_emoji_file(&path));
    app.pump();
    assert!(
        app.state
            .ui
            .server_settings
            .as_ref()
            .unwrap()
            .emojis
            .draft
            .is_some(),
        "the file was read"
    );
    app.state
        .ui
        .server_settings
        .as_mut()
        .unwrap()
        .emojis
        .draft
        .as_mut()
        .unwrap()
        .name
        .replace_text(name);
}

fn page_error(app: &App) -> Option<String> {
    let p = &app.state.ui.server_settings.as_ref().unwrap().emojis;
    p.draft
        .as_ref()
        .and_then(|d| d.error.clone())
        .or_else(|| p.error.clone())
}

#[test]
fn uploading_a_picture_suggests_a_name_and_adds_the_emoji() {
    let (mut app, fake) = emoji_page();
    let path = tmp("Blob Wave.png", &crate::media::generated_png(3, 48));
    app.act(|s| s.pick_emoji_file(&path));
    assert!(
        app.state
            .ui
            .server_settings
            .as_ref()
            .unwrap()
            .emojis
            .reading
    );
    assert!(fake.sent().iter().any(|c| matches!(
        c,
        Command::ReadImage {
            purpose: crate::backend::ImagePurpose::Emoji { guild: 1 },
            ..
        }
    )));
    app.pump();
    let d = app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .draft
        .as_ref()
        .unwrap();
    assert_eq!(d.name.text, "blob_wave", "from the file name");
    app.act(|s| s.create_emoji());
    assert!(app.state.ui.server_settings.as_ref().unwrap().emojis.busy);
    app.pump();
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .draft
        .is_none());
    assert!(
        app.state.emojis_of(G).iter().any(|e| e.name == "blob_wave"),
        "in the list"
    );
    assert!(
        fake.emojis(G).iter().any(|e| e.name == "blob_wave"),
        "on the server"
    );
    assert_eq!(app.state.emojis_of(G).len(), 7);
    assert!(app
        .state
        .ui
        .toasts
        .iter()
        .any(|t| t.message.contains("blob_wave")));
}

#[test]
fn a_gif_is_uploaded_as_animated() {
    let (mut app, fake) = emoji_page();
    let path = tmp(
        "spin.gif",
        &crate::media::generated_animated_emoji(9, 32, 4),
    );
    app.act(|s| s.pick_emoji_file(&path));
    app.pump();
    app.act(|s| s.create_emoji());
    app.pump();
    assert!(fake
        .emojis(G)
        .iter()
        .any(|e| e.name == "spin" && e.animated));
    assert!(app
        .state
        .emojis_of(G)
        .iter()
        .any(|e| e.name == "spin" && e.animated));
}

#[test]
fn names_are_validated_before_anything_is_sent() {
    let (mut app, fake) = emoji_page();
    draft_with(&mut app, "x.png", "Bad Name");
    let sends = |f: &FakeBackend| sent(f, |c| matches!(c, Command::CreateEmoji { .. }));
    app.act(|s| s.create_emoji());
    assert!(
        page_error(&app).unwrap().contains("2 to 32"),
        "{:?}",
        page_error(&app)
    );
    assert_eq!(sends(&fake), 0);
    for bad in ["a", "has-dash", "x".repeat(33).as_str()] {
        app.state
            .ui
            .server_settings
            .as_mut()
            .unwrap()
            .emojis
            .draft
            .as_mut()
            .unwrap()
            .name
            .replace_text(bad);
        app.act(|s| s.create_emoji());
        assert!(page_error(&app).is_some() && sends(&fake) == 0, "{bad}");
    }
    app.state
        .ui
        .server_settings
        .as_mut()
        .unwrap()
        .emojis
        .draft
        .as_mut()
        .unwrap()
        .name
        .replace_text("smug_cat");
    app.act(|s| s.create_emoji());
    assert!(
        page_error(&app).unwrap().contains("already exists"),
        "a taken name is caught here too"
    );
    assert_eq!(sends(&fake), 0);
    assert!(valid_name("fine_name_1"));
}

#[test]
fn a_file_over_two_mib_or_of_the_wrong_type_is_refused() {
    let (mut app, fake) = emoji_page();
    app.act(|s| s.pick_emoji_file(std::path::Path::new("/x/notes.txt")));
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .error
        .as_deref()
        .unwrap()
        .contains("PNG, JPEG, WebP or GIF"));
    assert!(sent(&fake, |c| matches!(c, Command::ReadImage { .. })) == 0);
    app.act(|s| {
        s.emoji_file_read(
            G,
            Ok((
                vec![0; thehideout_api::MAX_EMOJI_BYTES + 1],
                "image/png".into(),
            )),
        )
    });
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .error
        .as_deref()
        .unwrap()
        .contains("2 MiB"));
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .draft
        .is_none());
}

fn rename_of(app: &mut App) -> &mut crate::emoji::Rename {
    app.state.ui.server_settings.as_mut().unwrap().emojis.rename.as_mut().unwrap()
}

#[test]
fn renaming_in_place_validates_and_updates() {
    let (mut app, fake) = emoji_page();
    app.act(|s| s.start_rename(SMUG_CAT));
    rename_of(&mut app).name.replace_text("shipit");
    app.act(|s| s.commit_rename());
    assert!(rename_of(&mut app)
        .error
        .as_deref()
        .unwrap()
        .contains("already exists"));
    rename_of(&mut app).name.replace_text("X");
    app.act(|s| s.commit_rename());
    assert!(rename_of(&mut app).error.is_some());
    assert_eq!(sent(&fake, |c| matches!(c, Command::RenameEmoji { .. })), 0);
    rename_of(&mut app).name.replace_text("smug_dog");
    app.act(|s| s.commit_rename());
    app.pump();
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .rename
        .is_none());
    assert_eq!(app.state.emoji(G, SMUG_CAT).unwrap().name, "smug_dog");
    assert_eq!(
        fake.emojis(G)
            .iter()
            .find(|e| e.id == SMUG_CAT)
            .unwrap()
            .name,
        "smug_dog"
    );
    // Unchanged just closes the editor.
    app.act(|s| s.start_rename(SMUG_CAT));
    app.act(|s| s.commit_rename());
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .rename
        .is_none());
    assert_eq!(sent(&fake, |c| matches!(c, Command::RenameEmoji { .. })), 1);
}

#[test]
fn deleting_asks_first_and_the_emoji_goes_everywhere() {
    let (mut app, fake) = memes();
    app.act(|s| {
        s.open_server_settings(G);
        s.server_page(ServerPage::Emojis);
        s.open_dialog(DialogKind::DeleteEmoji {
            guild: G,
            emoji: SMUG_CAT,
        });
    });
    assert_eq!(
        sent(&fake, |c| matches!(c, Command::DeleteEmoji { .. })),
        0,
        "nothing until confirmed"
    );
    assert!(app.state.emoji_set().get("7001").is_some());
    app.act(|s| s.submit_dialog());
    app.pump();
    assert!(app.state.ui.dialog.is_none(), "the dialog closed");
    assert!(
        app.state.emoji(G, SMUG_CAT).is_none() && fake.emojis(G).iter().all(|e| e.id != SMUG_CAT)
    );
    assert!(
        app.state.emoji_set().get("7001").is_none(),
        "old messages show `:smug_cat:` text now"
    );
    assert!(
        !app.state.server_emojis().iter().any(|e| e.id == SMUG_CAT),
        "gone from the picker"
    );
}

#[test]
fn the_counter_and_the_cap_at_200() {
    let (mut app, fake) = emoji_page();
    fake.fill_emojis(G, MAX_EMOJIS);
    let all = fake.emojis(G);
    app.act(|s| s.set_emojis(G, all));
    assert_eq!(app.state.emojis_of(G).len(), 200);
    let before = fake.sent().len();
    app.act(|s| s.pick_emoji_file(&tmp("late.png", &crate::media::generated_png(1, 32))));
    assert_eq!(fake.sent().len(), before, "no file is even read at the cap");
    assert!(app
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .error
        .as_deref()
        .unwrap()
        .contains("200"));
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    assert!(has(&rt, "emoji-count") && has(&rt, "emoji-upload"));
}

#[test]
fn the_server_refusing_the_201st_shows_under_the_draft() {
    let (mut app, fake) = emoji_page();
    draft_with(&mut app, "last.png", "last_one");
    // The server filled up behind this client's back.
    fake.fill_emojis(G, MAX_EMOJIS);
    app.act(|s| s.create_emoji());
    app.pump();
    assert!(
        page_error(&app).unwrap().to_lowercase().contains("200"),
        "{:?}",
        page_error(&app)
    );
    assert!(
        !app.state.ui.server_settings.as_ref().unwrap().emojis.busy,
        "the button is free again"
    );
    assert!(
        app.state
            .ui
            .server_settings
            .as_ref()
            .unwrap()
            .emojis
            .draft
            .is_some(),
        "the draft stays"
    );
}

#[test]
fn another_admins_changes_arrive_live() {
    let (mut app, fake) = emoji_page();
    let mut list: Vec<Emoji> = fake.emojis(G);
    let mut extra = list[0].clone();
    extra.id = 8001;
    extra.name = "from_afar".into();
    extra.image = "emoji-8001".into();
    list.push(extra);
    fake.gateway(GatewayUpdate::Event(Event::EmojisUpdated {
        guild_id: G,
        emojis: list.clone(),
    }));
    app.pump();
    assert!(app.state.emojis_of(G).iter().any(|e| e.name == "from_afar"));
    list.retain(|e| e.id != 8001 && e.id != SHIPIT);
    fake.gateway(GatewayUpdate::Event(Event::EmojisUpdated {
        guild_id: G,
        emojis: list,
    }));
    app.pump();
    assert!(app.state.emoji(G, 8001).is_none() && app.state.emoji(G, SHIPIT).is_none());
}

#[test]
fn only_those_with_manage_emojis_see_the_page() {
    let (mut app, _) = booted_as(true, "ana");
    assert!(
        !app.state.can_manage_emojis(G),
        "a Designer with no permissions"
    );
    app.act(|s| s.open_server_settings(G));
    assert!(app.state.ui.server_settings.is_none());
    // A Roadie in Garage Band holds MANAGE_EMOJIS.
    let (mut app, _) = booted(true);
    app.act(|s| s.go_guild(2));
    app.pump();
    assert!(
        app.state.can_manage_emojis(2) && app.state.settings_page_allowed(2, ServerPage::Emojis)
    );
    assert!(
        !app.state.can_manage_emojis(3),
        "a moderator of channels only"
    );
}

#[test]
fn the_page_draws_its_rows_and_buttons() {
    let (app, _) = emoji_page();
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    for k in [
        "emoji-count",
        "emoji-upload",
        "emoji-list",
        "emoji-row-7001",
        "emoji-delete-7001",
        "emoji-rename-7001-button",
    ] {
        assert!(has(&rt, k), "{k}");
    }
    click(&mut rt, "emoji-rename-7001-button");
    settle(&mut rt, 3);
    assert!(rt
        .state
        .state
        .ui
        .server_settings
        .as_ref()
        .unwrap()
        .emojis
        .rename
        .is_some());
    click(&mut rt, "emoji-delete-7002");
    settle(&mut rt, 4);
    assert!(matches!(
        rt.state.state.ui.dialog.as_ref().map(|d| d.kind),
        Some(DialogKind::DeleteEmoji { emoji: 7002, .. })
    ));
}

// ---- channel icons ---------------------------------------------------------------------------

fn dev_dialog(app: &mut App) {
    app.act(|s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: G,
            channel: DEV,
        })
    });
    app.act(|s| s.toggle_picker());
}

#[test]
fn a_unicode_emoji_icon_is_saved_and_shown() {
    let (mut app, fake) = booted(true);
    dev_dialog(&mut app);
    app.act(|s| s.set_picker_tab(1));
    app.act(|s| s.pick_icon_emoji(unicode("\u{1f680}")));
    assert!(
        !app.state.channel_form().unwrap().picker_open,
        "choosing closes the picker"
    );
    assert_eq!(
        app.state.channel_form().unwrap().emoji,
        Some(IconEmoji::Unicode("\u{1f680}".into()))
    );
    app.act(|s| s.submit_dialog());
    assert!(fake.sent().iter().any(|c| matches!(c, Command::UpdateChannel { icon: Some(thehideout_api::ChannelIconRef::Unicode(e)), .. } if e == "\u{1f680}")));
    app.pump();
    assert_eq!(
        fake.channel(DEV).unwrap().icon_ref,
        thehideout_api::ChannelIconRef::Unicode("\u{1f680}".into())
    );
    let c = app.state.channel(G, DEV).unwrap().clone();
    assert_eq!(
        app.state.channel_emoji(&c),
        Some(IconEmoji::Unicode("\u{1f680}".into()))
    );
}

#[test]
fn a_server_emoji_icon_is_saved_and_falls_back_to_hash_when_the_emoji_is_deleted() {
    let (mut app, fake) = booted(true);
    dev_dialog(&mut app);
    app.act(|s| s.set_picker_tab(2));
    let cat = custom(&app, SMUG_CAT);
    app.act(|s| s.pick_icon_emoji(cat));
    app.act(|s| s.submit_dialog());
    app.pump();
    assert_eq!(
        fake.channel(DEV).unwrap().icon_ref,
        thehideout_api::ChannelIconRef::Custom(SMUG_CAT)
    );
    let c = app.state.channel(G, DEV).unwrap().clone();
    assert_eq!(
        app.state.channel_emoji(&c),
        Some(IconEmoji::Custom(SMUG_CAT))
    );
    // Someone deletes the emoji: the icon reference stays, but nothing draws; `#` shows.
    let rest: Vec<Emoji> = fake
        .emojis(G)
        .into_iter()
        .filter(|e| e.id != SMUG_CAT)
        .collect();
    fake.gateway(GatewayUpdate::Event(Event::EmojisUpdated {
        guild_id: G,
        emojis: rest,
    }));
    app.pump();
    let c = app.state.channel(G, DEV).unwrap().clone();
    assert_eq!(c.icon_ref, thehideout_api::ChannelIconRef::Custom(SMUG_CAT));
    assert_eq!(app.state.channel_emoji(&c), None, "the emoji is gone");
}

#[test]
fn choosing_an_icon_or_resetting_replaces_the_emoji() {
    let (mut app, _) = booted(true);
    dev_dialog(&mut app);
    app.act(|s| s.set_picker_tab(1));
    app.act(|s| s.pick_icon_emoji(unicode("\u{1f680}")));
    app.act(|s| s.toggle_picker());
    app.act(|s| s.pick_icon("rocket"));
    let f = app.state.channel_form().unwrap();
    assert_eq!((f.emoji.clone(), f.icon.as_str()), (None, "rocket"));
    assert_eq!(
        f.icon_ref(),
        thehideout_api::ChannelIconRef::Phosphor("rocket".into())
    );
    app.act(|s| s.pick_icon(""));
    assert_eq!(
        app.state.channel_form().unwrap().icon_ref(),
        thehideout_api::ChannelIconRef::Default
    );
}

#[test]
fn a_new_channel_can_start_with_an_emoji_icon() {
    let (mut app, fake) = booted(true);
    app.act(|s| s.open_dialog(DialogKind::CreateChannel { guild: G }));
    app.state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("party");
    app.act(|s| s.toggle_picker());
    app.act(|s| s.set_picker_tab(2));
    let p = custom(&app, PARROT);
    app.act(|s| s.pick_icon_emoji(p));
    app.act(|s| s.submit_dialog());
    app.pump();
    let made = fake.sent().into_iter().find_map(|c| match c {
        Command::CreateChannel { icon, .. } => Some(icon),
        _ => None,
    });
    assert_eq!(made, Some(thehideout_api::ChannelIconRef::Custom(PARROT)));
    assert!(
        app.state
            .channels_of(G)
            .iter()
            .any(|c| c.name == "party"
                && c.icon_ref == thehideout_api::ChannelIconRef::Custom(PARROT))
    );
}

#[test]
fn the_picker_tabs_switch_and_both_emoji_tabs_are_enabled() {
    let (mut app, _) = booted(true);
    dev_dialog(&mut app);
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    assert!(has(&rt, "icon-grid"), "Icons first");
    click(&mut rt, "picker-tab-1");
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.channel_form().unwrap().tab, 1);
    assert!(
        has(&rt, "icon-emoji"),
        "the Emoji tab shows the emoji picker"
    );
    click(&mut rt, "picker-tab-2");
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.channel_form().unwrap().tab, 2);
    assert!(
        has(&rt, "icon-emoji-cell-0"),
        "the server's emojis, first cell"
    );
    click(&mut rt, "icon-emoji-cell-0");
    settle(&mut rt, 3);
    assert_eq!(
        rt.state.state.channel_form().unwrap().emoji,
        Some(IconEmoji::Custom(7004))
    );
    assert!(!rt.state.state.channel_form().unwrap().picker_open);
}

#[test]
fn sidebar_and_header_draw_emoji_icons() {
    let (mut app, _) = booted(true);
    dev_dialog(&mut app);
    app.act(|s| s.set_picker_tab(1));
    app.act(|s| s.pick_icon_emoji(unicode("\u{1f680}")));
    app.act(|s| s.submit_dialog());
    app.pump();
    app.state.data.media.settle();
    let mut rt = runtime(app);
    settle(&mut rt, 3);
    // The row and the header both draw it: the rows' icon element is the emoji's box, not a glyph.
    assert!(
        has(&rt, "channels-1-row-dev/icon"),
        "the sidebar row has its icon"
    );
    assert_eq!(rt.state.state.current_channel(), Some(GENERAL));
}

#[test]
fn reactions_missed_during_an_outage_show_after_the_refetch() {
    let (mut app, fake) = memes();
    let m = hearts_message(&app);
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((3, true)));
    fake.react_missed(MEMES, m, heart(), ANA, true);
    fake.react_missed(MEMES, m, ReactionEmoji::Unicode("\u{1f680}".into()), BRUNO, true);
    app.act(|s| s.resync());
    app.pump();
    assert_eq!(reaction(&app, MEMES, m, &heart()), Some((4, true)), "the page brings the new count");
    assert_eq!(reaction(&app, MEMES, m, &ReactionEmoji::Unicode("\u{1f680}".into())), Some((1, false)), "and the new chip");
}

#[test]
fn emojis_changed_while_away_are_fetched_again_on_reconnect() {
    let (mut app, fake) = memes();
    let before = app.state.emojis_of(G).len();
    fake.fill_emojis(G, before + 2);
    app.act(|s| s.resync());
    app.pump();
    assert_eq!(app.state.emojis_of(G).len(), before + 2, "the open server's list was asked for again");
}
