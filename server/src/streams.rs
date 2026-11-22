//! Caps on concurrent gateway Subscribe streams.
//!
//! Each stream holds a connection, broadcast receivers and a task for as long
//! as the client stays, so one user (or one leaky client) must not open them
//! without bound.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// `THEHIDEOUT_MAX_STREAMS_PER_USER` default.
pub const DEFAULT_MAX_PER_USER: usize = 8;
/// `THEHIDEOUT_MAX_STREAMS` default.
pub const DEFAULT_MAX_TOTAL: usize = 5000;

/// Why a stream was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The user already has the maximum number of streams open.
    User,
    /// The server as a whole is at its stream limit.
    Server,
}

#[derive(Default)]
struct Counts {
    per_user: HashMap<u64, usize>,
    total: usize,
}

pub struct StreamGate {
    max_per_user: usize,
    max_total: usize,
    counts: Mutex<Counts>,
}

impl Default for StreamGate {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PER_USER, DEFAULT_MAX_TOTAL)
    }
}

impl StreamGate {
    pub fn new(max_per_user: usize, max_total: usize) -> Self {
        Self {
            max_per_user,
            max_total,
            counts: Mutex::default(),
        }
    }

    /// Reserves a slot for `user_id`; released when the permit is dropped.
    pub fn acquire(self: &Arc<Self>, user_id: u64) -> Result<StreamPermit, Denied> {
        let mut counts = self.counts.lock().unwrap();
        if counts.total >= self.max_total {
            return Err(Denied::Server);
        }
        let open = counts.per_user.entry(user_id).or_insert(0);
        if *open >= self.max_per_user {
            return Err(Denied::User);
        }
        *open += 1;
        counts.total += 1;
        Ok(StreamPermit {
            gate: self.clone(),
            user_id,
        })
    }

    /// Streams open right now.
    pub fn open(&self) -> usize {
        self.counts.lock().unwrap().total
    }
}

/// A reserved stream slot.
pub struct StreamPermit {
    gate: Arc<StreamGate>,
    user_id: u64,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        let mut counts = self.gate.counts.lock().unwrap();
        counts.total = counts.total.saturating_sub(1);
        if let Some(open) = counts.per_user.get_mut(&self.user_id) {
            *open -= 1;
            if *open == 0 {
                counts.per_user.remove(&self.user_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_per_user_and_in_total_and_releases_on_drop() {
        let gate = Arc::new(StreamGate::new(2, 3));
        let a1 = gate.acquire(1).unwrap();
        let _a2 = gate.acquire(1).unwrap();
        assert_eq!(gate.acquire(1).err(), Some(Denied::User));
        let _b1 = gate.acquire(2).unwrap();
        // Three open: the server is full for everyone.
        assert_eq!(gate.acquire(3).err(), Some(Denied::Server));
        assert_eq!(gate.open(), 3);
        drop(a1);
        assert_eq!(gate.open(), 2);
        let _a3 = gate.acquire(1).unwrap();
        assert_eq!(gate.acquire(1).err(), Some(Denied::Server));
    }

    #[test]
    fn bookkeeping_returns_to_empty() {
        let gate = Arc::new(StreamGate::new(8, 100));
        let permits: Vec<_> = (0..8).map(|_| gate.acquire(7).unwrap()).collect();
        assert_eq!(gate.acquire(7).err(), Some(Denied::User));
        drop(permits);
        assert_eq!(gate.open(), 0);
        assert!(gate.counts.lock().unwrap().per_user.is_empty());
    }
}
