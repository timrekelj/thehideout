//! Manual test with the real microphone and speakers.
//!
//! ```text
//! cargo run -p thehideout-voice --example voice_cli -- <ws-url> <token> [--ptt] [--muted] [--synthetic]
//! ```
//!
//! Prints every event. Type `m` + Enter to toggle mute, `d` deafen, `p` to hold/release
//! push-to-talk, `l` to list devices, `q` to quit. On macOS the terminal (or the bundled app)
//! needs microphone permission.

use std::io::BufRead;
use std::sync::mpsc;
use std::time::Duration;

use thehideout_voice::{ConnectOptions, VoiceEngine, VoiceEvent, VoiceMode};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(url), Some(token)) = (args.next(), args.next()) else {
        eprintln!("usage: voice_cli <ws-url> <token> [--ptt] [--muted] [--synthetic]");
        std::process::exit(2);
    };
    let flags: Vec<String> = args.collect();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (engine, events) = VoiceEngine::new(rt.handle().clone());
    if flags.iter().any(|f| f == "--ptt") {
        engine.set_mode(VoiceMode::PushToTalk);
    }
    engine.list_devices();
    engine.connect(url, token, ConnectOptions {
            start_muted: flags.iter().any(|f| f == "--muted"),
            // A generated tone instead of the microphone (no hardware or permission needed).
            audio_input: if flags.iter().any(|f| f == "--synthetic") {
                thehideout_voice::AudioInput::Synthetic(Default::default())
            } else {
                Default::default()
            },
            ..Default::default()
        });

    let (line_tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                return;
            }
        }
    });
    let (mut muted, mut deaf, mut ptt) = (false, false, false);
    loop {
        // The same non-blocking drain the desktop app does from its poll loop.
        while let Ok(e) = events.try_recv() {
            match e {
                VoiceEvent::Levels(l) => {
                    let bars: Vec<String> =
                        l.iter().map(|(id, v)| format!("{}:{}", if id.is_empty() { "me" } else { id }, "#".repeat((v * 20.0) as usize))).collect();
                    println!("levels {}", bars.join("  "));
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
