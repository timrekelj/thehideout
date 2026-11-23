//! Spawn, sleep and timeout over tokio natively and the browser event loop on wasm32.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::future::{AbortHandle, Abortable};

/// A [`timeout`] that elapsed.
#[derive(Debug)]
pub(crate) struct Elapsed;

/// A spawned task; aborts on [`abort`](Self::abort) (not on drop).
pub(crate) struct Task {
    abort: AbortHandle,
    done: Arc<AtomicBool>,
}

impl Task {
    pub(crate) fn abort(&self) {
        self.abort.abort();
    }

    /// Whether the task has finished or been aborted.
    pub(crate) fn is_finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

/// Spawns `fut` on the current tokio runtime. Panics outside one.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn(fut: impl Future<Output = ()> + Send + 'static) -> Task {
    let (task, fut) = wrap(fut);
    tokio::spawn(fut);
    task
}

/// Spawns `fut` on the browser event loop.
#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn(fut: impl Future<Output = ()> + 'static) -> Task {
    let (task, fut) = wrap(fut);
    wasm_bindgen_futures::spawn_local(fut);
    task
}

fn wrap<F: Future<Output = ()>>(fut: F) -> (Task, impl Future<Output = ()>) {
    let (abort, registration) = AbortHandle::new_pair();
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    let fut = async move {
        // Set on every exit, including an abort (the future is dropped, not polled to the end).
        struct Done(Arc<AtomicBool>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let _done = Done(flag);
        let _ = Abortable::new(fut, registration).await;
    };
    (Task { abort, done }, fut)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn sleep(delay: Duration) {
    tokio::time::sleep(delay).await;
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn sleep(delay: Duration) {
    gloo_timers::future::sleep(delay).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn timeout<F: Future>(limit: Duration, fut: F) -> Result<F::Output, Elapsed> {
    tokio::time::timeout(limit, fut).await.map_err(|_| Elapsed)
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn timeout<F: Future>(limit: Duration, fut: F) -> Result<F::Output, Elapsed> {
    use futures::future::{Either, select};
    let fut = std::pin::pin!(fut);
    let timer = std::pin::pin!(gloo_timers::future::sleep(limit));
    match select(fut, timer).await {
        Either::Left((out, _)) => Ok(out),
        Either::Right(_) => Err(Elapsed),
    }
}
