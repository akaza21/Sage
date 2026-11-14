use std::{
    sync::{Condvar, Mutex, MutexGuard},
    time::Duration,
};

use crate::acp::{
    contract::{SageChatId, SageProjectId},
    service::{
        RendererRecreationUsage,
        renderer_recreation::RendererRecreationNativeSnapshot,
        renderer_recreation_orchestrator::{
            MainWindowRendererReload, RendererRecreationPairError,
            pair_renderer_recreation_after_observed_reload,
        },
    },
};
use crate::state::AppState;

/// How long a finished load waits for its start snapshot. The snapshot reads
/// a bounded number of rows, so this only trips if the store is wedged.
const START_CAPTURE_WAIT: Duration = Duration::from_secs(30);

/// Both snapshots scan the store, so neither runs on the UI thread that reports
/// page loads: a start is captured on the blocking pool, and its finish waits.
#[derive(Default)]
pub(crate) struct RendererRecreationReloadLifecycle {
    slot: Mutex<Slot>,
    captured: Condvar,
}

pub(crate) struct StartPoint {
    id: u64,
    usage: RendererRecreationUsage,
    reader: Reader,
}

enum Reader {
    Pinned(rusqlite::Connection),
    Live,
    /// The pin failed; the start is not captured, so it cannot pair falsely.
    Unavailable,
}

struct SettleOnDrop<'a> {
    lifecycle: &'a RendererRecreationReloadLifecycle,
    id: u64,
    before: Option<PendingReload>,
}

impl Drop for SettleOnDrop<'_> {
    fn drop(&mut self) {
        self.lifecycle.settle_start(self.id, self.before.take());
    }
}

#[derive(Default)]
struct Slot {
    /// The newest start reported, and the newest start whose capture ended.
    started: u64,
    settled: u64,
    pending: Option<PendingReload>,
}

struct PendingReload {
    project_id: SageProjectId,
    chat_id: SageChatId,
    before: RendererRecreationNativeSnapshot,
}

impl RendererRecreationReloadLifecycle {
    #[cfg(test)]
    fn record_started(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        before: RendererRecreationNativeSnapshot,
    ) {
        self.slot().pending = Some(PendingReload {
            project_id,
            chat_id,
            before,
        });
    }

    pub(crate) fn take_finished(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
    ) -> Option<RendererRecreationNativeSnapshot> {
        let pending = self.slot().pending.take()?;
        (pending.project_id == *project_id && pending.chat_id == *chat_id).then_some(pending.before)
    }

