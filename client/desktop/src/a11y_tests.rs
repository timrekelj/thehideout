//! The accessibility tree of the key screens, as VoiceOver, Narrator and automation read it
//! (`Runtime::accessibility_json`): every element a user can press, focus or edit says what it is
//! and what it is called, and the controls a driver looks up by key are the controls themselves.

use rune::accesskit::{Action, ActionData};
use rune::Runtime;
use serde_json::Value;

use crate::app::App;
use crate::scenes::{booted, SCENES};
use crate::state::Menu;
use crate::tests::{runtime, settle};

/// Actions that make an element something to operate rather than something to read.
const OPERATE: &[&str] = &[
    "click",
    "focus",
    "set-value",
    "expand",
    "collapse",
    "context-menu",
    "increment",
    "decrement",
];

fn scene(name: &str) -> Runtime<App> {
    let (_, f) = SCENES
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no scene {name}"));
    let (mut app, _fake) = booted(true);
    f(&mut app);
    let mut rt = runtime(app);
    settle(&mut rt, 30);
    rt
}

fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(n);
    for c in n["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}

fn nodes(tree: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    walk(tree, &mut out);
    out
}

fn by_key<'a>(tree: &'a Value, key: &str) -> &'a Value {
    nodes(tree)
        .into_iter()
        .find(|n| n["key"] == key)
        .unwrap_or_else(|| panic!("no accessible element keyed {key}"))
}

