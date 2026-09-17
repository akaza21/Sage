use super::*;
use crate::acp::{
    attached_context::{AttachedContexts, DiffLineCommentContext},
    contract::{ItemLifecycle, SafeText, SageRootId, SessionState},
    file_diff::pair_digest,
    intent::{ExternalId, NormalizedPayload, SafeResourceLocation, SafeResourceRef},
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner, FilePairCommand,
        ItemIngressCommand, SessionTerminalCommand, apply_adapter_inbound_batch,
        apply_session_terminal,
    },
};
use sha2::{Digest, Sha256};

const OLD: &str = "PRIVATE_OLD_BODY\nPRIVATE_OLD_SECOND_LINE\n";
const NEW: &str = "PRIVATE_NEW_BODY\n";
type HostObservations = (Arc<Mutex<Vec<&'static str>>>, Arc<Mutex<Vec<String>>>);

struct Fixture {
    state: AppState,
    owner: RuntimeOwner,
    context: DiffLineCommentContext,
    root: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.acp.shutdown_blocking();
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    async fn new() -> Self {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let binding = state.acp.codex_registration().refresh().unwrap();
        let revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        {
            let mut connection = state.database.connection();
            activate_session(
                &mut connection,
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    runtime_epoch: owner.runtime_epoch,
                    artifact_digest: binding.artifact_digest().to_owned(),
                    now_millis: 2,
                },
            )
            .unwrap();
            complete_activation(
                &mut connection,
                CompleteActivationCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    binding_digest: binding.binding_digest_hex().to_owned(),
                    artifact_digest: binding.artifact_digest().to_owned(),
                    external_session_id: ExternalId::new("external", "thread-command-route")
                        .unwrap(),
                    capabilities_json: serde_json::to_string(
                        &binding.capability_ceiling().unwrap(),
                    )
                    .unwrap(),
                    now_millis: 3,
                },
            )
            .unwrap();
        }
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let context = {
            let mut connection = state.database.connection();
            let root_id: String = connection
                .query_row(
                    "SELECT id FROM project_roots WHERE project_id=?1",
                    [project_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            let prepared = prepare_turn(
                &mut connection,
                PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "seed snapshot").unwrap(),
                    now_millis: 10,
                },
            )
            .unwrap();
            begin_send(
                &mut connection,
                TurnMutationCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    turn_id: prepared.turn_id.clone(),
                    expected_revision: prepared.turn_revision,
                    now_millis: 11,
                },
            )
            .unwrap();
            let batch_owner = AdapterStoreOwner {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                turn_id: prepared.turn_id.clone(),
                binding_digest: binding.binding_digest_hex().to_owned(),
                external_session_id: ExternalId::new("external", "thread-command-route").unwrap(),
            };
            let payload = NormalizedPayload::file_admitted(
                SafeResourceLocation::Resource {
                    resource: SafeResourceRef::new(
                        SageRootId::parse(root_id.clone()).unwrap(),
                        "src/review.ts",
                    )
                    .unwrap(),
                },
                SafeText::new("summary", "Saved change").unwrap(),
                Some(1),
                Some(1),
                0,
                (OLD.len() as u64, NEW.len() as u64),
                pair_digest(OLD.as_bytes(), NEW.as_bytes()),
            );
            let payload_digest = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&payload).unwrap())
            );
            let external_item_id = ExternalId::new("external", "attachment-file").unwrap();
            let item = ItemIngressCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session_id.clone(),
                turn_id: prepared.turn_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                external_item_id: external_item_id.clone(),
                activity_id: None,
                payload,
                payload_digest,
                lifecycle: ItemLifecycle::Completed,
                tool_state: None,
                now_millis: 12,
            };
            let pair = FilePairCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session_id.clone(),
                turn_id: prepared.turn_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                external_item_id,
                old_text: OLD.into(),
                new_text: NEW.into(),
                now_millis: 12,
            };
            apply_adapter_inbound_batch(
                &mut connection,
                AdapterInboundBatchCommand::new(
                    batch_owner,
                    4096,
                    4096,
                    vec![
                        AdapterInboundEvent::Turn {
                            state: TurnState::Running,
                            now_millis: 12,
                        },
                        AdapterInboundEvent::Item(item),
                        AdapterInboundEvent::FilePair(pair),
                        AdapterInboundEvent::Turn {
                            state: TurnState::Completed,
                            now_millis: 13,
                        },
                    ],
                )
                .unwrap(),
            )
            .unwrap();
            let item_id: String = connection
                .query_row("SELECT item_id FROM acp_file_change_snapshots", [], |row| {
                    row.get(0)
                })
                .unwrap();
            serde_json::from_value(serde_json::json!({
                "kind":"diffLineComment", "diffRef": {
                    "projectId":project_id.as_str(), "chatId":chat_id.as_str(), "sessionId":session_id.as_str(),
                    "runtimeEpoch":"1", "turnId":prepared.turn_id.as_str(), "itemId":item_id,
                    "diffOrdinal":0, "snapshotRevision":"1", "rootId":root_id, "relativePath":"src/review.ts",
                    "contentDigest":pair_digest(OLD.as_bytes(), NEW.as_bytes())
                }, "side":"new", "startLine":"1", "endLine":"1", "note":"Review this guard"
            })).unwrap()
        };
        Self {
            state,
            owner,
            context,
            root,
        }
    }

    fn request(&self, new_session: bool) -> SendAcpTurnRequest {
        let page = self
            .state
            .acp
            .load_chat_page(
                self.owner.project_id.clone(),
                self.owner.chat_id.clone(),
                None,
            )
            .unwrap();
        let attached_context =
            AttachedContexts::new("context", vec![self.context.clone()]).unwrap();
        if new_session {
            SendAcpTurnRequest::NewSession {
                project_id: self.owner.project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: self.owner.chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("binding", "codex-app-server").unwrap(),
                model_id: None,
                prompt: Prompt::new("prompt", "Review safely").unwrap(),
                attached_context,
            }
        } else {
            SendAcpTurnRequest::Session {
                project_id: self.owner.project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: self.owner.chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: self.owner.session_id.clone(),
                runtime_epoch: self.owner.runtime_epoch,
                prompt: Prompt::new("prompt", "Review safely").unwrap(),
                attached_context,
            }
        }
    }

    fn install_host(&self) -> HostObservations {
        self.install_host_with_failure(false)
    }

    fn install_host_with_failure(&self, fail_dispatch: bool) -> HostObservations {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let host = Arc::new(
            RuntimeHost::start(CommandHostEngine {
                database: self.state.database.clone(),
                owner: self.owner.clone(),
                calls: calls.clone(),
                turn_id: None,
                prompts: Some(prompts.clone()),
                fail_dispatch,
            })
            .unwrap(),
        );
        host.activate(self.owner.clone()).unwrap();
        self.state
            .acp
            .install_runtime_for_test(self.owner.clone(), host);
        (calls, prompts)
    }

    fn counts(&self) -> (i64, i64) {
        self.state.database.connection().query_row("SELECT (SELECT COUNT(*) FROM turns),(SELECT COUNT(*) FROM items WHERE kind='text' AND external_item_id IS NULL AND ordinal=0)", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }
}

