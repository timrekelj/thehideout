//! Where the backend's async work runs: a small multi-threaded tokio runtime natively, the
//! browser's event loop (`spawn_local`) on wasm32. Futures keep their `Send` bounds on both, so
//! the code that spawns them is the same; the browser's `!Send` JS handles are wrapped in
//! `send_wrapper` where they appear (sound: wasm32-unknown-unknown has one thread).

use std::future::Future;

/// A spawned task. It stops at its next await point on [`abort`](Self::abort); dropping the
/// handle does not stop it.
pub struct Task(TaskInner);

impl Task {
    pub fn abort(&self) {
        self.0.abort();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;

    pub struct TaskInner(tokio::task::AbortHandle);

    impl TaskInner {
        pub fn abort(&self) {
            self.0.abort();
        }
    }

    /// Lazily built, so constructing a backend (headless scenes build one and drop it) costs nothing.
    #[derive(Default)]
    pub struct Rt(Option<tokio::runtime::Runtime>);

    impl Rt {
        /// Two workers: the gateway stream is a long-lived task and a slow decode or a stalled
        /// RPC must not hold it up.
        fn runtime(&mut self) -> &tokio::runtime::Runtime {
            self.0.get_or_insert_with(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("thehideout-net")
                    .enable_all()
                    .build()
                    .expect("start the network runtime")
            })
        }

        pub fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) -> Task {
            Task(TaskInner(self.runtime().spawn(fut).abort_handle()))
        }
    }

    /// Runs `f` where blocking is fine (a blocking thread of the runtime).
    pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;

    pub struct TaskInner(futures::future::AbortHandle);

    impl TaskInner {
        pub fn abort(&self) {
            self.0.abort();
        }
    }

    #[derive(Default)]
    pub struct Rt;

    impl Rt {
        pub fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) -> Task {
            let (handle, registration) = futures::future::AbortHandle::new_pair();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = futures::future::Abortable::new(fut, registration).await;
            });
            Task(TaskInner(handle))
        }
    }

    /// There is no blocking pool in the browser: `f` runs at once (callers keep it short).
    pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
        Ok(f())
    }
}

pub use imp::Rt;
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
pub use imp::blocking;
use imp::TaskInner;
