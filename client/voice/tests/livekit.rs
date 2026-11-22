//! End-to-end against a real LiveKit server, with a generated tone instead of a microphone.
//! Ignored by default; start the dev server first:
//!
//! ```text
//! docker run --rm -d -p 127.0.0.1:7880:7880 -p 127.0.0.1:7881:7881 -p 127.0.0.1:7882:7882/udp \
//!     livekit/livekit-server --dev --bind 0.0.0.0
//! cargo test -p thehideout-voice -- --ignored
//! ```
//!
//! `LIVEKIT_URL`, `LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` override the dev defaults.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use livekit_api::access_token::{AccessToken, VideoGrants};
use thehideout_voice::{
    AudioInput, ConnectOptions, MediaKind, ScreenKind, ScreenSource, SyntheticPicture, SyntheticTone,
    VideoInput, VideoKind, VoiceEngine, VoiceEvent, VoiceMode,
};

fn url() -> String {
    std::env::var("LIVEKIT_URL").unwrap_or_else(|_| "ws://127.0.0.1:7880".into())
}

fn token(room: &str, identity: &str) -> String {
    let key = std::env::var("LIVEKIT_API_KEY").unwrap_or_else(|_| "devkey".into());
    let secret = std::env::var("LIVEKIT_API_SECRET").unwrap_or_else(|_| "secret".into());
    AccessToken::with_api_key(&key, &secret)
        .with_identity(identity)
        .with_grants(VideoGrants { room_join: true, room: room.into(), ..Default::default() })
        .to_jwt()
        .unwrap()
}

/// Everything an engine has emitted so far, plus waiting for a particular event.
struct Watch {
    rx: Receiver<VoiceEvent>,
    seen: Vec<VoiceEvent>,
}

impl Watch {
    fn new(rx: Receiver<VoiceEvent>) -> Self {
        Self { rx, seen: Vec::new() }
    }

