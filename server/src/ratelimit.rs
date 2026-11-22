//! In-memory sliding-window rate limiter for unauthenticated and sensitive RPCs.
//!
//! Single-process by design (the app is one server with an in-process event
//! bus). Each key keeps the instants of its recent hits; a hard-limited hit is
//! rejected once `max` hits fall inside `window`. Keys are attacker-influenced
//! (usernames, peer IPs), so key length is capped and the map is bounded: when
//! it grows past [`MAX_KEYS`], expired keys are dropped and, if still over the
//! cap, arbitrary keys are evicted in O(n) without sorting.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use connectrpc::ConnectError;

/// Upper bound on tracked keys, on the length of a single key, and on stored
/// hits per key (hard caps are far below this; the cap only keeps the
/// per-request `retain` scan bounded).
const MAX_KEYS: usize = 100_000;
const MAX_KEY_LEN: usize = 256;
const MAX_HITS_PER_KEY: usize = 1024;

/// Hits of one key, remembered with the window they were recorded under so the
/// over-cap sweep can expire every key with its own window.
struct KeyHits {
    window: Duration,
    hits: Vec<Instant>,
}

pub struct RateLimiter {
    hits: Mutex<HashMap<String, KeyHits>>,
    /// Token buckets: `(tokens, last refill)` per key.
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
    max_keys: usize,
    /// Messages one user may send (or edit) in a burst, and per second after that.
    message_burst: u32,
    message_refill: f64,
}