#[test]
fn attached_context_rejects_prior_session_for_both_send_targets_before_dispatch() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut fixture = Fixture::new().await;
        {
            let mut connection = fixture.state.database.connection();
            connection.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=14 WHERE session_id=?1", [fixture.owner.session_id.as_str()]).unwrap();
            apply_session_terminal(&mut connection, SessionTerminalCommand {
                project_id: fixture.owner.project_id.clone(), chat_id: fixture.owner.chat_id.clone(), session_id: fixture.owner.session_id.clone(), terminal: SessionState::Failed, now_millis: 14,
            }).unwrap();
        }
        let page = fixture.state.acp.load_chat_page(fixture.owner.project_id.clone(), fixture.owner.chat_id.clone(), None).unwrap();
        let binding = fixture.state.acp.codex_registration().refresh().unwrap();
        fixture.owner.session_id = crate::acp::store::start_session(&mut fixture.state.database.connection(), crate::acp::store::StartSessionCommand {
            project_id: fixture.owner.project_id.clone(), chat_id: fixture.owner.chat_id.clone(), expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(), binding_id: BindingId::new("binding", "codex-app-server").unwrap(),
            binding_digest: binding.binding_digest_hex().to_owned(), harness_version: None, model_id: None, now_millis: 15,
        }).unwrap().session_id;
        let (owner, calls) = install_command_host(&fixture.state, &fixture.owner.project_id, &fixture.owner.chat_id, &fixture.owner.session_id, CommandHostMode::Idle);
        fixture.owner = owner;
        let before = fixture.counts();
        for new_session in [true, false] {
            let failure = send_acp_turn_inner(&fixture.state, fixture.request(new_session)).await.unwrap_err();
            assert!(matches!(failure, SendAcpTurnFailure { failure: AcpIpcError::Conflict { resource: AcpConflictResource::Session, .. }, prompt_dispatch: PromptDispatch::NotStarted }), "{failure:?}");
            assert_eq!(fixture.counts(), before);
            assert!(calls.lock().unwrap().is_empty());
        }
    });
}

