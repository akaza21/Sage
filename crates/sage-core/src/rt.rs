//! The app hands this runtime to Tauri at startup (`tauri::async_runtime::set`),
//! before anything touches Tauri's runtime, so the process keeps one runtime.

use std::{future::Future, sync::OnceLock};

use tokio::{
    runtime::{Handle, Runtime},
    task::JoinHandle,
};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Tokio runtime"))
}

pub fn handle() -> &'static Handle {
    runtime().handle()
}

#[track_caller]
pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(task)
}

#[track_caller]
pub fn spawn_blocking<F, R>(func: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(func)
}

/// Panics when called inside a runtime context, like any Tokio `block_on`.
#[track_caller]
pub fn block_on<F: Future>(task: F) -> F::Output {
    runtime().block_on(task)
}
