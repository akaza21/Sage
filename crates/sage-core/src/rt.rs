//! The process's one async runtime, for code that does not know Tauri.
//!
//! The runtime is a multi-thread Tokio runtime, built on first use, the same
//! way and with the same configuration as Tauri's default runtime. The app
//! hands this runtime to Tauri at startup (`tauri::async_runtime::set`), before
//! anything touches Tauri's runtime, so the process keeps one runtime and one
//! thread pool.

use std::{future::Future, sync::OnceLock};

use tokio::{
    runtime::{Handle, Runtime},
    task::JoinHandle,
};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Tokio runtime"))
}

/// The runtime's handle, building the runtime on first use.
pub fn handle() -> &'static Handle {
    runtime().handle()
}

/// Runs `task` on the runtime's worker threads.
#[track_caller]
pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(task)
}

/// Runs `func` on the runtime's blocking thread pool.
#[track_caller]
pub fn spawn_blocking<F, R>(func: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(func)
}

/// Runs `task` to completion on the calling thread, inside the runtime's
/// context. Like any Tokio `block_on`, it panics when called from inside a
/// runtime context (an async task or another `block_on`).
#[track_caller]
pub fn block_on<F: Future>(task: F) -> F::Output {
    runtime().block_on(task)
}
