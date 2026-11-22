//! Video frames for the UI. The engine keeps only the newest frame per track; conversion to
//! RGBA happens when the UI asks, not per received frame, so an unwatched track costs nothing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::{BoxVideoBuffer, VideoBuffer as _};

/// One decoded picture, tightly packed RGBA8 (`width * height * 4` bytes, no row padding).
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    /// Increases with every new picture on the track, so the UI can skip a texture upload
    /// when it has already seen this one.
    pub seq: u64,
}

pub(crate) enum Pending {
    /// A buffer straight from the SDK (decoded remote video).
    Buffer(BoxVideoBuffer),
    /// Already RGBA (local camera preview).
    #[allow(dead_code)]
    Rgba {
        width: u32,
        height: u32,
        rgba: Arc<[u8]>,
    },
}

#[derive(Default)]
pub(crate) struct Slot {
    seq: u64,
    pending: Option<Pending>,
    converted: Option<VideoFrame>,
}

/// The newest frame of one track, shared between the receiving task and the UI.
#[derive(Default)]
pub(crate) struct VideoCell(Mutex<Slot>);

impl VideoCell {
    /// Replaces the stored frame. Never queues.
    pub(crate) fn put(&self, frame: Pending) {
        let mut s = self.0.lock().unwrap();
        s.seq += 1;
        s.pending = Some(frame);
    }

    fn latest(&self) -> Option<VideoFrame> {
        let mut s = self.0.lock().unwrap();
        if let Some(p) = s.pending.take() {
            let seq = s.seq;
            s.converted = Some(match p {
                Pending::Buffer(buf) => {
                    let i420 = buf.to_i420();
                    let (w, h) = (i420.width(), i420.height());
                    let (y, u, v) = i420.data();
                    let (sy, su, sv) = i420.strides();
                    VideoFrame {
                        width: w,
                        height: h,
                        rgba: i420_to_rgba(w, h, y, sy, u, su, v, sv).into(),
                        seq,
                    }
                }
                Pending::Rgba {
                    width,
                    height,
                    rgba,
                } => VideoFrame {
                    width,
                    height,
                    rgba,
                    seq,
                },
            });
        }
        s.converted.clone()
    }
}

/// A handle on one video track's newest frame. Cheap to clone; all clones see the same frame.
#[derive(Clone)]
pub struct VideoSink {
    pub(crate) cell: Arc<VideoCell>,
}

impl VideoSink {
    /// The newest frame as RGBA8, or `None` before the first one arrives. Calling again
    /// without a new frame returns the same `Arc` (same `seq`) without redoing the conversion.
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        self.cell.latest()
    }
}

/// Track id (the LiveKit track sid) to its cell.
#[derive(Default)]
pub(crate) struct Registry(Mutex<HashMap<String, Arc<VideoCell>>>);

impl Registry {
    pub(crate) fn cell(&self, track_id: &str) -> Arc<VideoCell> {
        self.0
            .lock()
            .unwrap()
            .entry(track_id.to_string())
            .or_default()
            .clone()
    }

    pub(crate) fn insert(&self, track_id: &str, cell: Arc<VideoCell>) {
        self.0.lock().unwrap().insert(track_id.to_string(), cell);
    }

    pub(crate) fn remove(&self, track_id: &str) {
        self.0.lock().unwrap().remove(track_id);
    }
}

/// I420 planes to tightly packed RGBA8 (BT.601 limited range, what libwebrtc produces).
#[allow(clippy::too_many_arguments)]
pub fn i420_to_rgba(
    width: u32,
    height: u32,
    y: &[u8],
    stride_y: u32,
    u: &[u8],
    stride_u: u32,
    v: &[u8],
    stride_v: u32,
) -> Vec<u8> {
    let stride = width * 4;
    let mut dst = vec![0u8; (stride * height) as usize];
    // libyuv's "ABGR" is R, G, B, A in memory.
    yuv_helper::i420_to_abgr(
        y,
        stride_y,
        u,
        stride_u,
        v,
        stride_v,
        &mut dst,
        stride,
        width as i32,
        height as i32,
    );
    dst
}

