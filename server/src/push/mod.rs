//! Push notifications (APNs for iOS, FCM for Android).
//!
//! A background task ([`dispatch`]) listens to the event bus's tap, works out
//! who should be notified of each new message (and of being added to a guild)
//! from the notification settings and live gateway state, and hands the pushes
//! to a [`Transport`]. Nothing here runs on the message path: the bus only
//! clones the message into a broadcast channel.
//!
//! Foreground rule: a user with a live gateway stream that is not marked
//! backgrounded ([`Presence`]) is not pushed; they see the message live.
//! Streams count as foreground until the app says otherwise with
//! `NotificationService.SetClientState(false)`.
//!
//! There is no per-channel read state on the server, so pushes carry no badge
//! count.

pub mod apns;
pub mod config;
pub mod dispatch;
pub mod fcm;
pub mod jwt;
pub mod payload;
pub mod settings;
pub mod store;
#[cfg(test)]
pub mod testkeys;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Semaphore;

pub use payload::Notification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ios" => Some(Platform::Ios),
            "android" => Some(Platform::Android),
            _ => None,
        }
    }
}

/// A registered push destination.
#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    pub id: u64,
    pub user_id: u64,
    /// Hash of the session token that registered it.
    pub session_hash: String,
    pub platform: Platform,
    pub token: String,
    pub locale: Option<String>,
    /// iOS: send through the APNs sandbox.
    pub sandbox: bool,
}

impl std::fmt::Debug for Device {
    /// Never prints the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device").field("id", &self.id).field("user_id", &self.user_id).field("platform", &self.platform).finish_non_exhaustive()
    }
}

/// What a provider answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// The token is dead (unregistered, bad): delete the device.
    InvalidToken,
    /// Try again later (429, 5xx, network); the duration is the provider's `Retry-After`.
    Retry(Option<Duration>, String),
    /// Will not work if repeated; logged.
    Failed(String),
}

/// Delivers a notification to one device. Implemented by the real providers
/// ([`Providers`]) and by the recording fake of the tests.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Whether devices of this platform can be served (the provider is configured).
    fn supports(&self, _platform: Platform) -> bool {
        true
    }

    async fn send(&self, device: &Device, notification: &Notification) -> Outcome;
}

/// APNs and FCM; either may be missing.
pub struct Providers {
    pub apns: Option<apns::Apns>,
    pub fcm: Option<fcm::Fcm>,
}

impl Providers {
    /// `None` when neither provider is configured.
    pub fn from_config(config: &config::PushConfig) -> anyhow::Result<Option<Self>> {
        if config.apns.is_none() && config.fcm.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            apns: config.apns.as_ref().map(apns::Apns::new).transpose()?,
            fcm: config.fcm.as_ref().map(fcm::Fcm::new).transpose()?,
        }))
    }
}

#[async_trait]
impl Transport for Providers {
    fn supports(&self, platform: Platform) -> bool {
        match platform {
            Platform::Ios => self.apns.is_some(),
            Platform::Android => self.fcm.is_some(),
        }
    }

    async fn send(&self, device: &Device, notification: &Notification) -> Outcome {
        match (device.platform, &self.apns, &self.fcm) {
            (Platform::Ios, Some(apns), _) => apns.send(device, notification).await,
            (Platform::Android, _, Some(fcm)) => fcm.send(device, notification).await,
            _ => Outcome::Failed("provider not configured".into()),
        }
    }
}

/// Tuning of the dispatcher.
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    /// At most one push per channel and recipient per window; later ones are
    /// merged into one that follows when the window ends.
    pub coalesce_window: Duration,
    /// First retry delay after a 429 / 5xx; doubles per attempt.
    pub retry_backoff: Duration,
    /// Sends in flight at once.
    pub max_concurrent_sends: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { coalesce_window: Duration::from_secs(5), retry_backoff: Duration::from_millis(500), max_concurrent_sends: 32 }
    }
}

/// Push state shared through [`crate::AppState`].
pub struct Pusher {
    /// `None`: push is off (tokens are still stored).
    pub transport: Option<Arc<dyn Transport>>,
    pub presence: Arc<Presence>,
    pub tuning: Tuning,
    pub(crate) sends: Semaphore,
    pub(crate) coalescer: Coalescer,
}

impl Pusher {
    pub fn new(transport: Option<Arc<dyn Transport>>, tuning: Tuning) -> Self {
        Self {
            transport,
            presence: Arc::default(),
            tuning,
            sends: Semaphore::new(tuning.max_concurrent_sends.max(1)),
            coalescer: Coalescer::new(tuning.coalesce_window),
        }
    }

    pub fn disabled() -> Self {
        Self::new(None, Tuning::default())
    }
}

#[derive(Default)]
struct SessionStreams {
    open: usize,
    foreground: bool,
}

/// Which users have live gateway streams in the foreground.
#[derive(Default)]
pub struct Presence {
    users: Mutex<HashMap<u64, HashMap<String, SessionStreams>>>,
}

/// Keeps a stream counted as live; dropped with the stream.
pub struct PresenceGuard {
    presence: Arc<Presence>,
    user_id: u64,
    session: String,
}

impl Presence {
    /// Registers an open stream of `session`. A session's first stream starts
    /// in the foreground; later ones keep the state the session reported.
    pub fn open(self: &Arc<Self>, user_id: u64, session: &str) -> PresenceGuard {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let entry = users.entry(user_id).or_default().entry(session.to_owned()).or_default();
        if entry.open == 0 {
            entry.foreground = true;
        }
        entry.open += 1;
        PresenceGuard { presence: self.clone(), user_id, session: session.to_owned() }
    }

