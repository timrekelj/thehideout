//! The Voice & audio page of the settings: which microphone and speakers to use, a mic test with a
//! level meter, how the microphone opens (voice activity with a sensitivity, or push to talk), the
//! hotkeys, noise suppression and echo cancellation, and the sounds.
//!
//! It is laid out exactly like Appearance: the page header with Esc and close, then settings told
//! apart by spacing, each a title, a muted description and a control on the text column's left edge.
//! Every control applies at once and is saved by the prefs saver (`look.rs`).
//!
//! Only what the engine can do is offered: there is no input volume (the platform audio module has
//! no gain) and no per-user volume (people can only be muted for yourself, from the sidebar).

use rune::{col, row, text, Drag, El, Phase, Shortcut, ShortcutError, Tx};
use rune_kit::{Kit, Select, SelectAppearance, ShortcutRecorder, ShortcutRecorderState};
use thehideout_design::components::{
    slider_fraction, switch, Button, LevelMeter, Segmented, Slider, Variant, THUMB,
};
use thehideout_design::icons::{MICROPHONE, STOP};
use thehideout_design::{theme_of, type_};

use super::settings::{header, setting, toggle};
use crate::app::App;
use crate::voice::{device_options, InputMode, VoiceKey, VoiceMenu};

/// The width of the selects, the slider and the meter.
const CONTROL_W: f32 = 320.0;
/// The shortcut recorders' width (a keycap row and its border).
const KEY_W: f32 = CONTROL_W - KEY_LABEL_W - KEY_GAP;
/// The name column and the gap before a recorder: together with it they are the selects' width.
const KEY_LABEL_W: f32 = 88.0;
const KEY_GAP: f32 = 8.0;

pub fn page(a: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let s = &a.state;
    let prefs = &s.voice.prefs;
    let ptt = prefs.mode == InputMode::PushToTalk;

    let input = device_select(a, k, layer, VoiceMenu::Input);
    let output = device_select(a, k, layer, VoiceMenu::Output);
    // The camera is picked here on the web only (the desktop uses the system's).
    let camera = (!crate::voice::app_screen_picker()).then(|| device_select(a, k, layer, VoiceMenu::Camera));

    let page = col()
        .key("page-voice")
        .w_full()
        .max_w(super::settings::PAGE_W)
        .px(32.0)
        .pb(32.0)
        .child(header(k, "Voice & audio", |a: &mut App| {
            a.act(|s| s.close_settings())
        }))
        .child(setting(
            k,
            "Input device",
            "The microphone people hear.",
            col().key("set-input").w(CONTROL_W).child(input),
        ))
        .child(setting(
            k,
            "Output device",
            "Where you hear other people. Join, leave and mute sounds play on the system default.",
            col().key("set-output").w(CONTROL_W).child(output),
        ))
        .children(camera.map(|c| {
            setting(k, "Camera", "The camera people see when you turn it on.", col().key("set-camera").w(CONTROL_W).child(c))
        }))
        .child(setting(
            k,
            "Test your microphone",
            "Speak to see the level. Nothing is sent to anyone.",
            mic_test(a, k),
        ))
        .child(setting(
            k,
            "Input mode",
            if ptt {
                "Your microphone is open only while you hold the key."
            } else {
                "Your microphone opens when you speak."
            },
            col()
                .key("set-mode")
                .w_full()
                .gap(12.0)
                .child(col().w(CONTROL_W).child(mode_picker(k, prefs.mode)))
                .child(if ptt {
                    ptt_controls(a, k)
                } else {
                    sensitivity(a, k)
                }),
        ))
        .child(setting(
            k,
            "Shortcuts",
            "Mute and deafen from the keyboard. They work while thehideout is the focused window.",
            col()
                .key("set-shortcuts")
                .w_full()
                .gap(8.0)
                .child(key_row(
                    k,
                    "Mute",
                    recorder(a, k, "voice-key-mute", "Mute shortcut", VoiceKey::Mute),
                ))
                .child(key_row(
                    k,
                    "Deafen",
                    recorder(a, k, "voice-key-deafen", "Deafen shortcut", VoiceKey::Deafen),
                )),
        ))
        .child(toggle(
            k,
            "Noise suppression",
            "Filters out keyboard clatter, fans and other steady background sound.",
            col().key("set-noise").child(
                switch(prefs.noise_suppression)
                    .label("Noise suppression")
                    .hide_label()
                    .on_toggle(|a: &mut App| {
                        a.act(|s| s.voice_set_noise_suppression(!s.voice.prefs.noise_suppression))
                    })
                    .build(k),
            ),
        ))
        .child(toggle(
            k,
            "Echo cancellation",
            "Stops your speakers from feeding back into your microphone.",
            col().key("set-echo").child(
                switch(prefs.echo_cancellation)
                    .label("Echo cancellation")
                    .hide_label()
                    .on_toggle(|a: &mut App| {
                        a.act(|s| s.voice_set_echo_cancellation(!s.voice.prefs.echo_cancellation))
                    })
                    .build(k),
            ),
        ))
        .child(toggle(
            k,
            "Play sounds for joins, leaves and mute",
            "A short sound when someone joins or leaves your channel, and when you mute or deafen.",
            col().key("set-sounds").child(
                switch(s.voice.sounds)
                    .label("Play sounds for joins, leaves and mute")
                    .hide_label()
                    .on_toggle(|a: &mut App| a.act(|s| s.voice_set_sounds(!s.voice.sounds)))
                    .build(k),
            ),
        ));
    // The column is centred in the panel, whatever its width.
    col().w_full().items_center().child(page)
}