/// The synthetic test picture of frame number `n`: a horizontal colour gradient that scrolls
/// right as `n` grows, with the lower half a vertical one. RGBA8, opaque.
pub fn synthetic_rgba(width: u32, height: u32, n: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    let shift = (n * 7 % 256) as u32;
    for y in 0..height {
        for x in 0..width {
            let u = (x * 255 / width.max(1) + shift) % 256;
            let v = y * 255 / height.max(1);
            let (r, g, b) = if y < height / 2 { (u, 255 - u, 128) } else { (v, u, 255 - v) };
            out.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_frames_change_with_the_number() {
        let a = synthetic_rgba(16, 8, 0);
        let b = synthetic_rgba(16, 8, 1);
        assert_eq!(a.len(), 16 * 8 * 4);
        assert_ne!(a, b);
        assert!(a.chunks(4).all(|p| p[3] == 255));
    }

    /// A flat-colour 4x2 I420 picture.
    fn flat(y: u8, u: u8, v: u8) -> Vec<u8> {
        let (w, h) = (4u32, 2u32);
        let yp = vec![y; (w * h) as usize];
        let up = vec![u; (w / 2 * h / 2) as usize];
        let vp = vec![v; (w / 2 * h / 2) as usize];
        i420_to_rgba(w, h, &yp, w, &up, w / 2, &vp, w / 2)
    }

    fn close(px: &[u8], want: [u8; 4]) -> bool {
        px.iter()
            .zip(want)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 3)
    }

    #[test]
    fn i420_converts_to_rgba_byte_order() {
        let white = flat(235, 128, 128);
        assert_eq!(white.len(), 4 * 2 * 4);
        assert!(
            white.chunks(4).all(|p| close(p, [255, 255, 255, 255])),
            "{white:?}"
        );
        let black = flat(16, 128, 128);
        assert!(
            black.chunks(4).all(|p| close(p, [0, 0, 0, 255])),
            "{black:?}"
        );
        // pure red: R high, G and B low (proves RGBA, not BGRA)
        let red = flat(81, 90, 240);
        assert!(red.chunks(4).all(|p| close(p, [255, 0, 0, 255])), "{red:?}");
        let blue = flat(41, 240, 110);
        assert!(
            blue.chunks(4).all(|p| close(p, [0, 0, 255, 255])),
            "{blue:?}"
        );
    }

    #[test]
    fn only_the_newest_frame_is_kept() {
        let cell = Arc::new(VideoCell::default());
        let sink = VideoSink { cell: cell.clone() };
        assert!(sink.latest_frame().is_none());
        for i in 0..5u8 {
            cell.put(Pending::Rgba {
                width: 1,
                height: 1,
                rgba: vec![i, 0, 0, 255].into(),
            });
        }
        let f = sink.latest_frame().unwrap();
        assert_eq!(f.seq, 5);
        assert_eq!(&f.rgba[..], &[4, 0, 0, 255]);
        // asking again returns the same picture, not a re-conversion
        let again = sink.latest_frame().unwrap();
        assert!(Arc::ptr_eq(&f.rgba, &again.rgba));
        cell.put(Pending::Rgba {
            width: 1,
            height: 1,
            rgba: vec![9, 0, 0, 255].into(),
        });
        assert_eq!(sink.latest_frame().unwrap().seq, 6);
    }

    #[test]
    fn registry_shares_cells_by_track_id() {
        let r = Registry::default();
        let a = r.cell("TR_1");
        let b = r.cell("TR_1");
        assert!(Arc::ptr_eq(&a, &b));
        r.remove("TR_1");
        assert!(!Arc::ptr_eq(&a, &r.cell("TR_1")));
    }
}
