//! Process-wide shutdown signal.
//!
//! Long-lived work (gateway streams, background sweepers) selects on it so a
//! graceful drain can finish: axum/hyper wait for open responses, and a
//! Subscribe stream never ends on its own.

use std::sync::Arc;

use tokio::sync::watch;

#[derive(Clone)]
pub struct Shutdown {
    tx: Arc<watch::Sender<bool>>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self { tx: Arc::new(watch::channel(false).0) }
    }
}

impl Shutdown {
    /// Starts the shutdown; idempotent.
    pub fn trigger(&self) {
        self.tx.send_replace(true);
    }

    pub fn is_triggered(&self) -> bool {
        *self.tx.borrow()
    }

    /// Resolves once shutdown has been triggered (immediately if it already was).
    pub async fn triggered(&self) {
        let mut rx = self.tx.subscribe();
        // Err = the sender is gone, which can only mean the process is ending.
        let _ = rx.wait_for(|v| *v).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn triggered_resolves_after_trigger_and_for_late_waiters() {
        let s = Shutdown::default();
        assert!(!s.is_triggered());
        assert!(tokio::time::timeout(Duration::from_millis(30), s.triggered()).await.is_err());
        let waiter = tokio::spawn({
            let s = s.clone();
            async move { s.triggered().await }
        });
        s.trigger();
        tokio::time::timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap();
        assert!(s.is_triggered());
        tokio::time::timeout(Duration::from_millis(100), s.triggered()).await.unwrap();
    }
}