/// A select over the devices of one list, "System default" first. A saved device that is not
/// plugged in stays in the list, named, marked "(unavailable)"; the engine uses the default.
fn device_select(a: &App, k: &Kit, layer: &mut Vec<El>, which: VoiceMenu) -> El {
    let s = &a.state;
    let input = which == VoiceMenu::Input;
    let camera = which == VoiceMenu::Camera;
    let (list, default, saved) = {
        let p = &s.voice.prefs;
        let d = s.voice.devices.as_ref();
        if camera {
            (
                d.map(|d| d.cameras.as_slice()),
                d.and_then(|d| d.default_camera.as_deref()),
                p.camera.as_deref().map(|c| (c, p.camera_name.as_deref())),
            )
        } else if input {
            (
                d.map(|d| d.inputs.as_slice()),
                d.and_then(|d| d.default_input.as_deref()),
                p.input.as_deref().map(|i| (i, p.input_name.as_deref())),
            )
        } else {
            (
                d.map(|d| d.outputs.as_slice()),
                d.and_then(|d| d.default_output.as_deref()),
                p.output.as_deref().map(|o| (o, p.output_name.as_deref())),
            )
        }
    };
    let (options, selected) = device_options(list, default, saved);
    let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
    let current = labels[selected].clone();
    let (key, name) = if camera {
        ("voice-camera-select", "Camera")
    } else if input {
        ("voice-input-select", "Input device")
    } else {
        ("voice-output-select", "Output device")
    };
    Select::new(key, &labels)
        .appearance(SelectAppearance::Field)
        .width(CONTROL_W)
        .label(name)
        .current(Some(current.as_str()))
        .open(s.ui.voice_menu == which)
        .on_toggle(move |a: &mut App| {
            a.state.ui.voice_menu = if a.state.ui.voice_menu == which {
                VoiceMenu::None
            } else {
                which
            };
        })
        .on_close(|a: &mut App| a.state.ui.voice_menu = VoiceMenu::None)
        .on_choose(move |a: &mut App, label: &str, _tx: &mut Tx| {
            let chosen = options
                .iter()
                .find(|o| o.label == label)
                .and_then(|o| o.id.clone());
            a.act(|s| {
                if camera {
                    s.voice_set_camera(chosen)
                } else if input {
                    s.voice_set_input(chosen)
                } else {
                    s.voice_set_output(chosen)
                }
                s.ui.voice_menu = VoiceMenu::None;
            });
        })
        .build(k, layer)
}

/// The Test mic toggle and the level meter beside it (a fixed level in scenes; the engine feeds it
/// in the app). The test stops by itself when the page is left (`AppState::settle_voice_page`).
fn mic_test(a: &App, k: &Kit) -> El {
    let s = &a.state;
    let on = s.voice.mic_test;
    let prefs = &s.voice.prefs;
    let marker = (prefs.mode == InputMode::VoiceActivity).then_some(prefs.threshold);
    let button = Button::new(if on { "Stop test" } else { "Test mic" })
        .variant(Variant::Secondary)
        .icon(if on { &STOP } else { &MICROPHONE })
        .on_press(|a: &mut App| a.act(|s| s.toggle_mic_test()))
        .build(k)
        .key("voice-mic-test")
        .checked(on);
    row()
        .key("set-mic-test")
        .w_full()
        .gap(16.0)
        .center_y()
        .child(button)
        .child(
            col().w(CONTROL_W - 120.0).child(
                LevelMeter::new(s.voice.mic_level, "Microphone level")
                    .threshold(marker)
                    .build(k),
            ),
        )
}

