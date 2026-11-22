//! LevelMeter: a horizontal bar for a live level (the microphone in settings). A rounded track
//! with an accent fill from the left, and optionally a marker where a threshold sits (the
//! voice-activity gate), drawn a little taller than the track so it reads as a line across it.
//!
//! The level is drawn as given: smoothing (fast rise, slower fall) is the caller's, since the
//! caller is the one who hears the readings.

use rune::{El, el, row};
use rune_kit::Kit;

use crate::theme_of;
use crate::tokens::radius;

const TRACK: f32 = 8.0;
/// The marker pokes out this far above and below the track.
const MARKER_OVERHANG: f32 = 3.0;
const MARKER_W: f32 = 2.0;

pub struct LevelMeter {
    level: f32,
    threshold: Option<f32>,
    label: String,
}

impl LevelMeter {
    /// A meter at `level` (0..1), named `label` for assistive technology.
    pub fn new(level: f32, label: impl Into<String>) -> LevelMeter {
        LevelMeter {
            level: level.clamp(0.0, 1.0),
            threshold: None,
            label: label.into(),
        }
    }
    /// A marker at `threshold` (0..1).
    pub fn threshold(mut self, threshold: Option<f32>) -> Self {
        self.threshold = threshold.map(|t| t.clamp(0.0, 1.0));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut track = row()
            .key("meter-track")
            .w_full()
            .h(TRACK)
            .radius(radius::FULL)
            .fill(t.border_strong.color())
            .child(
                el().key("meter-fill")
                    .w_frac(self.level)
                    .h(TRACK)
                    .radius(radius::FULL)
                    .fill(t.accent.color()),
            );
        if let Some(th) = self.threshold {
            track = track.child(
                row()
                    .key("meter-marker")
                    .pin(Some(0.0), Some(-MARKER_OVERHANG), None, Some(-MARKER_OVERHANG))
                    .w_frac(th)
                    .child(
                        el().pin(None, Some(0.0), Some(-MARKER_W / 2.0), Some(0.0))
                            .w(MARKER_W)
                            .radius(radius::FULL)
                            .fill(t.text.color()),
                    ),
            );
        }
        // The overhang is part of the box, so the marker is not clipped by a neighbour.
        row()
            .key("meter")
            .role("meter")
            .label(self.label)
            .range_value(f64::from(self.level * 100.0), 0.0, 100.0)
            .w_full()
            .h(TRACK + 2.0 * MARKER_OVERHANG)
            .center_y()
            .child(track)
    }
}
