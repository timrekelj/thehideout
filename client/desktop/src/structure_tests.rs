//! Sections, reordering and channel settings, driven through the real view against the fake
//! backend: the layout draws in order, folds persist, every move sends the right `UpdateLayout`,
//! a stale one refetches, and only people who may manage channels get the controls.

use std::sync::{Arc, Mutex};

use rune::prefs::Prefs;
use rune::{Key, Runtime};
use thehideout_api::{ChannelIcon, ChannelIconRef, GuildLayout, PaletteColor};
use thehideout_design::components::ToastKind;
use thehideout_design::icons::catalog;

use crate::app::{fake_now, App};
use crate::backend::Command;
use crate::fake::{
    FakeBackend, ANA, ARCHIVE, BUILDING, COOK_MODS, DESIGN, DEV, GAMING, GENERAL, HANGOUT, MEMES,
    MUSIC, PLANS, RANDOM, TIM, VOICE,
};
use crate::format::Tz;
use crate::scenes::{booted, booted_as};
use crate::state::{AppState, DialogKind, Menu, Phase};
use crate::structure::CONFLICT_TOAST;
use crate::tests::{click, has, key, runtime, settle, type_text};

fn tim() -> (Runtime<App>, FakeBackend) {
    let (app, fake) = booted(true);
    (runtime(app), fake)
}

fn name_key(name: &str) -> String {
    format!("channels-1-row-{name}")
}

fn y_of(rt: &Runtime<App>, name: &str) -> f32 {
    rt.find(&name_key(name))
        .unwrap_or_else(|| panic!("no row {name}"))
        .1
}

fn order(l: &GuildLayout, section: Option<u64>) -> Vec<u64> {
    crate::layout::channels_in(l, section).to_vec()
}

/// The layouts `UpdateLayout` was sent with, in order.
fn layout_sends(fake: &FakeBackend) -> Vec<GuildLayout> {
    crate::backend::Backend::sent(&fake.clone())
        .into_iter()
        .filter_map(|c| {
            if let Command::UpdateLayout { layout, .. } = c {
                Some(layout)
            } else {
                None
            }
        })
        .collect()
}

fn right_click(rt: &mut Runtime<App>, name: &str) {
    let (x, y) = rt
        .find(&name_key(name))
        .unwrap_or_else(|| panic!("no row {name}"));
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt, 6);
}

/// A handler's way in: runs `f` on the app state, wakes the view and lets it settle.
fn act(rt: &mut Runtime<App>, f: impl FnOnce(&mut AppState)) {
    rt.update(|a, _| a.act(f));
    settle(rt, 6);
}

fn toast(rt: &Runtime<App>) -> Option<(ToastKind, String)> {
    rt.state
        .state
        .ui
        .toasts
        .front()
        .map(|t| (t.kind, t.message.clone()))
}

#[test]
fn the_sidebar_draws_the_layout_in_order() {
    let (rt, _) = tim();
    let names = [
        "general",
        "dev",
        "announcements",
        "design",
        "plans",
        "random",
        "music",
        "gaming",
        "memes",
        "books",
    ];
    let ys: Vec<f32> = names.iter().map(|n| y_of(&rt, n)).collect();
    assert!(
        ys.windows(2).all(|w| w[0] < w[1]),
        "top level first, then each section's channels in order: {ys:?}"
    );
    for section in [BUILDING, HANGOUT, ARCHIVE] {
        assert!(
            has(&rt, &format!("section-{section}")),
            "a header for section {section}"
        );
    }
    // A section's header sits above its channels and below the previous group.
    let header = |id: u64| rt.find(&format!("section-{id}/header")).unwrap().1;
    assert!(
        y_of(&rt, "announcements") < header(BUILDING) && header(BUILDING) < y_of(&rt, "design")
    );
    assert!(y_of(&rt, "plans") < header(HANGOUT) && header(HANGOUT) < y_of(&rt, "random"));
}