/// Production message rate: a burst of 10, then one per second.
pub const MESSAGE_BURST: u32 = 10;
pub const MESSAGE_REFILL_PER_SEC: f64 = 1.0;

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            hits: Mutex::new(HashMap::new()),
            buckets: Mutex::new(HashMap::new()),
            max_keys: MAX_KEYS,
            message_burst: MESSAGE_BURST,
            message_refill: MESSAGE_REFILL_PER_SEC,
        }
    }

    /// Overrides the message rate (a burst, then `refill_per_sec`); tests that
    /// send many messages quickly raise it.
    pub fn with_message_rate(mut self, burst: u32, refill_per_sec: f64) -> Self {
        self.message_burst = burst;
        self.message_refill = refill_per_sec;
        self
    }

    /// Counts one message send or edit of `user_id`; `ResourceExhausted` with
    /// a `retry-after` header when over the rate.
    pub fn check_message(&self, user_id: u64) -> Result<(), ConnectError> {
        self.bucket(&format!("message:{user_id}"), self.message_burst, self.message_refill)
    }

    #[cfg(test)]
    fn with_max_keys(max_keys: usize) -> Self {
        Self {
            hits: Mutex::new(HashMap::new()),
            buckets: Mutex::new(HashMap::new()),
            max_keys,
            message_burst: MESSAGE_BURST,
            message_refill: MESSAGE_REFILL_PER_SEC,
        }
    }

    /// Token bucket: `key` may burst `capacity` hits, then `refill_per_sec` per
    /// second. A rejected hit costs nothing and the error carries a
    /// `retry-after` header (whole seconds, at least 1) and says how long to wait.
    pub fn bucket(&self, key: &str, capacity: u32, refill_per_sec: f64) -> Result<(), ConnectError> {
        let key: String = key.chars().take(MAX_KEY_LEN).collect();
        let capacity = f64::from(capacity);
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > self.max_keys {
            // Full buckets carry no information.
            buckets.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * refill_per_sec < capacity);
        }
        let entry = buckets.entry(key).or_insert((capacity, now));
        entry.0 = (entry.0 + now.duration_since(entry.1).as_secs_f64() * refill_per_sec).min(capacity);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            return Ok(());
        }
        let wait = ((1.0 - entry.0) / refill_per_sec).ceil().max(1.0) as u64;
        let mut headers = http::HeaderMap::new();
        headers.insert("retry-after", http::HeaderValue::from(wait));
        Err(ConnectError::resource_exhausted(format!("slow down: try again in {wait}s")).with_headers(headers))
    }

    /// Records a hit for `key` and returns how many hits the key has within
    /// `window` (including this one). Returns `Err` when that count exceeds the
    /// hard `max`.
    pub fn hit(&self, key: &str, max: usize, window: Duration) -> Result<usize, ConnectError> {
        let count = self.record(key, window);
        if count > max {
            return Err(ConnectError::resource_exhausted(
                "too many attempts, try again later",
            ));
        }
        Ok(count)
    }

    /// Records a hit for `key` and returns how many hits the key has within
    /// `window` (including this one). Never rejects; for soft throttling by delay.
    pub fn record(&self, key: &str, window: Duration) -> usize {
        // Bound memory per key: attacker-chosen strings (e.g. a login username)
        // must not become arbitrarily long keys.
        let truncated;
        let key = if key.len() > MAX_KEY_LEN {
            truncated = key.chars().take(MAX_KEY_LEN).collect::<String>();
            &truncated
        } else {
            key
        };

        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let count = {
            let entry = hits.entry(key.to_owned()).or_insert(KeyHits {
                window,
                hits: Vec::new(),
            });
            entry.window = window;
            entry.hits.retain(|t| now.duration_since(*t) < window);
            entry.hits.push(now);
            if entry.hits.len() > MAX_HITS_PER_KEY {
                let excess = entry.hits.len() - MAX_HITS_PER_KEY;
                entry.hits.drain(..excess);
            }
            entry.hits.len()
        };
        if hits.len() > self.max_keys {
            // Expire every key with its own window first; only if the map is still
            // over the cap, evict the least-recently-active keys, one O(n) pass per
            // eviction (no sorting, no cloning the whole map under the lock).
            hits.retain(|_, e| {
                let window = e.window;
                e.hits.retain(|t| now.duration_since(*t) < window);
                !e.hits.is_empty()
            });
            let mut excess = hits.len().saturating_sub(self.max_keys);
            while excess > 0 {
                let oldest = hits
                    .iter()
                    .min_by_key(|(_, e)| e.hits.last().copied())
                    .map(|(k, _)| k.clone());
                match oldest {
                    Some(k) => {
                        hits.remove(&k);
                        excess -= 1;
                    }
                    None => break,
                }
            }
        }
        count
    }

    /// Records a hit for `key`; returns `Err` when the key already has `max`
    /// hits within `window`.
    pub fn check(&self, key: &str, max: usize, window: Duration) -> Result<(), ConnectError> {
        self.hit(key, max, window).map(|_| ())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_then_rejects_within_window() {
        let rl = RateLimiter::new();
        for _ in 0..3 {
            assert!(rl.check("k", 3, Duration::from_secs(60)).is_ok());
        }
        let err = rl.check("k", 3, Duration::from_secs(60)).unwrap_err();
        assert_eq!(err.code, connectrpc::ErrorCode::ResourceExhausted);
    }

    #[test]
    fn hit_returns_the_count_in_window() {
        let rl = RateLimiter::new();
        assert_eq!(rl.hit("k", 10, Duration::from_secs(60)).unwrap(), 1);
        assert_eq!(rl.hit("k", 10, Duration::from_secs(60)).unwrap(), 2);
    }

    #[test]
    fn keys_are_independent_and_expire() {
        let rl = RateLimiter::new();
        assert!(rl.check("a", 1, Duration::from_millis(50)).is_ok());
        assert!(rl.check("a", 1, Duration::from_millis(50)).is_err());
        assert!(rl.check("b", 1, Duration::from_millis(50)).is_ok());
        std::thread::sleep(Duration::from_millis(60));
        assert!(rl.check("a", 1, Duration::from_millis(50)).is_ok());
    }

    #[test]
    fn bucket_allows_a_burst_then_refills() {
        let rl = RateLimiter::new();
        for _ in 0..3 {
            assert!(rl.bucket("u", 3, 20.0).is_ok());
        }
        let err = rl.bucket("u", 3, 20.0).unwrap_err();
        assert_eq!(err.code, connectrpc::ErrorCode::ResourceExhausted);
        assert_eq!(err.response_headers().get("retry-after").unwrap(), "1");
        assert!(rl.bucket("other", 3, 20.0).is_ok());
        std::thread::sleep(Duration::from_millis(80));
        assert!(rl.bucket("u", 3, 20.0).is_ok());
    }

    #[test]
    fn overlong_keys_are_truncated() {
        let rl = RateLimiter::new();
        let long = "x".repeat(10_000);
        assert!(rl.check(&long, 1, Duration::from_secs(60)).is_ok());
        // Same prefix -> same truncated key, so the second hit is rejected.
        let mut long2 = "x".repeat(10_000);
        long2.push('y');
        assert!(rl.check(&long2, 1, Duration::from_secs(60)).is_err());
    }

    #[test]
    fn over_cap_sweep_expires_stale_keys_with_their_own_window() {
        let rl = RateLimiter::with_max_keys(3);
        let short = Duration::from_millis(50);
        rl.record("a", short);
        rl.record("b", short);
        rl.record("c", short);
        std::thread::sleep(Duration::from_millis(60));
        // Triggers the sweep: a, b, c are expired under their own window and dropped.
        rl.record("d", Duration::from_secs(60));
        assert_eq!(rl.hits.lock().unwrap().len(), 1);
        // Fill past the cap with live keys: the map stays bounded at the cap.
        rl.record("e", Duration::from_secs(60));
        rl.record("f", Duration::from_secs(60));
        rl.record("g", Duration::from_secs(60));
        assert_eq!(rl.hits.lock().unwrap().len(), 3);
    }
}