    /// Records whether the app of `session` is in the foreground (ignored while it has no stream).
    pub fn set_foreground(&self, user_id: u64, session: &str, foreground: bool) {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = users.get_mut(&user_id).and_then(|s| s.get_mut(session)) {
            entry.foreground = foreground;
        }
    }

    /// Whether the user has a live stream in the foreground.
    pub fn is_active(&self, user_id: u64) -> bool {
        let users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        users.get(&user_id).is_some_and(|sessions| sessions.values().any(|s| s.open > 0 && s.foreground))
    }
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        let mut users = self.presence.users.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sessions) = users.get_mut(&self.user_id) {
            if let Some(entry) = sessions.get_mut(&self.session) {
                entry.open = entry.open.saturating_sub(1);
                if entry.open == 0 {
                    sessions.remove(&self.session);
                }
            }
            if sessions.is_empty() {
                users.remove(&self.user_id);
            }
        }
    }
}

/// A push waiting for its window to end.
pub(crate) struct Pending {
    pub notification: Notification,
    pub devices: Vec<Device>,
}

struct Slot {
    last: Option<Instant>,
    pending: Option<Pending>,
}

/// What [`Coalescer::admit`] tells the caller to do.
pub(crate) enum Admit {
    /// Send this now.
    Now(Pending),
    /// Held: call [`Coalescer::take`] after the delay.
    Later(Duration),
    /// Merged into a held push whose timer is already running.
    Merged,
}

/// At most one push per (user, channel) per window; the rest collapse into one
/// trailing push carrying the latest message.
pub(crate) struct Coalescer {
    window: Duration,
    slots: Mutex<HashMap<(u64, u64), Slot>>,
}

const MAX_SLOTS: usize = 10_000;

impl Coalescer {
    pub fn new(window: Duration) -> Self {
        Self { window, slots: Mutex::default() }
    }

    pub fn admit(&self, key: (u64, u64), mut pending: Pending) -> Admit {
        let now = Instant::now();
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if slots.len() >= MAX_SLOTS {
            slots.retain(|_, s| s.pending.is_some() || s.last.is_some_and(|t| now.duration_since(t) < self.window));
        }
        let slot = slots.entry(key).or_insert(Slot { last: None, pending: None });
        if let Some(held) = slot.pending.take() {
            // A held mention stays a mention (it picks the louder Android channel).
            if held.notification.kind == payload::Kind::Mention {
                pending.notification.kind = payload::Kind::Mention;
            }
            slot.pending = Some(pending);
            return Admit::Merged;
        }
        match slot.last.map(|t| now.duration_since(t)) {
            Some(elapsed) if elapsed < self.window => {
                slot.pending = Some(pending);
                Admit::Later(self.window - elapsed)
            }
            _ => {
                slot.last = Some(now);
                Admit::Now(pending)
            }
        }
    }

    /// The held push of `key`, now due.
    pub fn take(&self, key: (u64, u64)) -> Option<Pending> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = slots.get_mut(&key)?;
        let pending = slot.pending.take();
        if pending.is_some() {
            slot.last = Some(Instant::now());
        }
        pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_counts_foreground_streams_only() {
        let presence = Arc::new(Presence::default());
        assert!(!presence.is_active(1));
        let a = presence.open(1, "s1");
        assert!(presence.is_active(1));
        presence.set_foreground(1, "s1", false);
        assert!(!presence.is_active(1));
        // A second session in the foreground makes the user active again.
        let b = presence.open(1, "s2");
        assert!(presence.is_active(1));
        drop(b);
        assert!(!presence.is_active(1));
        presence.set_foreground(1, "s1", true);
        assert!(presence.is_active(1));
        drop(a);
        assert!(!presence.is_active(1));
        assert!(presence.users.lock().unwrap().is_empty());
        // State for a session without a stream is ignored.
        presence.set_foreground(1, "s1", false);
        let _c = presence.open(1, "s1");
        assert!(presence.is_active(1));
    }

    fn pending(body: &str, kind: payload::Kind) -> Pending {
        Pending { notification: Notification { kind, title: "t".into(), body: body.into(), guild_id: 1, channel_id: 2, message_id: 3 }, devices: vec![] }
    }

    #[test]
    fn coalescer_sends_the_first_and_holds_the_rest_as_one() {
        let c = Coalescer::new(Duration::from_secs(60));
        assert!(matches!(c.admit((1, 2), pending("a", payload::Kind::Message)), Admit::Now(_)));
        assert!(matches!(c.admit((1, 2), pending("b", payload::Kind::Mention)), Admit::Later(_)));
        assert!(matches!(c.admit((1, 2), pending("c", payload::Kind::Message)), Admit::Merged));
        // Other channels and users are independent.
        assert!(matches!(c.admit((1, 3), pending("x", payload::Kind::Message)), Admit::Now(_)));
        assert!(matches!(c.admit((9, 2), pending("x", payload::Kind::Message)), Admit::Now(_)));
        let held = c.take((1, 2)).unwrap();
        assert_eq!(held.notification.body, "c", "the latest message wins");
        assert_eq!(held.notification.kind, payload::Kind::Mention, "a held mention stays a mention");
        assert!(c.take((1, 2)).is_none());
    }

    #[test]
    fn coalescer_admits_again_after_the_window() {
        let c = Coalescer::new(Duration::from_millis(20));
        assert!(matches!(c.admit((1, 2), pending("a", payload::Kind::Message)), Admit::Now(_)));
        std::thread::sleep(Duration::from_millis(40));
        assert!(matches!(c.admit((1, 2), pending("b", payload::Kind::Message)), Admit::Now(_)));
    }
}
