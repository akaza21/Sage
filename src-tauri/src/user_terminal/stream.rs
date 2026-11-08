use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;

/// How long a read waits for new output before answering with none, so the
/// renderer's long poll returns well inside any IPC timeout.
const LONG_POLL: std::time::Duration = std::time::Duration::from_secs(25);

struct ReaderPermit(Arc<AtomicBool>);

impl ReaderPermit {
    fn acquire(busy: Arc<AtomicBool>) -> Result<Self, TerminalError> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| TerminalError::Busy)?;
        Ok(Self(busy))
    }
}

impl Drop for ReaderPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl TerminalSessionPort {
    pub async fn read_output(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
        cursor: u64,
    ) -> Result<TerminalOutputPage, TerminalError> {
        let (output, busy) = self.checked_output(authority, scope, handle).await?;
        let _permit = ReaderPermit::acquire(busy)?;
        let mut changed = output.subscribe();
        let first = output.page(cursor)?;
        if !first.bytes.is_empty() || first.truncated {
            return Ok(first);
        }
        let _ = tokio::time::timeout(LONG_POLL, changed.changed()).await;
        let (current, _) = self.checked_output(authority, scope, handle).await?;
        current.page(cursor)
    }

    async fn checked_output(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
    ) -> Result<(TerminalOutputSink, Arc<AtomicBool>), TerminalError> {
        let _context = authority
            .0
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        let sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        if session.scope != *scope || session.revision != handle.revision {
            return Err(TerminalError::InvalidScope);
        }
        Ok((session.output.clone(), Arc::clone(&session.reader_busy)))
    }
}
