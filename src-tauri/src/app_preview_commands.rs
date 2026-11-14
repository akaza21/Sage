#[path = "app_preview/mod.rs"]
mod app_preview;

use rusqlite::params;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::project_registry::ProjectRegistryStore;
use crate::{acp::ipc::ACP_PROJECTION_CHANGED_EVENT, state::AppState};
use sage_acp_protocol::contract::ProjectGeneration;
use sage_db::database::Database;

use self::app_preview::{
    PreviewDiscoveryRequest, PreviewDiscoveryResponse, PreviewError, PreviewFailure,
    PreviewHealthResponse, PreviewProjection, PreviewProjectionOwner, PreviewStartResponse,
    PreviewStopResponse, PreviewTargetRef,
    discovery::{discover_uninstalled, validate_discovery_request, validate_target},
    lifecycle,
};

#[tauri::command]
pub async fn discover_preview_targets(
    app: AppHandle,
    state: State<'_, AppState>,
    request: PreviewDiscoveryRequest,
) -> Result<PreviewDiscoveryResponse, PreviewError> {
    validate_discovery_request(&request)?;
    let owner = PreviewProjectionOwner::from_discovery(&request);
    let roots = {
        let _guard = validate_current_owner(&state, &owner).await?;
        let observer = guarded_desktop_observer(&app, &state.database);
        lifecycle::begin_discovery(&request, &observer)?;
        state
            .database
            .root_records(owner.project_id.as_str())
            .map_err(|_| PreviewError::internal("Preview could not resolve Project roots."))?
    };
    let worker_request = request.clone();
    let (response, mappings) =
        tauri::async_runtime::spawn_blocking(move || discover_uninstalled(&worker_request, roots))
            .await
            .map_err(|_| PreviewError::internal("Preview discovery worker did not converge."))??;
    let _guard = validate_current_owner(&state, &owner).await?;
    let observer = guarded_desktop_observer(&app, &state.database);
    lifecycle::finish_discovery(&request, mappings, &observer)?;
    Ok(response)
}

#[tauri::command]
pub async fn get_preview_health(
    app: AppHandle,
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewHealthResponse, PreviewError> {
    validate_target(&request)?;
    let owner = PreviewProjectionOwner::from_target(&request);
    let guard = validate_current_owner(&state, &owner).await?;
    drop(guard);
    let database = state.database.clone();
    let worker_request = request.clone();
    let origin = tauri::async_runtime::spawn_blocking(move || {
        let observer = desktop_observer(&app, &database);
        lifecycle::health_observed(&worker_request, &observer)
    })
    .await
    .map_err(|_| PreviewError::internal("Preview health worker did not converge."))??;
    Ok(PreviewHealthResponse {
        ref_: request,
        healthy: origin.is_some(),
        origin,
    })
}

#[tauri::command]
pub async fn start_preview_target(
    app: AppHandle,
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewStartResponse, PreviewError> {
    validate_target(&request)?;
    let owner = PreviewProjectionOwner::from_target(&request);
    let guard = validate_current_owner(&state, &owner).await?;
    {
        let observer = guarded_desktop_observer(&app, &state.database);
        lifecycle::begin_start(&request, &observer)?;
    }
    drop(guard);
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let observer = desktop_observer(&app, &database);
        let reservation = match lifecycle::reserve_start(request.clone()) {
            Ok(reservation) => reservation,
            Err(error) => {
                let _ = lifecycle::fail_start_without_child(
                    &request,
                    PreviewFailure::Internal,
                    &observer,
                );
                return Err(error);
            }
        };
        if let Err(error) = lifecycle::mark_waiting_for_origin(&request, &observer) {
            let _ = lifecycle::fail_start_after_reservation(&request, &observer);
            return Err(error);
        }
        lifecycle::finish_start(reservation, &observer)
    })
    .await
    .map_err(|_| PreviewError::internal("Preview start worker did not converge."))?
}

#[tauri::command]
pub async fn stop_preview_target(
    app: AppHandle,
    state: State<'_, AppState>,
    request: PreviewTargetRef,
) -> Result<PreviewStopResponse, PreviewError> {
    validate_target(&request)?;
    let owner = PreviewProjectionOwner::from_target(&request);
    let guard = validate_current_owner(&state, &owner).await?;
    drop(guard);
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let observer = desktop_observer(&app, &database);
        lifecycle::stop_observed(&request, &observer)
    })
    .await
    .map_err(|_| PreviewError::internal("Preview stop worker did not converge."))?
}

#[cfg(test)]
pub(crate) use self::app_preview::test_support::ServingPreview;

pub(crate) fn stop_preview_children_before_epoch(new_epoch: u64) {
    lifecycle::stop_before_epoch(new_epoch);
}

pub(crate) fn retire_preview_runtime_owner(
    owner: &sage_acp_protocol::scheduler::RuntimeOwner,
) -> Result<(), PreviewError> {
    lifecycle::retire_runtime_owner(owner)
}

fn guarded_desktop_observer<'a>(
    app: &'a AppHandle,
    database: &'a Database,
) -> impl Fn(&PreviewProjection) -> Result<(), PreviewError> + Send + Sync + 'a {
    move |projection| {
        validate_session_owner(database, &projection.owner)?;
        app.emit(ACP_PROJECTION_CHANGED_EVENT, projection.event())
            .map_err(|_| PreviewError::internal("Preview projection could not be published."))
    }
}

