//! Self-reconnecting event stream. The reconnect policy lives in [`crate::state`];
//! this module only performs the I/O it asks for.

use std::sync::Arc;
use std::time::Duration;

use crate::client::{Client, EventStream};
use crate::error::ApiError;
use crate::runtime::{self, Task, sleep, timeout};
use crate::state::{Action, Failure, Input, Machine};
use crate::types::Event;

/// What the gateway reports to the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayUpdate {
    /// Opening a stream (first attempt or a retry).
    Connecting,
    /// The stream is open. Events missed while disconnected are not replayed,
    /// so refetch whatever the UI shows.
    Connected,
    /// A real-time event (heartbeats included).
    Event(Event),
    /// The connection failed or went silent; the next attempt is in `retry_in`.
    Disconnected { retry_in: Duration, reason: String },
    /// The server rejected the token. The gateway has stopped for good: sign in
    /// again and spawn a new one.
    AuthExpired,
}

/// Reconnect and liveness tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct GatewayConfig {
    /// Delay before the first retry; doubles on each consecutive failure.
    pub initial_backoff: Duration,
    /// Upper bound of the retry delay (before jitter).
    pub max_backoff: Duration,
    /// The server's heartbeat interval (30 s by default).
    pub heartbeat_interval: Duration,
    /// The connection is declared dead after `heartbeat_interval * silence_factor`
    /// without any event.
    pub silence_factor: f64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(30),
            silence_factor: 2.5,
        }
    }
}

impl GatewayConfig {
    /// How long the stream may stay silent before it is considered dead.
    pub fn silence_timeout(&self) -> Duration {
        self.heartbeat_interval.mul_f64(self.silence_factor)
    }
}

/// Entry point for starting the event stream.
pub struct Gateway;

impl Gateway {
    /// Starts the gateway with default settings on the current tokio runtime (natively) or the browser event loop (wasm32).
    ///
    /// `on_update` is called from the gateway task; keep it quick (forward to a
    /// channel or an event-loop proxy). The token is read from `client` on every
    /// (re)connect, so refreshing it with [`Client::set_token`] takes effect.
    ///
    /// # Panics
    /// Outside a tokio runtime.
    pub fn spawn(client: Client, on_update: impl Fn(GatewayUpdate) + Send + Sync + 'static) -> GatewayHandle {
        Self::spawn_with(client, GatewayConfig::default(), on_update)
    }

    /// Like [`spawn`](Self::spawn) with custom [`GatewayConfig`].
    pub fn spawn_with(
        client: Client,
        config: GatewayConfig,
        on_update: impl Fn(GatewayUpdate) + Send + Sync + 'static,
    ) -> GatewayHandle {
        GatewayHandle { task: runtime::spawn(run(client, config, Arc::new(on_update))) }
    }
}

/// Keeps the gateway running; stops it on [`shutdown`](Self::shutdown) or drop.
#[must_use = "dropping the handle stops the gateway"]
pub struct GatewayHandle {
    task: Task,
}

impl GatewayHandle {
    /// Stops the gateway and closes the stream. No further updates are delivered
    /// once the task has been cancelled at its next await point.
    pub fn shutdown(self) {
        drop(self);
    }

    /// Whether the gateway task has ended (shut down, or stopped by `AuthExpired`).
    pub fn is_stopped(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn classify(err: ApiError) -> Failure {
    match err {
        ApiError::Unauthenticated => Failure::Unauthenticated,
        other => Failure::Transient(other.to_string()),
    }
}

async fn run(client: Client, config: GatewayConfig, on_update: Arc<dyn Fn(GatewayUpdate) + Send + Sync>) {
    let silence = config.silence_timeout();
    let mut machine = Machine::new(config);
    let mut stream: Option<EventStream> = None;
    let mut input = Input::Start;

    loop {
        let mut io = None;
        for action in machine.step(input, rand::random::<f64>()) {
            match action {
                Action::Emit(update) => on_update(update),
                other => io = Some(other),
            }
        }

        input = match io {
            Some(Action::Halt) => return,
            Some(Action::Wait(delay)) => {
                sleep(delay).await;
                Input::BackoffElapsed
            }
            Some(Action::Connect) => match timeout(silence, client.open_stream()).await {
                Ok(Ok(opened)) => {
                    stream = Some(opened);
                    Input::Opened
                }
                Ok(Err(err)) => Input::Failed(classify(err)),
                Err(_) => Input::Failed(Failure::Transient("timed out opening the stream".into())),
            },
            // Connected: wait for the next event.
            _ => {
                let Some(open) = stream.as_mut() else { return };
                let failure = match timeout(silence, open.next()).await {
                    Ok(Ok(Some(event))) => {
                        input = Input::Event(event);
                        continue;
                    }
                    Ok(Ok(None)) => Failure::Transient("stream ended".into()),
                    Ok(Err(err)) => classify(err),
                    Err(_) => Failure::Transient(format!("no events for {silence:?}")),
                };
                stream = None;
                Input::Failed(failure)
            }
        };
    }
}
