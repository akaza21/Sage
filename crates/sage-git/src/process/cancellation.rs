//! Watch-window cancellation also reaches repository admission probes.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

tokio::task_local! {
    pub(super) static CANCELLED: Arc<AtomicBool>;
}

pub(super) async fn cancelled() {
    let flag = CANCELLED.try_with(Arc::clone).ok();
    let Some(flag) = flag else {
        return std::future::pending().await;
    };
    while !flag.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub(super) fn ensure_active() -> sage_core::error::AppResult<()> {
    if CANCELLED
        .try_with(|flag| flag.load(Ordering::Acquire))
        .unwrap_or(false)
    {
        Err(error())
    } else {
        Ok(())
    }
}

pub(super) fn error() -> sage_core::error::AppError {
    sage_core::error::AppError::Git("Git watch window cancelled".into())
}

pub async fn wait_for_cleanup() -> sage_core::error::AppResult<()> {
    super::containment::await_mutation_admission(
        &super::containment::git_containment(),
        crate::budget::REAP,
    )
    .await
}

#[cfg(test)]
mod tests;
