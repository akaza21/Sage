use std::time::Duration;

use tokio::{task::JoinHandle, time::timeout};

use super::BoundedBytes;

pub(super) type ReaderTask = JoinHandle<std::io::Result<BoundedBytes>>;

/// A Git proxy can exit while its backend still owns the inherited pipes.
/// Retain bounded readers through cleanup until their inherited pipes reach
/// EOF. Redirected or detached children require separate process ownership.
#[derive(Default)]
pub(super) struct ReaderTasks {
    pub(super) stdout: Option<ReaderTask>,
    pub(super) stderr: Option<ReaderTask>,
    pub(super) failed: bool,
}

impl ReaderTasks {
    pub(super) async fn wait_for_eof(&mut self, budget: Duration) -> bool {
        let completed = timeout(budget, async {
            for slot in [&mut self.stdout, &mut self.stderr] {
                if let Some(task) = slot.as_mut() {
                    let result = task.await;
                    slot.take();
                    self.failed |= !matches!(result, Ok(Ok(_)));
                }
            }
        })
        .await
        .is_ok();
        completed && !self.failed
    }
}

impl Drop for ReaderTasks {
    fn drop(&mut self) {
        for task in [&self.stdout, &self.stderr].into_iter().flatten() {
            task.abort();
        }
    }
}