#[test]
fn channels_show_their_own_icon_and_a_guild_without_sections_still_lists_channels() {
    let (mut rt, _) = tim();
    let c = rt.state.state.channel(1, DEV).unwrap();
    assert_eq!((c.icon.as_str(), c.color), ("code", PaletteColor::Blue));
    assert!(catalog::by_name(c.icon.as_str()).is_some());
    // Garage Band has a plain list.
    act(&mut rt, |s| s.go_guild(2));
    settle(&mut rt, 20);
    assert!(has(&rt, "channels-2-row-gigs") && has(&rt, "channels-2-row-general"));
}

#[test]
fn folds_hide_a_sections_channels_and_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("thehideout-collapse-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("prefs.json");
    let start = |path: &std::path::Path| {
        let (mut app, fake) = booted(true);
        let state = std::mem::replace(&mut app.state, AppState::new(fake_now, Tz::Fixed(0)));
        let mut started = App::start(Box::new(fake), state, Arc::new(Mutex::new(Prefs::at(path))));
        started.state.ui.look.spec.appearance = thehideout_design::Appearance::Dark;
        runtime(started)
    };
    let mut rt = start(&path);
    assert!(has(&rt, &name_key("random")));
    click(&mut rt, &format!("section-{HANGOUT}/header/toggle"));
    settle(&mut rt, 40);
    assert!(
        !has(&rt, &name_key("random")),
        "the fold hides its channels"
    );
    assert!(has(&rt, &name_key("design")), "the others stay open");
    assert!(rt.state.state.ui.collapsed.contains(&(1, HANGOUT)));
    drop(rt); // the saver finishes what it was handed
    let saved: std::collections::HashMap<String, Vec<u64>> = Prefs::at(&path)
        .get("collapsed_sections")
        .expect("saved under its key");
    assert_eq!(saved.get("1"), Some(&vec![HANGOUT]));
    // Next launch: read once before the first frame, still folded.
    let rt = start(&path);
    assert!(rt.state.state.ui.collapsed.contains(&(1, HANGOUT)));
    assert!(!has(&rt, &name_key("random")));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_up_from_the_context_menu_sends_the_layout_with_its_version() {
    let (mut rt, fake) = tim();
    right_click(&mut rt, "gaming");
    assert!(matches!(
        rt.state.state.ui.menu,
        Menu::Channel {
            id: GAMING,
            at: Some(_)
        }
    ));
    click(&mut rt, &format!("menu-channel-{GAMING}-item0"));
    settle(&mut rt, 12);
    let sent = layout_sends(&fake);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].version, 3, "based on the version the client holds");
    assert_eq!(
        order(&sent[0], Some(HANGOUT)),
        [RANDOM, GAMING, MUSIC, MEMES]
    );
    assert_eq!(rt.state.state.ui.menu, Menu::None, "the menu closes");
    // The server accepted it, announced it, and the client holds that.
    assert_eq!(
        order(&fake.layout(1), Some(HANGOUT)),
        [RANDOM, GAMING, MUSIC, MEMES]
    );
    assert_eq!(
        rt.state.state.layout(1).unwrap().version,
        fake.layout(1).version
    );
    assert!(y_of(&rt, "gaming") < y_of(&rt, "music"));
}

#[test]
fn stepping_past_a_sections_edge_moves_into_the_next_one() {
    let (mut rt, fake) = tim();
    act(&mut rt, |s| s.step_channel_by(1, MEMES, 1));
    settle(&mut rt, 12);
    let l = fake.layout(1);
    assert_eq!(
        (order(&l, Some(HANGOUT)), order(&l, Some(ARCHIVE))),
        (vec![RANDOM, MUSIC, GAMING], vec![MEMES, 19])
    );
    // And up from the top of Building lands at the end of the top level.
    act(&mut rt, |s| s.step_channel_by(1, DESIGN, -1));
    settle(&mut rt, 12);
    assert_eq!(order(&fake.layout(1), None), [GENERAL, DEV, 14, DESIGN]);
}