fn mode_picker(k: &Kit, mode: InputMode) -> El {
    Segmented::new(
        &InputMode::ALL.map(InputMode::name),
        InputMode::ALL.iter().position(|m| *m == mode).unwrap_or(0),
    )
    .label("Input mode")
    .on_pick(|a: &mut App, i| a.act(|s| s.voice_set_mode(InputMode::ALL[i])))
    .build(k)
}

/// The sensitivity slider: the gate's threshold, drawn as the marker on the meter above.
fn sensitivity(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let threshold = a.state.voice.prefs.threshold;
    let slider = Slider::new(threshold, "Sensitivity")
        .value_text(format!("{} percent", (threshold * 100.0).round()))
        // The box overhangs the text column by the thumb's radius (as a swatch's hover box does), so
        // the track itself lines up with the controls above and below.
        .width(CONTROL_W + THUMB)
        .build(k)
        .ml(-THUMB / 2.0)
        .key("voice-sensitivity")
        .on_drag(|a: &mut App, d: &mut Drag, _tx: &mut Tx| {
            if matches!(d.phase, Phase::Down | Phase::Move) && d.w > 0.0 {
                let f = slider_fraction(d.x, d.w);
                a.act(|s| s.voice_set_threshold(f));
            }
        })
        .on_step(|a: &mut App, delta: i32, _tx: &mut Tx| {
            a.act(|s| {
                let t = s.voice.prefs.threshold + delta as f32 * 0.01;
                s.voice_set_threshold(t)
            })
        })
        .on_step_limit(|a: &mut App, max: bool, _tx: &mut Tx| {
            a.act(|s| s.voice_set_threshold(if max { 1.0 } else { 0.0 }))
        });
    col()
        .key("set-sensitivity")
        .w_full()
        .gap(4.0)
        .child(text("Sensitivity").font(type_::at(k, type_::CONTROL)).color(t.text.color()))
        .child(slider)
        .child(
            text("Drag the marker just above your background noise: you transmit while the bar is past it.")
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color())
                .wrap(),
        )
}

/// The push-to-talk key and its note.
fn ptt_controls(a: &App, k: &Kit) -> El {
    let t = theme_of(k);
    let unset = a.state.voice_shortcut(VoiceKey::PushToTalk).is_none();
    let note = if unset {
        "Pick a key to talk with. Until you do, push to talk keeps your microphone closed."
    } else {
        "Works while thehideout is the focused window; other apps can't be heard from here yet. \
         A plain key is not heard while you type in a text box: use a function key, or a chord with Ctrl, Cmd or Alt, to talk while typing."
    };
    col()
        .key("set-ptt")
        .w_full()
        .gap(8.0)
        .child(key_row(
            k,
            "Push to talk",
            recorder(a, k, "voice-key-ptt", "Push to talk key", VoiceKey::PushToTalk),
        ))
        .child(
            text(note)
                .key("ptt-note")
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color())
                .wrap(),
        )
}

/// A hotkey's name and its recorder, side by side.
fn key_row(k: &Kit, name: &str, recorder: El) -> El {
    let t = theme_of(k);
    row()
        .w(CONTROL_W)
        .gap(KEY_GAP)
        .items_start()
        .child(
            row()
                .w(KEY_LABEL_W)
                .h(32.0)
                .center_y()
                .child(
                    text(name.to_owned())
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color()),
                ),
        )
        .child(recorder)
}

/// The recorder of one hotkey. A chord another voice hotkey has is refused with the recorder's own
/// "already in use" text; Backspace clears. The push-to-talk key may be a bare key.
fn recorder(a: &App, k: &Kit, key: &str, label: &str, which: VoiceKey) -> El {
    // The recorder keeps its own copy of the chord while it captures: `ui.voice_keys`.
    fn state_of(a: &mut App, which: VoiceKey) -> &mut ShortcutRecorderState {
        let keys = &mut a.state.ui.voice_keys;
        match which {
            VoiceKey::PushToTalk => &mut keys.ptt,
            VoiceKey::Mute => &mut keys.mute,
            VoiceKey::Deafen => &mut keys.deafen,
        }
    }
    let keys = &a.state.ui.voice_keys;
    let shown = match which {
        VoiceKey::PushToTalk => &keys.ptt,
        VoiceKey::Mute => &keys.mute,
        VoiceKey::Deafen => &keys.deafen,
    };
    let mut r = ShortcutRecorder::new(key, shown, move |a: &mut App| state_of(a, which))
        .label(label)
        .width(KEY_W)
        .on_change(move |a: &mut App, chord: Option<&Shortcut>, _tx: &mut Tx| {
            let mut result: Result<(), ShortcutError> = Ok(());
            a.act(|s| result = s.voice_set_key(which, chord));
            result
        });
    if which == VoiceKey::PushToTalk {
        r = r.allow_unmodified();
    }
    r.build(k)
}
