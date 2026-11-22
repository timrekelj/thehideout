//! Video capture threads: screens and windows through libwebrtc's desktop capturer, and the
//! camera through nokhwa (behind the `camera` feature, since the SDK has no camera capturer).
//!
//! Both capturers are not `Send`, so each lives on its own thread and is created there.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use livekit::webrtc::desktop_capturer::{
    CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::{I420Buffer, VideoFrame as RtcFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;

use crate::video::{Pending, VideoCell};
use crate::{ScreenKind, ScreenSource, SyntheticPicture};

const SCREEN_FPS: u64 = 30;

/// Called when a capture thread ends by itself: `Some(reason)` on failure.
pub(crate) type OnEnd = Arc<dyn Fn(Option<String>) + Send + Sync>;

/// A running capture. Dropping it asks the thread to stop (it does not wait).
pub(crate) struct Capture {
    stop: Arc<AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Lists the screens and windows that can be shared. On macOS the display entry is the
/// system picker: the user chooses the display when sharing starts.
pub(crate) fn list_screen_sources() -> Result<Vec<ScreenSource>, String> {
    let mut out = Vec::new();
    for (kind, st) in [
        (ScreenKind::Screen, DesktopCaptureSourceType::Screen),
        (ScreenKind::Window, DesktopCaptureSourceType::Window),
    ] {
        let Some(cap) = DesktopCapturer::new(options(st)) else {
            if kind == ScreenKind::Screen {
                return Err(
                    "screen capture is not available (is Screen Recording permission granted?)"
                        .into(),
                );
            }
            continue;
        };
        for s in cap.get_source_list() {
            let title = match (kind, s.title().is_empty()) {
                (ScreenKind::Screen, true) => "Entire screen".to_string(),
                _ => s.title(),
            };
            out.push(ScreenSource {
                id: s.id(),
                title,
                kind,
            });
        }
    }
    Ok(out)
}

fn options(st: DesktopCaptureSourceType) -> DesktopCapturerOptions {
    #[allow(unused_mut)]
    let mut o = DesktopCapturerOptions::new(st);
    o.set_include_cursor(true);
    // Windows are listed and captured directly; displays go through the system picker.
    #[cfg(target_os = "macos")]
    o.set_sck_system_picker(st != DesktopCaptureSourceType::Window);
    o
}

/// Pushes BGRA desktop frames (and, for the local preview, the same picture) at ~30 fps until
/// dropped.
pub(crate) fn start_screen(
    source: ScreenSource,
    video: NativeVideoSource,
    preview: Arc<VideoCell>,
    on_end: OnEnd,
) -> Capture {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("voice-screen-capture".into())
        .spawn(move || {
            let st = match source.kind {
                ScreenKind::Screen => DesktopCaptureSourceType::Screen,
                ScreenKind::Window => DesktopCaptureSourceType::Window,
            };
            let Some(mut cap) = DesktopCapturer::new(options(st)) else {
                return on_end(Some("could not start the screen capturer".into()));
            };
            let chosen = match source.kind {
                ScreenKind::Screen => None, // the system picker decides
                ScreenKind::Window => match cap
                    .get_source_list()
                    .into_iter()
                    .find(|s| s.id() == source.id)
                {
                    Some(s) => Some(s),
                    None => return on_end(Some(format!("window {:?} is gone", source.title))),
                },
            };
            let failed = Arc::new(AtomicBool::new(false));
            let failed_cb = failed.clone();
            cap.start_capture(chosen, move |res| match res {
                Ok(f) => {
                    let (w, h) = (f.width() as u32, f.height() as u32);
                    if w == 0 || h == 0 {
                        return;
                    }
                    let mut i420 = I420Buffer::new(w, h);
                    let (sy, su, sv) = i420.strides();
                    let (dy, du, dv) = i420.data_mut();
                    // libwebrtc desktop frames are BGRA, which libyuv calls ARGB.
                    yuv_helper::argb_to_i420(
                        f.data(),
                        f.stride(),
                        dy,
                        sy,
                        du,
                        su,
                        dv,
                        sv,
                        w as i32,
                        h as i32,
                    );
                    video.capture_frame(&RtcFrame::new(VideoRotation::VideoRotation0, &i420));
                    preview.put(Pending::Buffer(Box::new(i420)));
                }
                Err(CaptureError::Temporary) => {}
                Err(CaptureError::Permanent) => failed_cb.store(true, Ordering::Relaxed),
            });
            let frame_time = Duration::from_millis(1000 / SCREEN_FPS);
            while !flag.load(Ordering::Relaxed) {
                if failed.load(Ordering::Relaxed) {
                    return on_end(Some(
                        "screen capture stopped (cancelled, or permission denied)".into(),
                    ));
                }
                cap.capture_frame();
                std::thread::sleep(frame_time);
            }
        })
        .expect("spawn screen capture thread");
    Capture { stop }
}

/// Feeds a generated picture (see [`crate::synthetic_rgba`]) to the track and the preview, as the
/// camera or a screen would. For tests: no hardware, no permission.
pub(crate) fn start_synthetic(
    pic: SyntheticPicture,
    video: NativeVideoSource,
    preview: Arc<VideoCell>,
) -> Capture {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("voice-synthetic-video".into())
        .spawn(move || {
            let (w, h) = (pic.width.max(2) & !1, pic.height.max(2) & !1);
            let frame_time = Duration::from_millis(1000 / u64::from(pic.fps.max(1)));
            let mut n = 0u64;
            while !flag.load(Ordering::Relaxed) {
                let rgba: Arc<[u8]> = crate::synthetic_rgba(w, h, n).into();
                let mut i420 = I420Buffer::new(w, h);
                let (sy, su, sv) = i420.strides();
                let (dy, du, dv) = i420.data_mut();
                yuv_helper::abgr_to_i420(&rgba, w * 4, dy, sy, du, su, dv, sv, w as i32, h as i32);
                video.capture_frame(&RtcFrame::new(VideoRotation::VideoRotation0, &i420));
                preview.put(Pending::Rgba { width: w, height: h, rgba });
                n += 1;
                std::thread::sleep(frame_time);
            }
        })
        .expect("spawn synthetic video thread");
    Capture { stop }
}

/// Starts the default camera. Without the `camera` feature this is an error.
#[cfg(not(feature = "camera"))]
pub(crate) fn start_camera(
    _: NativeVideoSource,
    _: Arc<VideoCell>,
    _: OnEnd,
) -> Result<Capture, String> {
    Err("camera unsupported: the LiveKit SDK has no camera capturer; build thehideout-voice with the `camera` feature".into())
}

#[cfg(feature = "camera")]
pub(crate) fn start_camera(
    video: NativeVideoSource,
    preview: Arc<VideoCell>,
    on_end: OnEnd,
) -> Result<Capture, String> {
    use nokhwa::pixel_format::RgbAFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};

    // macOS asks for permission once; wait for the answer so the first start can succeed.
    if !nokhwa::nokhwa_check() {
        nokhwa::nokhwa_initialize(|_| {});
        for _ in 0..100 {
            if nokhwa::nokhwa_check() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !nokhwa::nokhwa_check() {
            return Err("camera permission was not granted".into());
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("voice-camera-capture".into())
        .spawn(move || {
            let format =
                RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
            let mut cam = match nokhwa::Camera::new(CameraIndex::Index(0), format) {
                Ok(c) => c,
                Err(e) => return on_end(Some(format!("camera: {e}"))),
            };
            if let Err(e) = cam.open_stream() {
                return on_end(Some(format!("camera: {e}")));
            }
            while !flag.load(Ordering::Relaxed) {
                let frame = match cam.frame() {
                    Ok(f) => f,
                    Err(e) => return on_end(Some(format!("camera: {e}"))),
                };
                let img = match frame.decode_image::<RgbAFormat>() {
                    Ok(i) => i,
                    Err(e) => return on_end(Some(format!("camera: {e}"))),
                };
                let (w, h) = (img.width(), img.height());
                let rgba: Arc<[u8]> = img.into_raw().into();
                let mut i420 = I420Buffer::new(w, h);
                let (sy, su, sv) = i420.strides();
                let (dy, du, dv) = i420.data_mut();
                // memory order R, G, B, A is libyuv's ABGR
                yuv_helper::abgr_to_i420(&rgba, w * 4, dy, sy, du, su, dv, sv, w as i32, h as i32);
                video.capture_frame(&RtcFrame::new(VideoRotation::VideoRotation0, &i420));
                preview.put(Pending::Rgba {
                    width: w,
                    height: h,
                    rgba,
                });
            }
            let _ = cam.stop_stream();
        })
        .map_err(|e| e.to_string())?;
    Ok(Capture { stop })
}