#[test]
fn move_to_section_from_the_menu_appends_to_it() {
    let (mut rt, fake) = tim();
    right_click(&mut rt, "plans");
    click(&mut rt, &format!("menu-channel-{PLANS}-item2"));
    settle(&mut rt, 8);
    assert!(
        matches!(rt.state.state.ui.menu, Menu::MoveTo { id: PLANS, .. }),
        "the submenu opens"
    );
    // Top level, Hangout, Archive (not Building: it is already there).
    click(&mut rt, &format!("menu-channel-{PLANS}-item2-sub-item0"));
    settle(&mut rt, 12);
    assert!(
        order(&fake.layout(1), None).ends_with(&[PLANS]),
        "{:?}",
        fake.layout(1)
    );
    assert_eq!(layout_sends(&fake).len(), 1);
}

#[test]
fn a_move_shows_at_once_and_a_late_answer_changes_nothing() {
    let (app, fake) = booted(true);
    fake.set_latency(0.5);
    let mut rt = runtime(app);
    act(&mut rt, |s| s.step_channel_by(1, GAMING, -1));
    settle(&mut rt, 2);
    assert_eq!(
        order(rt.state.state.layout(1).unwrap(), Some(HANGOUT)),
        [RANDOM, GAMING, MUSIC, MEMES],
        "optimistic"
    );
    assert!(
        rt.state.state.data.layout_undo.contains_key(&1),
        "waiting for the server"
    );
    // A second edit waits: it would be based on a version the server is about to retire.
    act(&mut rt, |s| s.step_channel_by(1, GAMING, -1));
    assert_eq!(layout_sends(&fake).len(), 1);
    settle(&mut rt, 60);
    assert!(rt.state.state.data.layout_undo.is_empty());
    assert_eq!(
        order(rt.state.state.layout(1).unwrap(), Some(HANGOUT)),
        [RANDOM, GAMING, MUSIC, MEMES]
    );
}

#[test]
fn a_stale_version_refetches_and_says_so() {
    let (mut rt, fake) = tim();
    fake.conflict_next_layout();
    act(&mut rt, |s| s.step_channel_by(1, GAMING, -1));
    settle(&mut rt, 20);
    assert_eq!(
        toast(&rt),
        Some((ToastKind::Info, CONFLICT_TOAST.to_owned()))
    );
    assert_eq!(
        CONFLICT_TOAST,
        "Layout changed elsewhere \u{2014} try again"
    );
    let sent = crate::backend::Backend::sent(&fake.clone());
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::LoadChannels { guild: 1 })),
        "refetched"
    );
    // The edit is gone and the client holds the server's newer layout.
    assert_eq!(
        order(rt.state.state.layout(1).unwrap(), Some(HANGOUT)),
        [RANDOM, MUSIC, GAMING, MEMES]
    );
    assert_eq!(
        rt.state.state.layout(1).unwrap().version,
        fake.layout(1).version
    );
    // Trying again works.
    act(&mut rt, |s| s.step_channel_by(1, GAMING, -1));
    settle(&mut rt, 12);
    assert_eq!(
        order(&fake.layout(1), Some(HANGOUT)),
        [RANDOM, GAMING, MUSIC, MEMES]
    );
}

#[test]
fn another_failure_puts_the_row_back() {
    let (mut rt, fake) = tim();
    act(&mut rt, |s| s.go_guild(3));
    settle(&mut rt, 20);
    assert!(
        rt.state.state.can_manage_channels(3),
        "a moderator, not the owner"
    );
    // Tim loses the role without the client hearing yet, so the server refuses the edit.
    fake.set_moderator(3, TIM, false);
    rt.state
        .state
        .data
        .member_roles
        .get_mut(&3)
        .unwrap()
        .insert(TIM, vec![COOK_MODS]);
    rt.state.act(|s| s.step_channel_by(3, 31, -1));
    assert_eq!(
        order(rt.state.state.layout(3).unwrap(), None),
        [31, 30],
        "shown at once"
    );
    rt.invalidate();
    settle(&mut rt, 20);
    assert_eq!(
        order(rt.state.state.layout(3).unwrap(), None),
        [30, 31],
        "and back again"
    );
    let (kind, message) = toast(&rt).expect("a toast says why");
    assert_eq!(kind, ToastKind::Danger);
    assert!(message.contains("Permission denied"), "{message}");
}

