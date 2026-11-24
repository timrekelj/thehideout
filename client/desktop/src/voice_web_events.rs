//! The JS voice engine's events (JSON strings, see `js/voice.js`) as [`VoiceEvent`]s. Apart from
//! `voice_web.rs` so they can be tested natively.

use thehideout_voice_types::{AudioDevice, MediaKind, Quality, VideoKind, VoiceEvent};

/// One event as the JS side reports it.
pub fn parse(json: &str) -> Option<VoiceEvent> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    let devices = |k: &str| -> Vec<AudioDevice> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|d| Some(AudioDevice { id: d.get("id")?.as_str()?.to_owned(), name: d.get("name")?.as_str()?.to_owned() }))
                    // A browser without permission lists placeholders with no id: not devices.
                    .filter(|d| !d.id.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(match v.get("t")?.as_str()? {
        "connecting" => VoiceEvent::Connecting,
        "connected" => VoiceEvent::Connected,
        "reconnecting" => VoiceEvent::Reconnecting,
        "disconnected" => VoiceEvent::Disconnected { reason: text("reason").unwrap_or_default() },
        "participantJoined" => VoiceEvent::ParticipantJoined { identity: text("identity")? },
        "participantLeft" => VoiceEvent::ParticipantLeft { identity: text("identity")? },
        "speaking" => VoiceEvent::Speaking { identity: text("identity")?, speaking: v.get("speaking")?.as_bool()? },
        "trackMuted" => VoiceEvent::TrackMuted { identity: text("identity")?, kind: MediaKind::Audio, muted: v.get("muted")?.as_bool()? },
        "quality" => VoiceEvent::ConnectionQuality {
            identity: text("identity")?,
            quality: match text("quality")?.as_str() {
                "excellent" => Quality::Excellent,
                "good" => Quality::Good,
                "poor" => Quality::Poor,
                _ => Quality::Lost,
            },
        },
        "levels" => VoiceEvent::Levels(
            v.get("levels")?
                .as_array()?
                .iter()
                .filter_map(|p| Some((p.get(0)?.as_str()?.to_owned(), p.get(1)?.as_f64()? as f32)))
                .collect(),
        ),
        "devices" => VoiceEvent::Devices {
            inputs: devices("inputs"),
            outputs: devices("outputs"),
            default_input: text("defaultInput"),
            default_output: text("defaultOutput"),
        },
        "videoTrack" => VoiceEvent::VideoTrack {
            identity: text("identity")?,
            source: if text("source")? == "screen" { VideoKind::Screen } else { VideoKind::Camera },
            track_id: text("trackId")?,
        },
        "videoEnded" => VoiceEvent::VideoTrackEnded { identity: text("identity")?, track_id: text("trackId")? },
        "cameras" => VoiceEvent::Cameras { cameras: devices("cameras"), default: text("defaultCamera") },
        "error" => VoiceEvent::Error(text("message")?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> String {
        s.to_owned()
    }

    #[test]
    fn states() {
        assert_eq!(parse(r#"{"t":"connecting"}"#), Some(VoiceEvent::Connecting));
        assert_eq!(parse(r#"{"t":"connected"}"#), Some(VoiceEvent::Connected));
        assert_eq!(parse(r#"{"t":"reconnecting"}"#), Some(VoiceEvent::Reconnecting));
        assert_eq!(parse(r#"{"t":"disconnected","reason":"left"}"#), Some(VoiceEvent::Disconnected { reason: id("left") }));
        assert_eq!(parse(r#"{"t":"disconnected"}"#), Some(VoiceEvent::Disconnected { reason: String::new() }));
    }

    #[test]
    fn video() {
        assert_eq!(
            parse(r#"{"t":"videoTrack","identity":"7","source":"screen","trackId":"TR_1"}"#),
            Some(VoiceEvent::VideoTrack { identity: id("7"), source: VideoKind::Screen, track_id: id("TR_1") })
        );
        assert_eq!(
            parse(r#"{"t":"videoTrack","identity":"7","source":"camera","trackId":"TR_2"}"#),
            Some(VoiceEvent::VideoTrack { identity: id("7"), source: VideoKind::Camera, track_id: id("TR_2") })
        );
        assert_eq!(parse(r#"{"t":"videoEnded","identity":"7","trackId":"TR_2"}"#), Some(VoiceEvent::VideoTrackEnded { identity: id("7"), track_id: id("TR_2") }));
        assert_eq!(
            parse(r#"{"t":"cameras","cameras":[{"id":"c1","name":"Cam"}],"defaultCamera":"c1"}"#),
            Some(VoiceEvent::Cameras { cameras: vec![AudioDevice { id: id("c1"), name: id("Cam") }], default: Some(id("c1")) })
        );
    }

    #[test]
    fn placeholder_devices_without_an_id_are_dropped() {
        assert_eq!(
            parse(r#"{"t":"cameras","cameras":[{"id":"","name":""},{"id":"c1","name":"Cam"}],"defaultCamera":null}"#),
            Some(VoiceEvent::Cameras { cameras: vec![AudioDevice { id: id("c1"), name: id("Cam") }], default: None })
        );
        assert_eq!(parse(r#"{"t":"error","message":""}"#), Some(VoiceEvent::Error(String::new())));
    }

    #[test]
    fn participants() {
        assert_eq!(parse(r#"{"t":"participantJoined","identity":"7"}"#), Some(VoiceEvent::ParticipantJoined { identity: id("7") }));
        assert_eq!(parse(r#"{"t":"participantLeft","identity":"7"}"#), Some(VoiceEvent::ParticipantLeft { identity: id("7") }));
        assert_eq!(parse(r#"{"t":"speaking","identity":"7","speaking":true}"#), Some(VoiceEvent::Speaking { identity: id("7"), speaking: true }));
        assert_eq!(
            parse(r#"{"t":"trackMuted","identity":"7","muted":true}"#),
            Some(VoiceEvent::TrackMuted { identity: id("7"), kind: MediaKind::Audio, muted: true })
        );
    }

    #[test]
    fn quality() {
        let q = |name: &str| parse(&format!(r#"{{"t":"quality","identity":"7","quality":"{name}"}}"#));
        let of = |quality| Some(VoiceEvent::ConnectionQuality { identity: id("7"), quality });
        assert_eq!(q("excellent"), of(Quality::Excellent));
        assert_eq!(q("good"), of(Quality::Good));
        assert_eq!(q("poor"), of(Quality::Poor));
        assert_eq!(q("lost"), of(Quality::Lost));
        assert_eq!(q("anything else"), of(Quality::Lost));
    }

    #[test]
    fn levels_skip_bad_pairs() {
        assert_eq!(parse(r#"{"t":"levels","levels":[["1",0.5],["2",1],[3,0.1],["4"]]}"#), Some(VoiceEvent::Levels(vec![(id("1"), 0.5), (id("2"), 1.0)])));
        assert_eq!(parse(r#"{"t":"levels","levels":[]}"#), Some(VoiceEvent::Levels(vec![])));
    }

    #[test]
    fn devices() {
        let json = r#"{"t":"devices","inputs":[{"id":"a","name":"Mic"},{"id":"b"}],"outputs":[{"id":"c","name":"Out"}],"defaultInput":"a"}"#;
        assert_eq!(
            parse(json),
            Some(VoiceEvent::Devices {
                inputs: vec![AudioDevice { id: id("a"), name: id("Mic") }],
                outputs: vec![AudioDevice { id: id("c"), name: id("Out") }],
                default_input: Some(id("a")),
                default_output: None,
            })
        );
    }

    #[test]
    fn errors() {
        assert_eq!(parse(r#"{"t":"error","message":"No microphone was found."}"#), Some(VoiceEvent::Error(id("No microphone was found."))));
    }

    #[test]
    fn malformed_or_unknown_input_is_dropped() {
        for json in [
            "",
            "not json",
            "[]",
            "null",
            "{}",
            r#"{"t":5}"#,
            r#"{"t":"nope"}"#,
            r#"{"t":"participantJoined"}"#,
            r#"{"t":"participantJoined","identity":7}"#,
            r#"{"t":"speaking","identity":"7"}"#,
            r#"{"t":"speaking","identity":"7","speaking":"yes"}"#,
            r#"{"t":"trackMuted","identity":"7"}"#,
            r#"{"t":"quality","identity":"7"}"#,
            r#"{"t":"levels"}"#,
            r#"{"t":"levels","levels":"loud"}"#,
            r#"{"t":"error"}"#,
        ] {
            assert_eq!(parse(json), None, "{json}");
        }
    }
}