fn actions(n: &Value) -> Vec<&str> {
    n["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn name(n: &Value) -> &str {
    n["name"].as_str().unwrap_or("")
}

/// Every element that can be pressed, focused or edited has a control's role and a name.
fn audit(scene_name: &str) -> usize {
    let rt = scene(scene_name);
    let tree = rt.accessibility_json();
    let mut operable = 0;
    let mut bad = Vec::new();
    for n in nodes(&tree) {
        if !actions(n).iter().any(|a| OPERATE.contains(a)) {
            continue;
        }
        operable += 1;
        let role = n["role"].as_str().unwrap_or("");
        if matches!(role, "" | "group" | "generic-container" | "unknown")
            || name(n).trim().is_empty()
        {
            bad.push(format!(
                "{role} key={} name={:?} actions={:?}",
                n["key"],
                name(n),
                actions(n)
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{scene_name}: controls without a role or a name:\n{}",
        bad.join("\n")
    );
    operable
}

#[test]
fn every_operable_element_has_a_role_and_a_name() {
    for s in [
        "main",
        "home-dms",
        "settings-security",
        "settings-voice",
        "settings-voice-ptt",
        "admin-overview",
        "admin-invites",
        "invite-by-email",
        "reactions",
        "attachments-composer",
        "attachments-messages",
        "embeds",
        "sections",
        "voice-sidebar",
        "voice-connected",
        "voice-server-muted",
        "voice-stage",
        "voice-stage-screen",
        "voice-screen-picker",
        "voice-sidebar-live",
        "dialog-create-voice",
    ] {
        let n = audit(s);
        assert!(
            n >= 5,
            "{s}: only {n} operable elements, the scene did not build"
        );
    }
}

#[test]
fn rail_add_tile_is_a_button_that_opens_its_menu() {
    let mut rt = scene("main");
    let tree = rt.accessibility_json();
    let add = by_key(&tree, "rail-add");
    assert_eq!(add["role"], "button", "{add}");
    assert_eq!(name(add), "Add a server");
    assert!(actions(add).contains(&"click"), "{add}");
    assert!(
        add["states"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s == "collapsed")),
        "{add}"
    );
    // Servers and Home are buttons too, the selected one says so.
    let home = by_key(&tree, "home");
    assert_eq!(
        (home["role"].as_str(), name(home)),
        (Some("button"), "Home, direct messages")
    );
    let hideout = by_key(&tree, "guild-1");
    assert_eq!(
        (hideout["role"].as_str(), name(hideout)),
        (Some("button"), "The Hideout")
    );
    assert_eq!(hideout["value"], "selected");
    // Pressing it the way assistive technology does opens the menu.
    assert!(rt.accessibility_action("rail-add", Action::Click, None));
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.ui.menu, Menu::Add);
}

#[test]
fn channel_rows_are_named_buttons_with_their_selection() {
    let rt = scene("main");
    let tree = rt.accessibility_json();
    let general = by_key(&tree, "channels-1-row-general");
    assert_eq!(general["role"], "button", "{general}");
    assert_eq!(name(general), "general");
    assert!(actions(general).contains(&"click"));
    assert_eq!(general["value"], "selected");
    let dev = by_key(&tree, "channels-1-row-dev");
    assert_eq!((dev["role"].as_str(), name(dev)), (Some("button"), "dev"));
    assert!(dev["value"].is_null(), "{dev}");
    assert_eq!(dev["description"], "1 mention");
    // No unnamed layout box (a menu's anchor) sits between the list and its rows.
    let top = by_key(&tree, "channels-top");
    for c in top["children"].as_array().unwrap() {
        assert_eq!(c["role"], "button", "{c}");
    }
}

#[test]
fn composer_field_is_a_named_text_field_inside_the_composer() {
    let mut rt = scene("main");
    let tree = rt.accessibility_json();
    let composer = by_key(&tree, "composer");
    let field = composer["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == "input")
        .unwrap_or_else(|| panic!("the field is not a child of the composer: {composer}"));
    assert_eq!(field["role"], "multiline-text-input", "{field}");
    assert_eq!(name(field), "Message #general");
    assert!(actions(field).contains(&"set-value"), "{field}");
    assert_eq!(
        nodes(&tree).iter().filter(|n| n["key"] == "input").count(),
        1
    );
    // Setting its value types into the draft.
    assert!(rt.accessibility_action(
        "input",
        Action::SetValue,
        Some(ActionData::Value("hello from VoiceOver".into()))
    ));
    settle(&mut rt, 3);
    assert_eq!(rt.state.state.composer().text, "hello from VoiceOver");
}

#[test]
fn page_headers_close_button_lines_its_glyph_up_with_the_content_edge() {
    // Settings, server settings and admin pages share one header: the close button's box overhangs
    // the content column by the 8pt its glyph sits inside, so the glyph (not the hover box) lines up
    // with the right edge of the controls below.
    use crate::tests::rect_of;
    for name in [
        "settings-appearance",
        "settings-voice",
        "settings-voice-ptt",
        "settings-security",
        "server-settings-overview",
        "server-settings-roles",
        "admin-overview",
        "admin-users",
        "admin-invites",
    ] {
        let rt = scene(name);
        let key = if name.starts_with("admin") { "admin-header" } else { "page-header" };
        let header = rect_of(&rt, key);
        let close = rect_of(&rt, "settings-close");
        assert_eq!(close[0] + close[2], header[0] + header[2] + 8.0, "{name}: close button");
    }
}

#[test]
fn the_voice_page_names_every_control_and_reads_the_meter_and_slider_as_ranges() {
    let rt = scene("settings-voice");
    let tree = rt.accessibility_json();
    assert_eq!(name(by_key(&tree, "settings-nav-voice")), "Voice & audio");
    // The selects, the toggle and the switches are named for what they do.
    for (key, want) in [
        ("voice-input-select-trigger", "Input device"),
        ("voice-output-select-trigger", "Output device"),
        ("voice-mic-test", "Stop test"),
        ("voice-key-mute", "Mute shortcut"),
        ("voice-key-deafen", "Deafen shortcut"),
    ] {
        let n = by_key(&tree, key);
        assert!(name(n).contains(want), "{key}: {n}");
    }
    // The sensitivity slider is a slider with a number, steps with the arrows and reads in percent.
    let slider = by_key(&tree, "voice-sensitivity");
    assert_eq!(slider["role"], "slider", "{slider}");
    assert_eq!(name(slider), "Sensitivity");
    assert!(actions(slider).contains(&"increment") && actions(slider).contains(&"decrement"), "{slider}");
    // The meter is a meter, named, not operable.
    let meter = by_key(&tree, "meter");
    assert_eq!(meter["role"], "meter", "{meter}");
    assert_eq!(name(meter), "Microphone level");
    assert!(!actions(meter).iter().any(|a| OPERATE.contains(a)), "{meter}");
    // The push-to-talk page has its recorder, named, and no slider.
    let ptt = scene("settings-voice-ptt");
    let tree = ptt.accessibility_json();
    assert!(name(by_key(&tree, "voice-key-ptt")).contains("Push to talk key"));
    assert!(nodes(&tree).iter().all(|n| n["key"] != "voice-sensitivity"));
}

#[test]
fn in_push_to_talk_the_mute_button_says_so() {
    let mut rt = scene("voice-connected");
    let tree = rt.accessibility_json();
    assert!(by_key(&tree, "voice-mute")["description"].is_null());
    rt.state.state.voice.prefs.mode = crate::voice::InputMode::PushToTalk;
    rt.update(|a, _| a.act(|_| {}));
    settle(&mut rt, 3);
    let tree = rt.accessibility_json();
    assert_eq!(by_key(&tree, "voice-mute")["description"], "Push to talk");
}