#[test]
fn dragging_a_row_down_two_places_sends_that_order() {
    let (mut rt, fake) = tim();
    let (x, y) = rt.find(&name_key("random")).unwrap();
    rt.pointer_down(x, y);
    settle(&mut rt, 1);
    for step in 1..=7 {
        rt.pointer_move(x, y + step as f32 * 10.0);
        settle(&mut rt, 1);
    }
    assert!(rt.state.state.ui.drag.is_some(), "a drag is in progress");
    rt.pointer_up(x, y + 70.0);
    settle(&mut rt, 12);
    assert!(rt.state.state.ui.drag.is_none());
    let sent = layout_sends(&fake);
    assert_eq!(sent.len(), 1, "a drop is one edit");
    assert_eq!(
        order(&sent[0], Some(HANGOUT)),
        [MUSIC, GAMING, RANDOM, MEMES]
    );
}

#[test]
fn a_press_that_barely_moves_is_a_click_not_a_drag() {
    let (mut rt, fake) = tim();
    let (x, y) = rt.find(&name_key("dev")).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_move(x, y + 2.0);
    rt.pointer_up(x, y + 2.0);
    settle(&mut rt, 6);
    assert!(layout_sends(&fake).is_empty());
    assert_eq!(
        rt.state.state.current_channel(),
        Some(DEV),
        "it selected the channel"
    );
}

#[test]
fn dragging_a_section_header_reorders_the_sections() {
    let (mut rt, fake) = tim();
    // Fold Building first so its group is one row tall; then drag it past Hangout (3 + 1 rows).
    act(&mut rt, |s| s.toggle_section(1, BUILDING));
    settle(&mut rt, 30);
    let (x, y) = rt
        .find(&format!("section-{BUILDING}/header/toggle"))
        .unwrap();
    rt.pointer_down(x + 40.0, y);
    for step in 1..=12 {
        rt.pointer_move(x + 40.0, y + step as f32 * 10.0);
        settle(&mut rt, 1);
    }
    rt.pointer_up(x + 40.0, y + 120.0);
    settle(&mut rt, 12);
    let l = fake.layout(1);
    assert_eq!(
        l.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
        [HANGOUT, BUILDING, ARCHIVE, VOICE]
    );
}

#[test]
fn a_section_menu_renames_moves_and_creates_a_channel_in_it() {
    let (mut rt, fake) = tim();
    act(&mut rt, |s| {
        s.ui.menu = Menu::Section {
            id: HANGOUT,
            at: None,
        }
    });
    settle(&mut rt, 6);
    // Move up
    click(&mut rt, &format!("menu-section-more-{HANGOUT}-item1"));
    settle(&mut rt, 12);
    assert_eq!(
        fake.layout(1)
            .sections
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        [HANGOUT, BUILDING, ARCHIVE, VOICE]
    );
    // Create channel here
    act(&mut rt, |s| s.open_create_channel_in(1, ARCHIVE));
    let f = rt.state.state.channel_form().unwrap();
    assert_eq!(f.section, Some(ARCHIVE));
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("Old News");
    act(&mut rt, |s| {
        s.channel_form().unwrap().color = "teal";
        s.channel_form().unwrap().icon = "newspaper".into();
        s.submit_dialog()
    });
    settle(&mut rt, 12);
    let created = fake
        .channel(1000 + 10)
        .or_else(|| (1000..1100).find_map(|i| fake.channel(i)))
        .expect("created");
    assert_eq!(
        (created.name.as_str(), created.section_id, created.color),
        ("old-news", Some(ARCHIVE), PaletteColor::Teal)
    );
    assert_eq!(created.icon, ChannelIcon::new("newspaper"));
    assert!(order(&fake.layout(1), Some(ARCHIVE)).contains(&created.id));
    assert!(rt.state.state.ui.dialog.is_none());
}