fn desktop_observer<'a>(
    app: &'a AppHandle,
    database: &'a Database,
) -> impl Fn(&PreviewProjection) -> Result<(), PreviewError> + Send + Sync + 'a {
    move |projection| {
        let state = app.state::<AppState>();
        let transition = state.project_transitions.try_lock().map_err(|_| {
            PreviewError::conflict(
                "transitionEpoch",
                "Preview publication raced a Project transition.",
            )
        })?;
        validate_transition_owner(*transition, &projection.owner)?;
        validate_session_owner(database, &projection.owner)?;
        app.emit(ACP_PROJECTION_CHANGED_EVENT, projection.event())
            .map_err(|_| PreviewError::internal("Preview projection could not be published."))
    }
}

async fn validate_current_owner<'state>(
    state: &'state AppState,
    owner: &PreviewProjectionOwner,
) -> Result<tokio::sync::MutexGuard<'state, u64>, PreviewError> {
    let transition = state.project_transitions.lock().await;
    validate_transition_owner(*transition, owner)?;
    validate_session_owner(&state.database, owner)?;
    Ok(transition)
}

fn validate_transition_owner(
    transition: u64,
    owner: &PreviewProjectionOwner,
) -> Result<(), PreviewError> {
    let generation = ProjectGeneration::new(transition)
        .map_err(|_| PreviewError::internal("Preview Project generation is unavailable."))?;
    if generation == owner.project_generation && transition == owner.transition_epoch {
        Ok(())
    } else {
        Err(PreviewError::conflict(
            "transitionEpoch",
            "Preview request belongs to a stale Project transition.",
        ))
    }
}

fn validate_session_owner(
    database: &Database,
    owner: &PreviewProjectionOwner,
) -> Result<(), PreviewError> {
    let admitted = database
        .connection()
        .query_row(
            "SELECT EXISTS(
               SELECT 1
                 FROM app_state a
                 JOIN agent_sessions s
                   ON s.project_id=a.active_project_id
                 JOIN session_runtime_epochs e
                   ON e.project_id=s.project_id
                  AND e.chat_id=s.chat_id
                  AND e.session_id=s.id
                WHERE a.singleton=1
                  AND a.active_project_id=?1
                  AND s.project_id=?1
                  AND s.chat_id=?2
                  AND s.id=?3
                  AND s.state IN ('ready','active')
                  AND s.residency='hot'
                  AND e.runtime_epoch=?4
                  AND e.state='running'
            )",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| PreviewError::internal("Preview owner could not be validated."))?;
    admitted.then_some(()).ok_or_else(|| {
        PreviewError::conflict("previewOwner", "Preview owner or runtime epoch is stale.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_acp_protocol::contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
    };

    fn owner() -> PreviewProjectionOwner {
        PreviewProjectionOwner {
            project_id: SageProjectId::parse("project-preview").unwrap(),
            project_generation: ProjectGeneration::new(7).unwrap(),
            transition_epoch: 7,
            chat_id: SageChatId::parse("chat-preview").unwrap(),
            session_id: SageAgentSessionId::parse("session-preview").unwrap(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
        }
    }

    fn state() -> (AppState, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("sage-preview-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let database = crate::database::open_database(&path).unwrap();
        let state = AppState::try_new(database).unwrap();
        state
            .database
            .connection()
            .execute_batch(
                "INSERT INTO projects VALUES('project-preview','Preview',1,1,1);
                 UPDATE app_state SET active_project_id='project-preview',updated_at=1 WHERE singleton=1;
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-preview','project-preview',0,'Preview',0,1,1);
                 INSERT INTO agent_sessions(
                   id,project_id,chat_id,ordinal,binding_id,binding_digest,
                   capabilities_json,state,residency,legacy,revision,
                   projection_revision,created_at,updated_at,terminal_at
                 ) VALUES(
                   'session-preview','project-preview','chat-preview',0,'binding',
                   '0000000000000000000000000000000000000000000000000000000000000000',
                   '{}','ready','hot',0,1,1,1,1,NULL
                 );
                 INSERT INTO session_runtime_epochs(
                   project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,
                   started_at,terminal_at,exit_kind
                 ) VALUES(
                   'project-preview','chat-preview','session-preview',3,'running',NULL,1,NULL,NULL
                 );",
            )
            .unwrap();
        (state, path)
    }

    #[test]
    fn exact_owner_observation_rejects_each_forged_identity_field() {
        tauri::async_runtime::block_on(async {
            let (state, path) = state();
            *state.project_transitions.lock().await = 7;
            let exact = owner();
            assert!(validate_current_owner(&state, &exact).await.is_ok());

            let forged = [
                PreviewProjectionOwner {
                    project_id: SageProjectId::parse("project-foreign").unwrap(),
                    ..exact.clone()
                },
                PreviewProjectionOwner {
                    project_generation: ProjectGeneration::new(8).unwrap(),
                    ..exact.clone()
                },
                PreviewProjectionOwner {
                    transition_epoch: 8,
                    ..exact.clone()
                },
                PreviewProjectionOwner {
                    chat_id: SageChatId::parse("chat-foreign").unwrap(),
                    ..exact.clone()
                },
                PreviewProjectionOwner {
                    session_id: SageAgentSessionId::parse("session-foreign").unwrap(),
                    ..exact.clone()
                },
                PreviewProjectionOwner {
                    runtime_epoch: RuntimeEpoch::new(4).unwrap(),
                    ..exact
                },
            ];
            for candidate in forged {
                assert!(validate_current_owner(&state, &candidate).await.is_err());
            }
            drop(state);
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        });
    }
}
