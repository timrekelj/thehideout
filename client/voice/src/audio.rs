//! The pure parts of the audio path: level maths, throttling, push-to-talk / voice-activity
//! gating and speaking detection. Nothing here touches a device or the network, so it is all
//! unit tested. The engine feeds these from its 20 ms tick.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::VoiceMode;

/// Level meters run from silence (0.0) at -60 dBFS to full scale (1.0) at 0 dBFS.
pub const LEVEL_FLOOR_DB: f32 = -60.0;
/// Levels are delivered to the UI at about this rate.
pub const LEVELS_HZ: u32 = 15;
/// How long the voice-activity gate stays open after the last loud frame, so word endings
/// are not clipped.
pub const VAD_HANGOVER: Duration = Duration::from_millis(300);
/// If a voice-activity gate gets no level data for this long it fails open, so a capture
/// path that cannot be metered never leaves the user silently muted.
pub const VAD_STALE: Duration = Duration::from_millis(1000);
/// The level above which the local user counts as speaking when push-to-talk is used.
pub const SPEAKING_LEVEL: f32 = 0.3;
const SPEAKING_HANGOVER: Duration = Duration::from_millis(250);

/// Maps 16-bit PCM to a 0..1 meter value: RMS in dBFS, -60 dB .. 0 dB.
pub fn level_of(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * (rms as f32).log10();
    ((db - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB).clamp(0.0, 1.0)
}

/// Maps a linear 0..1 amplitude (WebRTC's `audioLevel` stat) onto the same meter scale.
pub fn level_from_linear(amplitude: f64) -> f32 {
    if amplitude <= 0.0 {
        return 0.0;
    }
    ((20.0 * (amplitude as f32).log10() - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB).clamp(0.0, 1.0)
}

/// A level shared between an audio callback thread (writer) and the engine tick (reader).
/// Keeps the peak since the last read; `None` means no frame arrived in between.
#[derive(Default, Debug)]
pub struct LevelCell(AtomicU32);

impl LevelCell {
    const SCALE: f32 = 1_000_000.0;

    pub fn record(&self, level: f32) {
        // Stored as value + 1 so that 0 can mean "no data".
        let v = (level.clamp(0.0, 1.0) * Self::SCALE) as u32 + 1;
        self.0.fetch_max(v, Ordering::Relaxed);
    }

    pub fn take(&self) -> Option<f32> {
        match self.0.swap(0, Ordering::Relaxed) {
            0 => None,
            v => Some((v - 1) as f32 / Self::SCALE),
        }
    }
}

/// Collects the latest level per identity and releases them in batches at a fixed rate.
#[derive(Debug)]
pub struct LevelBatcher {
    interval: Duration,
    last_flush: Option<Instant>,
    pending: HashMap<String, f32>,
    last_was_silent: bool,
}

impl LevelBatcher {
    pub fn new(hz: u32) -> Self {
        Self {
            interval: Duration::from_secs_f64(1.0 / hz.max(1) as f64),
            last_flush: None,
            pending: HashMap::new(),
            last_was_silent: false,
        }
    }

    /// Keeps the loudest value seen for `identity` since the last flush.
    pub fn push(&mut self, identity: &str, level: f32) {
        let slot = self.pending.entry(identity.to_string()).or_insert(0.0);
        *slot = slot.max(level);
    }

    /// The batch to send, sorted by identity, once the interval has passed since the last one.
    /// An all-silent batch is sent once (so meters drop to zero) and then suppressed until
    /// something is heard again. Identities that were not pushed since the last flush are gone.
    pub fn flush(&mut self, now: Instant) -> Option<Vec<(String, f32)>> {
        if let Some(last) = self.last_flush
            && now.duration_since(last) < self.interval
        {
            return None;
        }
        if self.pending.is_empty() {
            return None;
        }
        self.last_flush = Some(now);
        let mut batch: Vec<(String, f32)> = self.pending.drain().collect();
        batch.sort_by(|a, b| a.0.cmp(&b.0));
        let silent = batch.iter().all(|(_, l)| *l <= 0.0);
        if silent && self.last_was_silent {
            return None;
        }
        self.last_was_silent = silent;
        Some(batch)
    }
}

/// Decides whether the microphone should be sending right now.
#[derive(Debug, Clone)]
pub struct TxGate {
    mode: VoiceMode,
    ptt: bool,
    muted: bool,
    last_loud: Option<Instant>,
    last_fed: Option<Instant>,
}

impl TxGate {
    pub fn new(mode: VoiceMode) -> Self {
        Self {
            mode,
            ptt: false,
            muted: false,
            last_loud: None,
            last_fed: None,
        }
    }

    pub fn set_mode(&mut self, mode: VoiceMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> VoiceMode {
        self.mode
    }

    pub fn set_ptt(&mut self, pressed: bool) {
        self.ptt = pressed;
    }

    /// Muted (by the user or by deafening) closes the gate regardless of the mode.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Feeds one mic level reading.
    pub fn feed(&mut self, level: f32, now: Instant) {
        self.last_fed = Some(now);
        if let VoiceMode::VoiceActivity { threshold } = self.mode
            && level >= threshold
        {
            self.last_loud = Some(now);
        }
    }

    /// Whether audio should flow at `now`.
    pub fn transmitting(&self, now: Instant) -> bool {
        if self.muted {
            return false;
        }
        match self.mode {
            VoiceMode::PushToTalk => self.ptt,
            VoiceMode::VoiceActivity { .. } => {
                let fresh = self
                    .last_fed
                    .is_some_and(|t| now.duration_since(t) <= VAD_STALE);
                if !fresh {
                    return true;
                }
                self.last_loud
                    .is_some_and(|t| now.duration_since(t) <= VAD_HANGOVER)
            }
        }
    }
}

/// Turns the local mic level into `Speaking` transitions, with a short hang time.
#[derive(Debug, Default)]
pub struct SpeakingDetector {
    speaking: bool,
    last_loud: Option<Instant>,
}

impl SpeakingDetector {
    /// Returns `Some(new_state)` when the speaking state changes.
    pub fn update(
        &mut self,
        level: f32,
        threshold: f32,
        transmitting: bool,
        now: Instant,
    ) -> Option<bool> {
        if transmitting && level >= threshold {
            self.last_loud = Some(now);
        }
        let speaking = transmitting
            && self
                .last_loud
                .is_some_and(|t| now.duration_since(t) <= SPEAKING_HANGOVER);
        if speaking != self.speaking {
            self.speaking = speaking;
            Some(speaking)
        } else {
            None
        }
    }
}

/// The level above which `mode` considers the local user to be speaking.
pub fn speaking_threshold(mode: VoiceMode) -> f32 {
    match mode {
        VoiceMode::VoiceActivity { threshold } => threshold,
        VoiceMode::PushToTalk => SPEAKING_LEVEL,
    }
}

/// One 10 ms mono frame of a sine wave, continuing from `phase` (radians). Used by the
/// synthetic input.
pub fn sine_frame(out: &mut [i16], sample_rate: u32, freq: f32, amplitude: f32, phase: &mut f32) {
    let step = std::f32::consts::TAU * freq / sample_rate as f32;
    for s in out.iter_mut() {
        *s = (phase.sin() * amplitude * i16::MAX as f32) as i16;
        *phase += step;
        if *phase > std::f32::consts::TAU {
            *phase -= std::f32::consts::TAU;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn level_maps_silence_and_full_scale() {
        assert_eq!(level_of(&[]), 0.0);
        assert_eq!(level_of(&[0; 480]), 0.0);
        let full: Vec<i16> = (0..480)
            .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
            .collect();
        assert!(level_of(&full) > 0.99);
        // about -30 dBFS lands mid-scale
        let half_scale = (32768.0 * 10f32.powf(-30.0 / 20.0)) as i16;
        let mid = level_of(&vec![half_scale; 480]);
        assert!((mid - 0.5).abs() < 0.02, "{mid}");
        // below the floor stays at zero
        assert_eq!(level_of(&[1; 480]), 0.0);
    }

    #[test]
    fn linear_amplitude_maps_like_pcm() {
        assert_eq!(level_from_linear(0.0), 0.0);
        assert!((level_from_linear(1.0) - 1.0).abs() < 1e-6);
        let a = level_from_linear(10f64.powf(-30.0 / 20.0));
        assert!((a - 0.5).abs() < 0.01, "{a}");
        assert_eq!(level_from_linear(1e-5), 0.0);
    }

    #[test]
    fn sine_has_the_expected_level() {
        let mut phase = 0.0;
        let mut frame = [0i16; 480];
        sine_frame(&mut frame, 48_000, 440.0, 0.5, &mut phase);
        // amplitude 0.5 -> RMS 0.354 -> about -9 dBFS -> 0.85
        let l = level_of(&frame);
        assert!((l - 0.85).abs() < 0.03, "{l}");
    }

    #[test]
    fn level_cell_keeps_the_peak_and_reports_no_data() {
        let c = LevelCell::default();
        assert_eq!(c.take(), None);
        c.record(0.2);
        c.record(0.7);
        c.record(0.4);
        assert!((c.take().unwrap() - 0.7).abs() < 1e-5);
        assert_eq!(c.take(), None);
        c.record(0.0);
        assert_eq!(c.take(), Some(0.0));
    }

    #[test]
    fn batcher_throttles_to_the_rate_and_keeps_the_peak() {
        let t0 = Instant::now();
        let mut b = LevelBatcher::new(15); // 66.6 ms
        b.push("2", 0.1);
        b.push("1", 0.5);
        b.push("1", 0.3);
        let first = b.flush(t0).unwrap();
        assert_eq!(first, vec![("1".to_string(), 0.5), ("2".to_string(), 0.1)]);
        b.push("1", 0.9);
        assert!(b.flush(ms(t0, 30)).is_none(), "too soon");
        // the pending value survives until the interval has passed
        assert_eq!(b.flush(ms(t0, 70)).unwrap(), vec![("1".to_string(), 0.9)]);
        // ~15 batches per second when pushed every 10 ms
        let mut n = 0;
        let mut t = ms(t0, 70);
        for _ in 0..100 {
            t += Duration::from_millis(10);
            b.push("1", 0.5);
            if b.flush(t).is_some() {
                n += 1;
            }
        }
        assert!((13..=16).contains(&n), "{n} batches in 1 s");
    }

    #[test]
    fn batcher_sends_one_silent_batch_then_stays_quiet() {
        let t0 = Instant::now();
        let mut b = LevelBatcher::new(15);
        b.push("1", 0.5);
        assert!(b.flush(t0).is_some());
        b.push("1", 0.0);
        assert_eq!(b.flush(ms(t0, 100)).unwrap(), vec![("1".to_string(), 0.0)]);
        b.push("1", 0.0);
        assert!(b.flush(ms(t0, 200)).is_none());
        b.push("1", 0.2);
        assert!(b.flush(ms(t0, 300)).is_some());
        assert!(b.flush(ms(t0, 400)).is_none(), "nothing pending");
    }

    #[test]
    fn push_to_talk_follows_the_key_and_ignores_level() {
        let t0 = Instant::now();
        let mut g = TxGate::new(VoiceMode::PushToTalk);
        g.feed(1.0, t0);
        assert!(!g.transmitting(t0));
        g.set_ptt(true);
        g.feed(0.0, ms(t0, 10));
        assert!(g.transmitting(ms(t0, 10)));
        g.set_ptt(false);
        assert!(!g.transmitting(ms(t0, 20)));
    }

    #[test]
    fn mute_closes_every_mode() {
        let t0 = Instant::now();
        let mut g = TxGate::new(VoiceMode::PushToTalk);
        g.set_ptt(true);
        g.set_muted(true);
        assert!(!g.transmitting(t0));
        let mut g = TxGate::new(VoiceMode::VoiceActivity { threshold: 0.3 });
        g.feed(0.9, t0);
        assert!(g.transmitting(t0));
        g.set_muted(true);
        assert!(!g.transmitting(t0));
        g.set_muted(false);
        assert!(g.transmitting(t0));
    }

    #[test]
    fn voice_activity_opens_on_loud_and_holds_then_closes() {
        let t0 = Instant::now();
        let mut g = TxGate::new(VoiceMode::VoiceActivity { threshold: 0.4 });
        g.feed(0.1, t0);
        assert!(!g.transmitting(t0), "quiet room");
        g.feed(0.39, ms(t0, 20));
        assert!(!g.transmitting(ms(t0, 20)), "just under the threshold");
        g.feed(0.4, ms(t0, 40));
        assert!(g.transmitting(ms(t0, 40)));
        g.feed(0.0, ms(t0, 200));
        assert!(g.transmitting(ms(t0, 200)), "inside the hangover");
        g.feed(0.0, ms(t0, 400));
        assert!(!g.transmitting(ms(t0, 400)), "hangover over");
    }

    #[test]
    fn voice_activity_fails_open_without_level_data() {
        let t0 = Instant::now();
        let mut g = TxGate::new(VoiceMode::VoiceActivity { threshold: 0.4 });
        assert!(g.transmitting(t0), "never fed");
        g.feed(0.0, t0);
        assert!(!g.transmitting(ms(t0, 100)));
        assert!(
            g.transmitting(ms(t0, 1500)),
            "meter went quiet for over a second"
        );
    }

    #[test]
    fn mode_switch_takes_effect() {
        let t0 = Instant::now();
        let mut g = TxGate::new(VoiceMode::VoiceActivity { threshold: 0.4 });
        g.feed(0.9, t0);
        assert!(g.transmitting(t0));
        g.set_mode(VoiceMode::PushToTalk);
        assert!(!g.transmitting(t0));
    }

    #[test]
    fn speaking_detector_has_a_hang_time() {
        let t0 = Instant::now();
        let mut d = SpeakingDetector::default();
        assert_eq!(d.update(0.1, 0.3, true, t0), None);
        assert_eq!(d.update(0.6, 0.3, true, ms(t0, 20)), Some(true));
        assert_eq!(d.update(0.6, 0.3, true, ms(t0, 40)), None);
        assert_eq!(d.update(0.0, 0.3, true, ms(t0, 200)), None, "still hanging");
        assert_eq!(d.update(0.0, 0.3, true, ms(t0, 400)), Some(false));
        // not transmitting (muted, PTT up) is never speaking
        assert_eq!(d.update(0.9, 0.3, false, ms(t0, 500)), None);
    }

    #[test]
    fn speaking_threshold_follows_the_mode() {
        assert_eq!(
            speaking_threshold(VoiceMode::VoiceActivity { threshold: 0.7 }),
            0.7
        );
        assert_eq!(speaking_threshold(VoiceMode::PushToTalk), SPEAKING_LEVEL);
    }
}
