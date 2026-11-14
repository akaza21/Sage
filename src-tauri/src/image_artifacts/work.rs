//! Image normalisation runs one permit at a time, uploads and prompt sends
//! alike, so native memory for decoding stays within
//! `IMAGE_WORK_MEMORY_BYTES` however many images arrive together.
use tokio::sync::{Semaphore, SemaphorePermit};

use sage_core::limits::IMAGE_WORK_PERMITS;

static IMAGE_WORK: Semaphore = Semaphore::const_new(IMAGE_WORK_PERMITS);

/// Waits for a share of the image-work memory budget. Hold the permit until
/// the decoded pixels are freed.
pub(crate) async fn image_work_permit() -> SemaphorePermit<'static> {
    match IMAGE_WORK.acquire().await {
        Ok(permit) => permit,
        // The semaphore is a static that is never closed.
        Err(_) => unreachable!("the image work semaphore is never closed"),
    }
}

#[cfg(test)]
pub(crate) fn image_work_available() -> usize {
    IMAGE_WORK.available_permits()
}