#[test]
fn sections_are_created_renamed_and_deleted_through_dialogs() {
    let (mut rt, fake) = tim();
    act(&mut rt, |s| s.open_dialog(DialogKind::CreateSection { guild: 1 }));
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("Pets");
    act(&mut rt, |s| s.submit_dialog());
    settle(&mut rt, 12);
    let pets = fake.layout(1).sections.last().unwrap().clone();
    assert_eq!(pets.name, "Pets");
    assert!(has(&rt, &format!("section-{}", pets.id)));
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::RenameSection {
            guild: 1,
            section: pets.id,
        })
    });
    assert_eq!(
        rt.state.state.ui.dialog.as_ref().unwrap().input.text,
        "Pets",
        "starts from the current name"
    );
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("Animals");
    act(&mut rt, |s| s.submit_dialog());
    settle(&mut rt, 12);
    assert_eq!(fake.layout(1).sections.last().unwrap().name, "Animals");
    // Deleting asks first, and its channels survive at the top.
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::DeleteSection {
            guild: 1,
            section: ARCHIVE,
        })
    });
    assert!(has(&rt, "dialog-submit"));
    assert!(
        fake.layout(1).sections.iter().any(|s| s.id == ARCHIVE),
        "nothing yet"
    );
    act(&mut rt, |s| s.submit_dialog());
    settle(&mut rt, 12);
    let l = fake.layout(1);
    assert!(!l.sections.iter().any(|s| s.id == ARCHIVE));
    assert!(
        order(&l, None).contains(&19),
        "books moved to the top level"
    );
    assert!(rt.state.state.ui.dialog.is_none());
}

#[test]
fn only_people_who_may_manage_channels_get_the_structure_controls() {
    // Ana is a plain member of The Hideout.
    let (app, fake) = booted_as(true, "ana");
    let mut rt = runtime(app);
    assert!(!rt.state.state.can_manage_channels(1));
    assert!(
        !has(&rt, "channel-settings-button"),
        "no gear in the header"
    );
    assert!(
        !has(&rt, &format!("channel-more-{DEV}")),
        "no \"...\" on rows"
    );
    assert!(!has(&rt, &format!("section-more-{HANGOUT}")));
    right_click(&mut rt, "gaming");
    assert_eq!(rt.state.state.ui.menu, Menu::None, "no context menu");
    // A drag does nothing, and the state refuses an edit outright.
    let (x, y) = rt.find(&name_key("random")).unwrap();
    rt.pointer_down(x, y);
    for step in 1..=7 {
        rt.pointer_move(x, y + step as f32 * 10.0);
        settle(&mut rt, 1);
    }
    rt.pointer_up(x, y + 70.0);
    settle(&mut rt, 6);
    assert!(rt.state.state.ui.drag.is_none() && layout_sends(&fake).is_empty());
    assert!(!rt
        .state
        .state
        .edit_layout(1, rt.state.state.layout(1).unwrap().clone()));
    // Opening the guild menu: creating is disabled.
    click(&mut rt, "guild-header");
    settle(&mut rt, 8);
    // Garage Band is Ana's own: the controls are there.
    act(&mut rt, |s| s.go_guild(2));
    settle(&mut rt, 20);
    assert!(rt.state.state.can_manage_channels(2));
    assert!(has(&rt, "channel-settings-button"));
}

#[test]
fn a_moderator_who_is_not_the_owner_can_manage_and_roles_events_change_it() {
    let (mut rt, fake) = tim();
    act(&mut rt, |s| s.go_guild(3));
    settle(&mut rt, 20);
    assert!(
        rt.state.state.can_manage_channels(3),
        "from the Moderator role"
    );
    // Roles and members arrive on the gateway while the app is open.
    fake.set_moderator(3, TIM, false);
    settle(&mut rt, 6);
    assert!(
        !rt.state.state.can_manage_channels(3),
        "MemberUpdated takes the role away"
    );
    assert!(!has(&rt, "channel-settings-button"));
    fake.set_moderator(3, TIM, true);
    settle(&mut rt, 6);
    assert!(rt.state.state.can_manage_channels(3));
    // And a plain member promoted elsewhere.
    let (app, fake) = booted_as(true, "ana");
    let mut rt = runtime(app);
    assert!(!rt.state.state.can_manage_channels(1));
    fake.set_moderator(1, ANA, true);
    settle(&mut rt, 6);
    assert!(rt.state.state.can_manage_channels(1));
    assert!(has(&rt, "channel-settings-button"));
}

