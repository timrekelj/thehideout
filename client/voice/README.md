# thehideout-voice

The voice and video engine for the desktop app: a thin, testable layer over the
[LiveKit Rust SDK](https://github.com/livekit/rust-sdks) (`livekit` 0.12, libwebrtc 0.4).
Plain Rust types, no Rune. Nothing here blocks the UI.

The app gets `{ url, token }` from the server's `JoinVoice` RPC and passes them to
`connect`. One room is one voice channel; a participant's identity is our user id as a
decimal string.

## API

```rust
let (engine, events) = VoiceEngine::new(tokio_handle);   // events: std::sync::mpsc::Receiver

engine.connect(url, token, ConnectOptions::default());
engine.set_mute(true);               // others see TrackMuted
engine.set_deaf(true);               // stops all remote playback, implies mute
engine.set_mode(VoiceMode::PushToTalk);          // or VoiceActivity { threshold }
engine.set_ptt(true);                // key down / up
engine.set_input_device(Some(id));   // None = system default
engine.set_output_device(None);
engine.set_noise_suppression(true);
engine.set_echo_cancellation(true);
engine.set_user_volume("42", 0.0);   // see "What is not supported"
engine.set_camera(true);             // needs the `camera` feature
engine.set_screen_share(Some(source));
engine.list_devices();               // -> VoiceEvent::Devices
engine.list_screen_sources();        // -> VoiceEvent::ScreenSources
engine.start_mic_test();             // settings meter, works without a room
engine.disconnect();

let sink = engine.subscribe_video(&track_id);  // from VoiceEvent::VideoTrack
let frame = sink.latest_frame();               // Option<VideoFrame { width, height, rgba, seq }>
```

Every command is fire and forget. Results arrive as `VoiceEvent`s:
`Connecting`, `Connected`, `Reconnecting`, `Disconnected { reason }`, `ParticipantJoined` /
`ParticipantLeft`, `Speaking`, `Levels`, `TrackMuted`, `Devices`, `ScreenSources`,
`VideoTrack` / `VideoTrackEnded`, `ConnectionQuality`, `Error`.

Notes on the events:

- `Levels` is throttled to about 15 per second, carries 0..1 values (RMS in dBFS, -60 dB to
  0 dB) and includes every participant with an audio track plus the local mic. The local mic
  is under the local identity once connected and under `""` outside a room (mic test). When
  everything goes silent one all-zero batch is sent, then nothing until something is heard.
- `Speaking` for remote users comes from the LiveKit server's active-speaker detection; for
  the local user it comes from the mic level and respects the transmit gate (muted or
  push-to-talk released is never speaking).
- A voice-activity gate keeps transmitting for 300 ms after the last loud reading. If it
  gets no level data for a second it fails open, so a mic that cannot be metered is never
  silently muted.
- After `Disconnected`, treat everyone as gone: `Speaking(false)` / `ParticipantLeft` are not
  replayed. Video tracks do get a `VideoTrackEnded`.
- The local user's own camera and screen come back as `VideoTrack` with the local identity,
  for a preview.
- Mute is a real LiveKit mute (peers see `TrackMuted`). Push-to-talk and voice activity only
  enable or disable the track locally, so they never flicker the mute indicator.

### Video frames

`VideoSink::latest_frame()` returns the newest frame as tightly packed RGBA8. Only the newest
frame is kept, nothing queues, and the I420 to RGBA conversion happens when you ask (once per
new frame), so an unwatched track costs nothing. `seq` increases per picture so the UI can
skip a texture upload it has already done. Sinks are cheap to clone.

## Threading model

- `VoiceEngine::new(handle)` spawns one actor task on the tokio runtime you pass in. All
  state lives in that task; commands, room events and background results reach it as
  messages on one channel, so there are no locks in the engine.
- Command methods push a message and return. They never wait for the SDK.
- Events go out on an unbounded `std::sync::mpsc` channel. Drain it with `try_recv` in the
  poll loop (`examples/voice_cli.rs` does exactly that). Levels are throttled, so the
  channel stays small if drained a few times a second.
- A 20 ms tick in the actor reads the level meters, runs the PTT/VAD gate and the speaking
  detector, and batches levels.
- Room I/O, device switching, screen capture and the camera run on SDK threads, and for the
  two capturers on their own threads (their handles are not `Send`).
- `list_devices` and the first `connect` initialise the platform audio module inside the
  actor and can take on the order of 100 ms; the UI is unaffected.
- Dropping the `VoiceEngine` leaves the room and stops the actor.

## Platform audio, and which SDK path this crate uses

livekit 0.12 exposes libwebrtc's platform **audio device module (ADM)** (`PlatformAudio`),
including on macOS (CoreAudio). The crate uses it for capture **and** playback:

- device listing and switching (stable GUIDs, hot swap while running)
- WebRTC's software echo cancellation, noise suppression and AGC, all **on by default**
  (`set_noise_suppression` / `set_echo_cancellation` toggle them)
- remote audio plays through the selected output device with the AEC reference intact

No `cpal` capture or playback is involved for calls. `cpal` is used for one thing only: the
settings mic-test meter outside a room, because the ADM hands capture straight to WebRTC with
no tap (a sink on the local ADM track receives no frames). Inside a room the local level is
read from the published track's `audioLevel` stat (about 20 times a second).

`ConnectOptions::audio_input` can be `AudioInput::Synthetic(..)`, a generated tone that needs
no hardware or permission and never reaches the speakers. It exists for tests.
`ConnectOptions::video_input` can likewise be `VideoInput::Synthetic(SyntheticPicture { width, height, fps })`:
a generated, scrolling gradient (`synthetic_rgba`) that `set_camera(true)` and `set_screen_share(Some(_))`
publish instead of the camera and the desktop capturer (any `ScreenSource` will do). It needs no hardware,
permission or system picker, and exists for tests (`tests/livekit.rs` and the desktop's `voice_e2e`).

## Permissions

macOS needs microphone permission for capture, camera permission for the camera, and Screen
Recording permission for sharing. A bundled desktop app must declare, in its `Info.plist`:

```xml
<key>NSMicrophoneUsageDescription</key><string>The Hideout needs the microphone for voice chat.</string>
<key>NSCameraUsageDescription</key><string>The Hideout needs the camera for video.</string>
```

(Screen recording has no usage key; macOS prompts on first capture.) Unbundled, the terminal
that launches the binary is the one asked. Without microphone permission the ADM fails to
start recording and the engine reports an `Error`.

## Linking the desktop binary on macOS

libwebrtc's Objective-C categories only link in with `-ObjC`; without it the first
peer-connection factory aborts with `+[NSString stringForStdString:]: unrecognized selector`.
This crate's `build.rs` passes the flag for its own tests and examples, but cargo does not
forward it to dependants, so the **desktop binary needs it too**, e.g. a
`client/desktop/build.rs` with
`println!("cargo:rustc-link-arg-bins=-ObjC")` (macOS only), or
`rustflags = ["-C", "link-arg=-ObjC"]` in `.cargo/config.toml`. This crate does not touch the
desktop crate.

## What is and is not supported (livekit 0.12)

| Area | Status |
| --- | --- |
| Audio devices (list, switch, default) | Supported through the ADM. Ids are ADM GUIDs; `"default"` is the system default entry. |
| Echo cancellation, noise suppression, AGC | Supported, on by default (WebRTC software APM through the ADM). AGC has no switch. |
| Mic level, VAD, push-to-talk, mute, deafen | Supported. Local level in a room comes from track stats; outside a room from the `cpal` mic-test meter. |
| Screen and window share | Supported through libwebrtc's `DesktopCapturer`. On macOS the "display" entry is the system picker (ScreenCaptureKit): the user picks the display when sharing starts. Windows are listed by title and captured directly. 30 fps, cursor included. No system-audio capture. |
| Camera | **The SDK has no camera capturer.** Implemented with `nokhwa` (AVFoundation) behind the `camera` cargo feature, default off. Without the feature `set_camera(true)` emits an `Error`. Always the default camera, highest frame rate format. Verified to compile, not run against hardware. |
| `set_user_volume` | **Only mute.** The SDK mixes remote audio inside WebRTC and has no per-track gain, so `volume <= 0.0` silences that user and any other value plays at full volume. Real per-user gain needs the ADM playout replaced by our own mixer (`NativeAudioStream` plus `cpal`), at the cost of the AEC reference. |
| `set_input_gain` | **Synthetic input only.** The ADM path has no gain control in this SDK (`SetMicrophoneVolume` is not bound); AGC is on instead. The value is stored and ignored for the real mic. |
| Video codecs | LiveKit defaults (VP8, simulcast for the camera). |
| Push-to-talk and the voice hotkeys while the app is **not** focused | **Not supported (known limitation).** The desktop app handles its hotkeys (mute, deafen, push to talk) as window key events, so they work only while its window is focused; the Voice & audio page says so. Rune does have global shortcuts (`App::global_shortcut`, `AppHandle::register_shortcut`: Carbon `RegisterEventHotKey` on macOS, `RegisterHotKey` on Windows, X11 grabs, the Wayland portal), but they report a *press only*: there is no key-up event, and hold-to-talk needs the release. Mute and deafen toggles could use them; push to talk cannot. The alternative is an OS-level keyboard hook (`CGEventTap`, `SetWindowsHookEx`, evdev), which needs Accessibility / Input Monitoring permission or raw device access and is deliberately not added. The window losing focus releases the key, so a held key never leaves the mic open. |

## Tests

```text
cargo test -p thehideout-voice                 # unit tests: levels, throttle, gating, I420 to RGBA, state machine
cargo test -p thehideout-voice -- --ignored    # end to end, needs a LiveKit dev server
```

The ignored tests (`tests/livekit.rs`) mint tokens with `livekit-api`, connect two engines to
a dev server and check join, audio track, non-zero levels, `Speaking`, mute, push-to-talk and
leave, using a synthetic tone. Start the server with:

```text
docker run --rm -d -p 127.0.0.1:7880:7880 -p 127.0.0.1:7881:7881 -p 127.0.0.1:7882:7882/udp \
    livekit/livekit-server --dev --bind 0.0.0.0
```

Keys are `devkey` / `secret`. Set `LIVEKIT_URL` (default `ws://127.0.0.1:7880`),
`LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` to point at another server.

`cargo run -p thehideout-voice --example voice_cli -- <ws-url> <token>` joins with the real
microphone and prints every event (keys: `m` mute, `d` deafen, `p` push-to-talk, `l` devices,
`q` quit).

## Build notes

The first build downloads a prebuilt libwebrtc (via `webrtc-sys-build`) and compiles the
LiveKit stack; expect a couple of minutes cold. Later builds are incremental. The toolchain is
the repo's pinned stable one (`client/rust-toolchain.toml`).