    /// Called on the UI thread. Fixes the point in time the snapshot describes (a
    /// read transaction and the live counters), so the reloaded page's own
    /// requests can never land in "before"; the costly hashing waits for
    /// [`Self::capture_start`].
    ///
    /// The pinned reader holds back WAL checkpoints until the capture ends.
    pub(crate) fn begin_start(&self, state: &AppState) -> StartPoint {
        let usage = state.acp.renderer_recreation_usage();
        let reader = match state.database.pinned_reader() {
            Ok(Some(reader)) => Reader::Pinned(reader),
            // An in-memory store has no second connection to pin.
            Ok(None) => Reader::Live,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "ACP renderer recreation pairing skipped: no pinned reader"
                );
                Reader::Unavailable
            }
        };
        // Numbered last, so a start that fails above is never left unsettled.
        StartPoint {
            id: self.number_start(),
            usage,
            reader,
        }
    }

    fn number_start(&self) -> u64 {
        let mut slot = self.slot();
        slot.pending = None;
        slot.started += 1;
        slot.started
    }

    pub(crate) fn capture_start(&self, state: &AppState, start: StartPoint) {
        // Settles even if the capture panics, so a finish never waits it out.
        let mut settle = SettleOnDrop {
            lifecycle: self,
            id: start.id,
            before: None,
        };
        if matches!(start.reader, Reader::Unavailable) {
            return;
        }
        // Admission reads the live store: a reload that switched the active
        // Chat before this runs is paired against that Chat's pinned state.
        settle.before = admitted_codex_page(state).and_then(|(project_id, chat_id)| {
            let before = match &start.reader {
                Reader::Pinned(reader) => state.acp.renderer_recreation_snapshot_on(
                    reader,
                    &project_id,
                    &chat_id,
                    start.usage,
                ),
                Reader::Live | Reader::Unavailable => state.acp.renderer_recreation_snapshot_on(
                    &state.database.connection(),
                    &project_id,
                    &chat_id,
                    start.usage,
                ),
            }
            .ok()?;
            Some(PendingReload {
                project_id,
                chat_id,
                before,
            })
        });
    }

    fn settle_start(&self, id: u64, before: Option<PendingReload>) {
        let mut slot = self.slot();
        if slot.started == id {
            slot.pending = before;
        }
        slot.settled = slot.settled.max(id);
        drop(slot);
        self.captured.notify_all();
    }

    pub(crate) fn pair_admitted_finish(
        &self,
        state: &AppState,
        window: &tauri::WebviewWindow,
    ) -> Option<Result<(), RendererRecreationPairError>> {
        if !self.wait_for_start_capture(START_CAPTURE_WAIT) {
            tracing::warn!("ACP renderer recreation pairing skipped: start capture timed out");
            return None;
        }
        let (project_id, chat_id) = admitted_codex_page(state)?;
        let before = self.take_finished(&project_id, &chat_id)?;
        let frontend = MainWindowRendererReload::new(window);
        Some(pair_renderer_recreation_after_observed_reload(
            &state.acp,
            before,
            &project_id,
            &chat_id,
            &frontend,
        ))
    }

    fn wait_for_start_capture(&self, timeout: Duration) -> bool {
        let slot = self.slot();
        let (slot, _) = self
            .captured
            .wait_timeout_while(slot, timeout, |slot| slot.settled < slot.started)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.settled >= slot.started
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn admitted_codex_page(state: &AppState) -> Option<(SageProjectId, SageChatId)> {
    let bootstrap = state
        .database
        .bootstrap_snapshot_with_catalog(&state.harnesses)
        .ok()?;
    if bootstrap.selected_harness_id.as_deref() != Some("codex") {
        return None;
    }
    let project_id = SageProjectId::parse(&bootstrap.project?.id).ok()?;
    let chat_id = SageChatId::parse(&bootstrap.active_tab_id?).ok()?;
    let page = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .ok()?;
    let current_session_id = page.current_session_id?;
    page.sessions
        .as_slice()
        .iter()
        .any(|session| {
            session.id == current_session_id && session.binding_id.as_str() == "codex-app-server"
        })
        .then_some((project_id, chat_id))
}

#[cfg(test)]
mod tests {
    use super::RendererRecreationReloadLifecycle;
    use crate::{
        acp::{
            contract::{SageChatId, SageProjectId},
            service::AcpService,
        },
        persistence::Database,
    };

    fn fixture_database() -> Database {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        database
            .connection()
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
                 VALUES('project-reload','Reload',1,1,1);
                 INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                 VALUES('root-reload','project-reload','C:/reload','c:/reload',0);
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES('chat-reload','project-reload',0,'Reload',7,1,1);",
            )
            .unwrap();
        database
    }

    #[test]
    fn finished_reload_only_consumes_a_matching_admitted_page() {
        let service = AcpService::new(fixture_database()).unwrap();
        let project_id = SageProjectId::parse("project-reload").unwrap();
        let chat_id = SageChatId::parse("chat-reload").unwrap();
        let before = service
            .renderer_recreation_native_snapshot(&project_id, &chat_id)
            .unwrap();
        let lifecycle = RendererRecreationReloadLifecycle::default();

        lifecycle.record_started(project_id.clone(), chat_id.clone(), before);
        assert!(lifecycle.take_finished(&project_id, &chat_id).is_some());

        lifecycle.record_started(
            project_id.clone(),
            chat_id.clone(),
            service
                .renderer_recreation_native_snapshot(&project_id, &chat_id)
                .unwrap(),
        );
        assert!(
            lifecycle
                .take_finished(&project_id, &SageChatId::parse("foreign-chat").unwrap())
                .is_none()
        );
        assert!(lifecycle.take_finished(&project_id, &chat_id).is_none());
    }

    fn pending(service: &AcpService) -> super::PendingReload {
        let project_id = SageProjectId::parse("project-reload").unwrap();
        let chat_id = SageChatId::parse("chat-reload").unwrap();
        super::PendingReload {
            before: service
                .renderer_recreation_native_snapshot(&project_id, &chat_id)
                .unwrap(),
            project_id,
            chat_id,
        }
    }

    #[test]
    fn a_finish_waits_for_its_start_capture_and_a_newer_start_supersedes_an_older_one() {
        let service = AcpService::new(fixture_database()).unwrap();
        let project_id = SageProjectId::parse("project-reload").unwrap();
        let chat_id = SageChatId::parse("chat-reload").unwrap();
        let lifecycle = RendererRecreationReloadLifecycle::default();
        let no_wait = std::time::Duration::ZERO;
        assert!(lifecycle.wait_for_start_capture(no_wait));

        let older = lifecycle.number_start();
        let newer = lifecycle.number_start();
        assert!(!lifecycle.wait_for_start_capture(no_wait));
        lifecycle.settle_start(older, Some(pending(&service)));
        assert!(!lifecycle.wait_for_start_capture(no_wait));
        assert!(lifecycle.take_finished(&project_id, &chat_id).is_none());

        lifecycle.settle_start(newer, Some(pending(&service)));
        assert!(lifecycle.wait_for_start_capture(no_wait));
        assert!(lifecycle.take_finished(&project_id, &chat_id).is_some());
    }
}
