//! Manual test with the real microphone and speakers.
//!
//! ```text
//! cargo run -p thehideout-voice --example voice_cli -- <ws-url> <token> [--ptt] [--muted] [--synthetic] [--camera]
//! ```
//!
//! Prints every event. Type `m` + Enter to toggle mute, `d` deafen, `p` to hold/release
//! push-to-talk, `c` camera, `s` screen share (with `--synthetic`: a generated picture), `l` to list
//! devices, `q` to quit. A video track from someone else is watched and its frames summarized
//! (`video-frame ...`); `--camera` publishes the camera once connected. On macOS the terminal (or the bundled app)
//! needs microphone permission.

use std::io::BufRead;
use std::sync::mpsc;
use std::time::Duration;

use thehideout_voice::{ConnectOptions, VoiceEngine, VoiceEvent, VoiceMode};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(url), Some(token)) = (args.next(), args.next()) else {
        eprintln!("usage: voice_cli <ws-url> <token> [--ptt] [--muted] [--synthetic] [--camera]");
        std::process::exit(2);
    };
    let flags: Vec<String> = args.collect();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (engine, events) = VoiceEngine::new(rt.handle().clone());
    if flags.iter().any(|f| f == "--ptt") {
        engine.set_mode(VoiceMode::PushToTalk);
    }
    engine.list_devices();
    let synthetic = flags.iter().any(|f| f == "--synthetic");
    engine.connect(url, token, ConnectOptions {
        start_muted: flags.iter().any(|f| f == "--muted"),
        // A generated tone and picture instead of the microphone and camera (no hardware or
        // permission needed).
        audio_input: if synthetic {
            thehideout_voice::AudioInput::Synthetic(Default::default())
        } else {
            Default::default()
        },
        video_input: if synthetic {
            thehideout_voice::VideoInput::Synthetic(Default::default())
        } else {
            Default::default()
        },
        ..Default::default()
    });
    let auto_camera = flags.iter().any(|f| f == "--camera");

    let (line_tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                return;
            }
        }
    });
    let (mut muted, mut deaf, mut ptt, mut camera, mut screen) = (false, false, false, false, false);
    loop {
        // The same non-blocking drain the desktop app does from its poll loop.
        while let Ok(e) = events.try_recv() {
            match e {
                VoiceEvent::Levels(l) => {
                    let bars: Vec<String> =
                        l.iter().map(|(id, v)| format!("{}:{}", if id.is_empty() { "me" } else { id }, "#".repeat((v * 20.0) as usize))).collect();
                    println!("levels {}", bars.join("  "));
                }
                VoiceEvent::Connected => {
                    println!("Connected");
                    if auto_camera {
                        camera = true;
                        engine.set_camera(true);
                    }
                }
                VoiceEvent::VideoTrack { identity, source, track_id } => {
                    println!("VideoTrack {{ identity: {identity:?}, source: {source:?}, track_id: {track_id:?} }}");
                    // Watch what others publish: print what arrives.
                    let sink = engine.subscribe_video(&track_id);
                    std::thread::spawn(move || {
                        let mut seen = 0;
                        for _ in 0..60 {
                            std::thread::sleep(Duration::from_millis(250));
                            if let Some(f) = sink.latest_frame()
                                && f.seq != seen
                            {
                                seen = f.seq;
                                let n = (f.rgba.len() / 4).max(1);
                                let sum = |c: usize| f.rgba.chunks_exact(4).map(|p| p[c] as u64).sum::<u64>() / n as u64;
                                println!("video-frame {track_id} {}x{} seq {} mean-rgb {},{},{}", f.width, f.height, f.seq, sum(0), sum(1), sum(2));
                            }
                        }
                    });
                }
                other => println!("{other:?}"),
            }
        }
        match lines.try_recv() {
            Ok(l) => match l.trim() {
                "m" => {
                    muted = !muted;
                    engine.set_mute(muted);
                }
                "d" => {
                    deaf = !deaf;
                    engine.set_deaf(deaf);
                }
                "p" => {
                    ptt = !ptt;
                    engine.set_ptt(ptt);
                }
                "c" => {
                    camera = !camera;
                    engine.set_camera(camera);
                }
                "s" => {
                    screen = !screen;
                    engine.set_screen_share(screen.then(|| thehideout_voice::ScreenSource {
                        id: 0,
                        title: "synthetic".into(),
                        kind: thehideout_voice::ScreenKind::Screen,
                    }));
                }
                "l" => engine.list_devices(),
                "q" => break,
                _ => {}
            },
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    engine.disconnect();
    std::thread::sleep(Duration::from_millis(500));
}