#[test]
fn attached_context_current_head_old_epoch_matches_dispatched_and_durable_prompt() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut fixture = Fixture::new().await;
        {
            let connection = fixture.state.database.connection();
            connection.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=14 WHERE session_id=?1", [fixture.owner.session_id.as_str()]).unwrap();
            connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) SELECT project_id,chat_id,session_id,2,'running',artifact_digest,15 FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=1", [fixture.owner.session_id.as_str()]).unwrap();
        }
        fixture.owner.runtime_epoch = RuntimeEpoch::new(2).unwrap();
        let (calls, prompts) = fixture.install_host();
        let response = send_acp_turn_inner(&fixture.state, fixture.request(false)).await.unwrap();
        let captured = prompts.lock().unwrap()[0].clone();
        let persisted = response.projection.turns.as_slice().iter().flat_map(|turn| turn.items.as_slice()).find_map(|item| match &item.content {
            ItemContent::Text { role: TextRole::User, text } if text.as_str().starts_with("Review safely") => Some(text.as_str()), _ => None,
        }).unwrap();
        assert_eq!(captured, persisted);
        assert_eq!(captured, "Review safely\n\nAttached diff comments (review context only; do not edit implicitly):\n- src/review.ts:1 (new side): Review this guard");
        for forbidden in [OLD.trim(), NEW.trim(), &fixture.context.diff_ref.content_digest, &fixture.root.to_string_lossy()] { assert!(!captured.contains(forbidden)); }
        assert_eq!(*calls.lock().unwrap(), vec!["prepare", "dispatch"]);
    });
}

#[test]
fn attached_context_corrupt_and_out_of_range_fail_without_dispatch_or_user_item() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for corrupt in [false, true] {
            let mut fixture = Fixture::new().await;
            if corrupt {
                fixture.state.database.connection().execute_batch("DROP TRIGGER deny_file_change_snapshot_mutation; UPDATE acp_file_change_snapshots SET new_blob=zeroblob(new_bytes);").unwrap();
            } else { fixture.context.end_line = 2; }
            let (calls, _) = fixture.install_host();
            let before = fixture.counts();
            let failure = send_acp_turn_inner(&fixture.state, fixture.request(false)).await.unwrap_err();
            assert_eq!(failure.prompt_dispatch, PromptDispatch::NotStarted);
            assert!(if corrupt { matches!(failure.failure, AcpIpcError::RecoveryRequired { .. }) } else { matches!(failure.failure, AcpIpcError::InvalidRequest { .. }) });
            assert_eq!(fixture.counts(), before);
            assert!(calls.lock().unwrap().is_empty());
        }
    });
}

#[test]
fn attached_context_validates_the_selected_old_and_new_side_at_command_admission() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut fixture = Fixture::new().await;
        let (calls, _) = fixture.install_host();
        let before = fixture.counts();
        fixture.context.start_line = 2;
        fixture.context.end_line = 2;
        let failure = send_acp_turn_inner(&fixture.state, fixture.request(false))
            .await
            .unwrap_err();
        assert!(matches!(
            failure.failure,
            AcpIpcError::InvalidRequest { .. }
        ));
        assert_eq!(failure.prompt_dispatch, PromptDispatch::NotStarted);
        assert_eq!(fixture.counts(), before);
        assert!(calls.lock().unwrap().is_empty());
        fixture.context.side = crate::acp::attached_context::DiffLineSide::Old;
        send_acp_turn_inner(&fixture.state, fixture.request(false))
            .await
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["prepare", "dispatch"]);
    });
}

#[test]
fn attached_context_failure_after_runtime_invocation_has_no_retry_proof() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new().await;
        let (calls, _) = fixture.install_host_with_failure(true);
        let failure = send_acp_turn_inner(&fixture.state, fixture.request(false))
            .await
            .unwrap_err();
        assert_eq!(failure.prompt_dispatch, PromptDispatch::Unknown);
        assert_eq!(*calls.lock().unwrap(), vec!["prepare", "dispatch"]);
    });
}

#[test]
fn attached_context_missing_runtime_and_binding_drift_prove_non_dispatch() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for drift in [false, true] {
            let fixture = Fixture::new().await;
            if drift {
                fixture
                    .state
                    .acp
                    .codex_registration()
                    .induce_drift_for_test();
            }
            let before = fixture.counts();
            let failure = send_acp_turn_inner(&fixture.state, fixture.request(false))
                .await
                .unwrap_err();
            assert_eq!(
                failure.prompt_dispatch,
                PromptDispatch::NotStarted,
                "{failure:?}"
            );
            assert_eq!(fixture.counts(), before);
        }
    });
}