    fn drain(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            self.seen.push(e);
        }
    }

    /// Waits up to `secs` for an event matching `f` (already-seen events count).
    async fn wait(&mut self, what: &str, secs: u64, f: impl Fn(&VoiceEvent) -> bool) {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            self.drain();
            if self.seen.iter().any(&f) {
                return;
            }
            if Instant::now() > end {
                panic!("timed out waiting for {what}; saw {:#?}", self.seen);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

fn level_of(events: &[VoiceEvent], identity: &str) -> f32 {
    events
        .iter()
        .filter_map(|e| if let VoiceEvent::Levels(l) = e { Some(l) } else { None })
        .flatten()
        .filter(|(id, _)| id == identity)
        .map(|(_, v)| *v)
        .fold(0.0, f32::max)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a LiveKit dev server on 127.0.0.1:7880"]
async fn two_engines_hear_each_other() {
    let room = format!("test-{}", std::process::id());
    let (a, a_rx) = VoiceEngine::new(tokio::runtime::Handle::current());
    let (b, b_rx) = VoiceEngine::new(tokio::runtime::Handle::current());
    let (mut wa, mut wb) = (Watch::new(a_rx), Watch::new(b_rx));

    // B listens; its own input is a quiet tone it keeps muted.
    b.connect(
        url(),
        token(&room, "2"),
        ConnectOptions {
            audio_input: AudioInput::Synthetic(SyntheticTone { freq_hz: 300.0, amplitude: 0.1 }),
            start_muted: true,
            ..Default::default()
        },
    );
    wb.wait("B connected", 20, |e| *e == VoiceEvent::Connected).await;
    assert!(wb.seen.contains(&VoiceEvent::Connecting));

    // A talks: a loud 440 Hz tone, voice-activity mode.
    a.set_mode(VoiceMode::VoiceActivity { threshold: 0.35 });
    a.connect(
        url(),
        token(&room, "1"),
        ConnectOptions {
            audio_input: AudioInput::Synthetic(SyntheticTone { freq_hz: 440.0, amplitude: 0.5 }),
            ..Default::default()
        },
    );
    wa.wait("A connected", 20, |e| *e == VoiceEvent::Connected).await;

    // B sees A join and hears its audio track.
    wb.wait("ParticipantJoined(1) on B", 20, |e| {
        matches!(e, VoiceEvent::ParticipantJoined { identity } if identity == "1")
    })
    .await;
    wb.wait("non-zero level for A on B", 20, |e| {
        matches!(e, VoiceEvent::Levels(l) if l.iter().any(|(id, v)| id == "1" && *v > 0.3))
    })
    .await;
    wb.wait("A speaking on B", 20, |e| {
        matches!(e, VoiceEvent::Speaking { identity, speaking: true } if identity == "1")
    })
    .await;

    // A sees B and its own meter and speaking state.
    wa.wait("ParticipantJoined(2) on A", 20, |e| {
        matches!(e, VoiceEvent::ParticipantJoined { identity } if identity == "2")
    })
    .await;
    wa.wait("local level on A", 20, |e| {
        matches!(e, VoiceEvent::Levels(l) if l.iter().any(|(id, v)| id == "1" && *v > 0.3))
    })
    .await;
    wa.wait("A speaking locally", 20, |e| {
        matches!(e, VoiceEvent::Speaking { identity, speaking: true } if identity == "1")
    })
    .await;
    assert!(level_of(&wa.seen, "1") > 0.3);

    // Levels are throttled: no more than ~15 batches per second.
    let before = wa.seen.iter().filter(|e| matches!(e, VoiceEvent::Levels(_))).count();
    tokio::time::sleep(Duration::from_secs(2)).await;
    wa.drain();
    let batches = wa.seen.iter().filter(|e| matches!(e, VoiceEvent::Levels(_))).count() - before;
    assert!((15..=36).contains(&batches), "{batches} level batches in 2 s");

    // Muting A is visible to B.
    a.set_mute(true);
    wb.wait("A's mic muted on B", 20, |e| {
        matches!(e, VoiceEvent::TrackMuted { identity, kind: MediaKind::Audio, muted: true } if identity == "1")
    })
    .await;
    a.set_mute(false);
    wb.wait("A's mic unmuted on B", 20, |e| {
        matches!(e, VoiceEvent::TrackMuted { identity, kind: MediaKind::Audio, muted: false } if identity == "1")
    })
    .await;

    // Push-to-talk with the key up silences the tone: A stops speaking.
    a.set_mode(VoiceMode::PushToTalk);
    wa.wait("A stops speaking (PTT up)", 20, |e| {
        matches!(e, VoiceEvent::Speaking { identity, speaking: false } if identity == "1")
    })
    .await;

    // Leaving.
    a.disconnect();
    wa.wait("A disconnected", 20, |e| matches!(e, VoiceEvent::Disconnected { .. })).await;
    wb.wait("ParticipantLeft(1) on B", 20, |e| {
        matches!(e, VoiceEvent::ParticipantLeft { identity } if identity == "1")
    })
    .await;
    b.disconnect();
    wb.wait("B disconnected", 20, |e| matches!(e, VoiceEvent::Disconnected { .. })).await;
    assert!(
        !wb.seen.iter().any(|e| matches!(e, VoiceEvent::Error(_))) && !wa.seen.iter().any(|e| matches!(e, VoiceEvent::Error(_))),
        "unexpected errors: A {:#?} B {:#?}",
        wa.seen.iter().filter(|e| matches!(e, VoiceEvent::Error(_))).collect::<Vec<_>>(),
        wb.seen.iter().filter(|e| matches!(e, VoiceEvent::Error(_))).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a LiveKit dev server on 127.0.0.1:7880"]
async fn bad_token_ends_in_disconnected() {
    let (e, rx) = VoiceEngine::new(tokio::runtime::Handle::current());
    let mut w = Watch::new(rx);
    e.connect(
        url(),
        "not-a-token",
        ConnectOptions { audio_input: AudioInput::Synthetic(SyntheticTone::default()), ..Default::default() },
    );
    w.wait("Disconnected", 30, |e| matches!(e, VoiceEvent::Disconnected { .. })).await;
    assert_eq!(w.seen.first(), Some(&VoiceEvent::Connecting));
    assert!(!w.seen.contains(&VoiceEvent::Connected));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a LiveKit dev server on 127.0.0.1:7880"]
async fn a_synthetic_camera_and_screen_reach_the_other_engine() {
    let room = format!("video-{}", std::process::id());
    let (a, a_rx) = VoiceEngine::new(tokio::runtime::Handle::current());
    let (b, b_rx) = VoiceEngine::new(tokio::runtime::Handle::current());
    let (mut wa, mut wb) = (Watch::new(a_rx), Watch::new(b_rx));
    let opts = |tone| ConnectOptions {
        audio_input: AudioInput::Synthetic(tone),
        video_input: VideoInput::Synthetic(SyntheticPicture { width: 320, height: 180, fps: 15 }),
        start_muted: true,
        ..Default::default()
    };
    b.connect(url(), token(&room, "2"), opts(SyntheticTone::default()));
    wb.wait("B connected", 20, |e| *e == VoiceEvent::Connected).await;
    a.connect(url(), token(&room, "1"), opts(SyntheticTone::default()));
    wa.wait("A connected", 20, |e| *e == VoiceEvent::Connected).await;

    // The camera: A gets a preview track of its own, B a track for A, and real frames on B.
    a.set_camera(true);
    wa.wait("A's own camera track", 20, |e| {
        matches!(e, VoiceEvent::VideoTrack { identity, source: VideoKind::Camera, .. } if identity == "1")
    })
    .await;
    wb.wait("A's camera on B", 30, |e| {
        matches!(e, VoiceEvent::VideoTrack { identity, source: VideoKind::Camera, .. } if identity == "1")
    })
    .await;
    let track_id = wb
        .seen
        .iter()
        .find_map(|e| match e {
            VoiceEvent::VideoTrack { identity, source: VideoKind::Camera, track_id } if identity == "1" => {
                Some(track_id.clone())
            }
            _ => None,
        })
        .unwrap();
    let sink = b.subscribe_video(&track_id);
    let end = Instant::now() + Duration::from_secs(30);
    let first = loop {
        if let Some(f) = sink.latest_frame() {
            break f;
        }
        assert!(Instant::now() < end, "no frame arrived on B");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(first.width >= 16 && first.height >= 16);
    assert_eq!(first.rgba.len(), (first.width * first.height * 4) as usize);
    assert!(first.rgba.chunks(4).any(|p| p[..3] != first.rgba[..3]), "not one flat colour");
    // The preview on A is a frame too.
    let (mine, _) = (a.subscribe_video(&track_id_of(&wa, VideoKind::Camera)), ());
    assert!(
        {
            let end = Instant::now() + Duration::from_secs(10);
            loop {
                if mine.latest_frame().is_some() {
                    break true;
                }
                if Instant::now() > end {
                    break false;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        },
        "A's preview has frames"
    );

    // A screen share is a second track.
    a.set_screen_share(Some(ScreenSource { id: 1, title: "Entire screen".into(), kind: ScreenKind::Screen }));
    wb.wait("A's screen on B", 30, |e| {
        matches!(e, VoiceEvent::VideoTrack { identity, source: VideoKind::Screen, .. } if identity == "1")
    })
    .await;

    // Stopping ends the tracks on both sides.
    a.set_camera(false);
    a.set_screen_share(None);
    wb.wait("A's camera ended on B", 30, |e| {
        matches!(e, VoiceEvent::VideoTrackEnded { track_id: t, .. } if *t == track_id)
    })
    .await;
    wa.wait("A's own camera ended", 20, |e| {
        matches!(e, VoiceEvent::VideoTrackEnded { identity, .. } if identity == "1")
    })
    .await;
}

fn track_id_of(w: &Watch, kind: VideoKind) -> String {
    w.seen
        .iter()
        .find_map(|e| match e {
            VoiceEvent::VideoTrack { identity, source, track_id } if identity == "1" && *source == kind => {
                Some(track_id.clone())
            }
            _ => None,
        })
        .expect("track announced")
}
