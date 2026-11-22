//! The HTTP accept loop.
//!
//! `axum::serve` has no knobs for the HTTP/1 header read timeout (slowloris:
//! a client trickling request headers holds a connection forever), so this is
//! the same loop with hyper's builder exposed, plus a bounded graceful drain.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;

use crate::shutdown::Shutdown;

/// How long a client may take to send the request headers.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long open connections get to finish after shutdown starts.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Serves `app` until `shutdown` is triggered, then stops accepting and waits
/// up to `drain` for open connections (gateway streams end on the same
/// signal). Returns even if some connections are still open after `drain`.
pub async fn serve(listener: TcpListener, app: Router, shutdown: Shutdown, header_read_timeout: Duration, drain: Duration) {
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().timer(TokioTimer::new()).header_read_timeout(header_read_timeout);
    let graceful = GracefulShutdown::new();
    loop {
        let (stream, peer): (_, SocketAddr) = tokio::select! {
            biased;
            _ = shutdown.triggered() => break,
            accepted = listener.accept() => match accepted {
                Ok(connection) => connection,
                Err(e) => {
                    // Typically EMFILE: back off instead of spinning.
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        let _ = stream.set_nodelay(true);
        // ConnectInfo<SocketAddr> is stamped into every request's extensions,
        // which `clientip` reads.
        let service = TowerToHyperService::new(app.clone().layer(axum::Extension(ConnectInfo(peer))));
        let connection = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).into_owned();
        let connection = graceful.watch(connection);
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::debug!(error = %e, "connection ended with an error");
            }
        });
    }
    drop(listener);
    match tokio::time::timeout(drain, graceful.shutdown()).await {
        Ok(()) => tracing::info!("all connections drained"),
        Err(_) => tracing::warn!(seconds = drain.as_secs(), "drain timed out; closing remaining connections"),
    }
}
