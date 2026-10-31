//! One background thread for download file-system work: planning a silent
//! save, remembering the first folder, and releasing placeholders. That work
//! can block (an offline network share), so it never runs on the WebView2
//! UI thread. The queue is bounded; when it is full the caller falls back
//! (a download is canceled, a placeholder waits for the startup sweep).

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        OnceLock,
        mpsc::{SyncSender, sync_channel},
    },
};

type Job = Box<dyn FnOnce() + Send>;

const MAX_QUEUED: usize = 32;

static WORKER: OnceLock<Option<SyncSender<Job>>> = OnceLock::new();

/// Queues `job`; false when the queue is full or the thread is gone.
pub(crate) fn submit(job: impl FnOnce() + Send + 'static) -> bool {
    let sender = WORKER.get_or_init(|| {
        let (sender, receiver) = sync_channel::<Job>(MAX_QUEUED);
        std::thread::Builder::new()
            .name("browser-downloads".into())
            .spawn(move || {
                for job in receiver {
                    let _ = catch_unwind(AssertUnwindSafe(job));
                }
            })
            .ok()
            .map(|_| sender)
    });
    sender
        .as_ref()
        .is_some_and(|sender| sender.try_send(Box::new(job)).is_ok())
}

/// Waits until every job queued before this call has run.
#[cfg(test)]
pub(crate) fn flush() {
    let (sender, receiver) = std::sync::mpsc::channel();
    assert!(submit(move || {
        let _ = sender.send(());
    }));
    receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
}