#[test]
fn channel_settings_save_sends_only_what_changed_and_shows_the_result() {
    let (mut rt, fake) = tim();
    click(&mut rt, "channel-settings-button");
    settle(&mut rt, 8);
    assert!(matches!(
        rt.state.state.ui.dialog.as_ref().map(|d| d.kind),
        Some(DialogKind::ChannelSettings {
            guild: 1,
            channel: GENERAL
        })
    ));
    assert_eq!(
        rt.state.state.ui.dialog.as_ref().unwrap().input.text,
        "general"
    );
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .channel
        .as_mut()
        .unwrap()
        .topic
        .replace_text("  The lobby  ");
    act(&mut rt, |s| {
        s.pick_color("pink");
        s.pick_icon("house");
    });
    act(&mut rt, |s| s.submit_dialog());
    let sent = crate::backend::Backend::sent(&fake.clone());
    let Some(Command::UpdateChannel {
        name,
        icon,
        color,
        topic,
        channel,
        ..
    }) = sent.last().cloned()
    else {
        panic!("{sent:?}")
    };
    assert_eq!(channel, GENERAL);
    assert_eq!(
        (name, icon, color, topic),
        (
            None,
            Some(ChannelIconRef::Phosphor("house".into())),
            Some(PaletteColor::Pink),
            Some("The lobby".to_owned())
        )
    );
    settle(&mut rt, 12);
    assert!(rt.state.state.ui.dialog.is_none(), "closed on success");
    let c = rt.state.state.channel(1, GENERAL).unwrap();
    assert_eq!(
        (c.icon.as_str(), c.color, c.topic.as_str()),
        ("house", PaletteColor::Pink, "The lobby")
    );
    assert_eq!(fake.channel(GENERAL).unwrap().topic, "The lobby");
    // Nothing changed: no command.
    let before = crate::backend::Backend::sent(&fake.clone()).len();
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: GENERAL,
        })
    });
    act(&mut rt, |s| s.submit_dialog());
    assert_eq!(crate::backend::Backend::sent(&fake.clone()).len(), before);
    assert!(rt.state.state.ui.dialog.is_none());
}

#[test]
fn channel_settings_errors_show_inline_and_the_header_follows_the_topic() {
    let (mut rt, _fake) = tim();
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: DEV,
        })
    });
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("general");
    rt.state.act(|s| s.submit_dialog());
    assert_eq!(
        rt.state.state.ui.dialog.as_ref().unwrap().phase,
        Phase::Busy
    );
    rt.invalidate();
    settle(&mut rt, 12);
    let d = rt.state.state.ui.dialog.as_ref().expect("stays open");
    assert_eq!(
        (d.phase, d.error.as_deref()),
        (Phase::Idle, Some("That name is already taken here."))
    );
    // Over-long topics are refused before sending.
    rt.state
        .state
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("dev");
    rt.state
        .state
        .channel_form()
        .unwrap()
        .topic
        .replace_text("x".repeat(300));
    act(&mut rt, |s| s.submit_dialog());
    assert!(rt
        .state
        .state
        .ui
        .dialog
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("256"));
    // A topic on the open channel shows beside its name in the header.
    assert!(has(&rt, "dlg-dialog"));
}

