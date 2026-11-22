//! Slider: a horizontal 0..1 control. A 4pt track with an accent fill and a 16pt thumb, inside a
//! 28pt box that takes the focus ring and the pointer. The track is inset by the thumb's radius,
//! so the thumb stays inside the box at both ends and the box lines up with the text column.
//!
//! The control only draws. The caller attaches what moves it to the built element:
//! `on_drag` (a pointer position, turned into a value by [`slider_fraction`]) and `on_step`
//! (arrow keys and assistive technology; Home and End with `on_step_limit`).

use rune::{El, el, row};
use rune_kit::Kit;

use crate::theme_of;
use crate::tokens::{radius, space};

/// The thumb's size.
pub const THUMB: f32 = 16.0;
/// The box's height, and so the pointer's target.
const HEIGHT: f32 = 28.0;
const TRACK: f32 = 4.0;

pub struct Slider {
    frac: f32,
    label: String,
    value_text: Option<String>,
    width: Option<f32>,
}

impl Slider {
    /// A slider at `frac` (0..1), named `label` for assistive technology.
    pub fn new(frac: f32, label: impl Into<String>) -> Slider {
        Slider {
            frac: frac.clamp(0.0, 1.0),
            label: label.into(),
            value_text: None,
            width: None,
        }
    }
    /// The value as words for assistive technology ("35 percent"); by default the number.
    pub fn value_text(mut self, text: impl Into<String>) -> Self {
        self.value_text = Some(text.into());
        self
    }
    /// A fixed width; by default the slider takes its parent's.
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let pad = THUMB / 2.0;
        let thumb = el()
            .key("slider-thumb")
            .pin(None, Some((TRACK - THUMB) / 2.0), Some(-pad), None)
            .size(THUMB, THUMB)
            .radius(radius::FULL)
            .fill(t.accent.color())
            .border(2.0, t.surface.color());
        let track = row()
            .key("slider-track")
            .w_full()
            .h(TRACK)
            .radius(radius::FULL)
            .fill(t.border_strong.color())
            .child(
                row()
                    .key("slider-fill")
                    .w_frac(self.frac)
                    .h(TRACK)
                    .radius(radius::FULL)
                    .fill(t.accent.color())
                    .child(thumb),
            );
        let mut root = row()
            .role("slider")
            .label(self.label)
            .range_value(f64::from(self.frac * 100.0), 0.0, 100.0)
            .range_step(1.0)
            .h(HEIGHT)
            .px(pad)
            .center_y()
            .radius(radius::MD)
            .focusable(true)
            .focus_ring(space::RING_WIDTH, space::RING_OFFSET, k.c.border_focus)
            .child(track);
        root = match self.width {
            Some(w) => root.w(w),
            None => root.w_full(),
        };
        if let Some(text) = self.value_text {
            root = root.a11y_value(text);
        }
        root
    }
}

/// Where a drag at `x` of a box `w` wide (as `Drag::x` and `Drag::w` give them) puts the thumb:
/// 0..1 along the track, which is inset by the thumb's radius at both ends.
pub fn slider_fraction(x: f32, w: f32) -> f32 {
    let usable = w - THUMB;
    if usable <= 0.0 {
        return 0.0;
    }
    ((x - THUMB / 2.0) / usable).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fraction_runs_along_the_inset_track_and_clamps() {
        // A 116pt box has a 100pt track starting 8pt in.
        assert_eq!(slider_fraction(8.0, 116.0), 0.0);
        assert_eq!(slider_fraction(58.0, 116.0), 0.5);
        assert_eq!(slider_fraction(108.0, 116.0), 1.0);
        assert_eq!(slider_fraction(-30.0, 116.0), 0.0, "a drag past the end holds the end");
        assert_eq!(slider_fraction(900.0, 116.0), 1.0);
        assert_eq!(slider_fraction(5.0, 10.0), 0.0, "a box narrower than the thumb has no track");
    }
}
