use std::sync::Mutex;

use crate::acp::{
    contract::{SageChatId, SageProjectId},
    service::{
        renderer_recreation::RendererRecreationNativeSnapshot,
        renderer_recreation_orchestrator::{
            MainWindowRendererReload, RendererRecreationPairError,
            pair_renderer_recreation_after_observed_reload,
        },
    },
};
use crate::state::AppState;

#[derive(Default)]
pub(crate) struct RendererRecreationReloadLifecycle {
    pending: Mutex<Option<PendingReload>>,
}

struct PendingReload {
    project_id: SageProjectId,
    chat_id: SageChatId,
    before: RendererRecreationNativeSnapshot,
}

impl RendererRecreationReloadLifecycle {
    pub(crate) fn record_started(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        before: RendererRecreationNativeSnapshot,
    ) {
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PendingReload {
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
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()?;
        (pending.project_id == *project_id && pending.chat_id == *chat_id).then_some(pending.before)
    }

    pub(crate) fn record_admitted_start(&self, state: &AppState) {
        let Some((project_id, chat_id)) = admitted_codex_page(state) else {
            self.clear();
            return;
        };
        let Ok(before) = state
            .acp
            .renderer_recreation_native_snapshot(&project_id, &chat_id)
        else {
            self.clear();
            return;
        };
        self.record_started(project_id, chat_id, before);
    }

    pub(crate) fn pair_admitted_finish(
        &self,
        state: &AppState,
        window: &tauri::WebviewWindow,
    ) -> Option<Result<(), RendererRecreationPairError>> {
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

    fn clear(&self) {
        let _ = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
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

    #[test]
    fn finished_reload_only_consumes_a_matching_admitted_page() {
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
        let service = AcpService::new(database).unwrap();
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
}