#[test]
fn the_icon_picker_searches_names_and_tags_and_picks_with_the_keyboard() {
    let (mut rt, _) = tim();
    act(&mut rt, |s| s.open_dialog(DialogKind::CreateChannel { guild: 1 }));
    click(&mut rt, "icon-field");
    settle(&mut rt, 8);
    assert!(rt.state.state.channel_form().unwrap().picker_open);
    assert!(has(&rt, "icon-picker") && has(&rt, "picker-tabs"));
    assert!(
        rt.state.state.channel_form().unwrap().results.len() >= 1500,
        "the whole catalog before a search"
    );
    type_text(&mut rt, "xbox");
    settle(&mut rt, 4);
    let first = rt.state.state.channel_form().unwrap().results[0].icon.name;
    assert_eq!(first, "game-controller", "found by tag");
    assert!(rt.state.state.channel_form().unwrap().results.len() < 20);
    key(&mut rt, Key::Right);
    key(&mut rt, Key::Left);
    key(&mut rt, Key::Enter);
    settle(&mut rt, 6);
    let f = rt.state.state.channel_form().unwrap();
    assert_eq!(f.icon, "game-controller");
    assert!(!f.picker_open, "choosing closes the picker");
    // Colours: pick one, then none.
    click(&mut rt, "swatch-red");
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.channel_form().unwrap().color, "red");
    click(&mut rt, "swatch-none");
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.channel_form().unwrap().color, "");
}

#[test]
fn the_emoji_tabs_are_enabled_and_take_the_pick() {
    let (mut rt, _) = tim();
    act(&mut rt, |s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: DEV,
        })
    });
    act(&mut rt, |s| s.toggle_picker());
    settle(&mut rt, 6);
    assert!(has(&rt, "picker-tab-0"));
    for (tab, n) in [("picker-tab-1", 1), ("picker-tab-2", 2)] {
        let at = rt.find(tab).expect("the tab is there");
        crate::tests::click_at(&mut rt, at);
        settle(&mut rt, 3);
        assert_eq!(rt.state.state.channel_form().unwrap().tab, n, "{tab} is enabled");
    }
}

#[test]
fn the_channel_header_shows_the_icon_colour_and_topic() {
    let (rt, _) = tim();
    // #general: its topic is in the header beside its name.
    assert!(rt
        .state
        .state
        .current_guild_channel()
        .is_some_and(|c| !c.topic.is_empty()));
    assert!(has(&rt, "channel-settings-button"));
}

#[test]
fn a_gateway_channel_update_changes_the_row_for_a_second_client() {
    let (mut rt, fake) = tim();
    // Another client changes #dev's look through the same server.
    let other = booted(true);
    other.1.set_latency(0.0);
    let (mut app2, fake2) = other;
    app2.act(|s| {
        s.open_dialog(DialogKind::ChannelSettings {
            guild: 1,
            channel: DEV,
        });
        s.channel_form().unwrap().icon = "rocket".into();
        s.submit_dialog();
    });
    app2.pump();
    assert_eq!(fake2.channel(DEV).unwrap().icon, ChannelIcon::new("rocket"));
    // The gateway delivers the same event to this client.
    fake.gateway(thehideout_api::GatewayUpdate::Event(
        thehideout_api::Event::ChannelUpdated(fake2.channel(DEV).unwrap()),
    ));
    settle(&mut rt, 6);
    assert_eq!(
        rt.state.state.channel(1, DEV).unwrap().icon.as_str(),
        "rocket"
    );
}

#[test]
fn the_more_button_sits_at_the_right_edge_of_its_row_and_shows_on_hover() {
    let (mut rt, _) = tim();
    let row = crate::tests::rect_of(&rt, &name_key("random"));
    let more = crate::tests::rect_of(&rt, &format!("channel-more-{RANDOM}"));
    assert!(more[0] + more[2] <= row[0] + row[2] + 0.5 && more[0] > row[0] + row[2] / 2.0, "right half of the row: {more:?} in {row:?}");
    assert!(more[1] >= row[1] - 0.5 && more[1] + more[3] <= row[1] + row[3] + 0.5, "inside the row's height: {more:?} in {row:?}");
    // Pressing it opens the same menu the right-click does.
    click(&mut rt, &format!("channel-more-{RANDOM}"));
    settle(&mut rt, 6);
    assert_eq!(rt.state.state.ui.menu, Menu::Channel { id: RANDOM, at: None });
    assert!(has(&rt, &format!("menu-channel-more-{RANDOM}-item0")));
}
