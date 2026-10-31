use super::{
    external_fixture_adapters::{ExternalCase, hex, send_started},
    external_fixture_runtime::{
        ExternalPrepared, prepare_external_case, prepare_external_environment,
    },
    external_fixture_support::{FIXTURE_BINDING_DIGEST, FixtureEnvironment, TempRoot},
    test_support::{FixtureDeadline, JournalCleanup},
};
use crate::{
    acp::{
        context_handoff::HandoffSummary,
        contract::{ClientIntentId, RequiredText, SageChatId},
        recovery::recover_runtime_after_zero_process,
        runtime::{JournalHandle, LiveWrite, SessionCoordinator},
        scheduler::Scheduler,
        store::{
            DatabaseAcpStore, RecoverRuntimeCommand, SendObservation, StartSessionCommand,
            context_handoff, load_chat_page,
            session_replace::{ReplaceSessionCommand, replace_session},
        },
    },
    persistence::Database,
};

pub(super) struct HandoffJourney {
    prepared: Option<ExternalPrepared>,
    deadline: FixtureDeadline,
}

impl HandoffJourney {
    pub(super) async fn from_environment(
        case: ExternalCase,
        deadline: FixtureDeadline,
        environment: FixtureEnvironment,
    ) -> Self {
        Self {
            prepared: Some(prepare_external_environment(case, deadline, environment).await),
            deadline,
        }
    }
    pub(super) async fn new() -> Self {
        let deadline = FixtureDeadline::new();
        Self {
            prepared: Some(prepare_external_case(ExternalCase::TurnTextSuccess, deadline).await),
            deadline,
        }
    }

    pub(super) fn session_id(&self) -> String {
        self.prepared().started.session_id.as_str().to_owned()
    }

    pub(super) fn summary(&self) -> HandoffSummary {
        context_handoff::summaries(
            &self.prepared().database.connection(),
            self.prepared().project_id.as_str(),
            self.prepared().chat_id.as_str(),
        )
        .unwrap()
        .into_iter()
        .find(|summary| summary.target_session == self.session_id())
        .unwrap()
    }

    pub(super) fn plan(&self) -> (String, String) {
        self.prepared()
            .database
            .connection()
            .query_row(
                "SELECT plan_json,digest FROM acp_context_handoffs WHERE target_session=?1",
                [self.session_id()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    pub(super) fn send_count(&self) -> i64 {
        send_started(
            &self.prepared().database,
            &self.prepared().started.session_id,
        )
    }

    pub(super) fn native_epoch_count(&self) -> i64 {
        self.prepared()
            .database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM session_runtime_epochs WHERE project_id=?1 AND chat_id=?2",
                rusqlite::params![
                    self.prepared().project_id.as_str(),
                    self.prepared().chat_id.as_str()
                ],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub(super) fn original_user_text(&self) -> String {
        self.prepared().database.connection().query_row(
            "SELECT json_extract(payload_json,'$.payload.text') FROM items WHERE session_id=?1 AND kind='text' ORDER BY rowid LIMIT 1",
            [self.session_id()], |row|row.get(0)).unwrap()
    }

    fn prepared(&self) -> &ExternalPrepared {
        self.prepared.as_ref().unwrap()
    }

    pub(super) async fn dispatch(&mut self, observation: SendObservation) -> LiveWrite {
        let prepared = self.prepared.take().unwrap();
        let (active, write) = tokio::time::timeout(
            self.deadline.operation_before_cleanup(),
            prepared.active.dispatch_once(prepared.now_millis),
        )
        .await
        .expect("dispatch deadline")
        .unwrap_or_else(|failure| panic!("dispatch: {:?}", failure.error));
        let active = tokio::time::timeout(
            self.deadline.operation_before_cleanup(),
            active.observe(observation, prepared.now_millis + 1),
        )
        .await
        .expect("observation deadline")
        .unwrap_or_else(|failure| panic!("observation: {:?}", failure.error));
        self.prepared = Some(ExternalPrepared {
            active,
            now_millis: prepared.now_millis + 2,
            ..prepared
        });
        write
    }

    pub(super) async fn complete(&mut self) {
        assert_eq!(
            self.dispatch(SendObservation::CompleteBytes).await,
            LiveWrite::Complete
        );
        let mailbox = self.prepared().mailbox.clone();
        for _ in 0..8 {
            if self
                .prepared()
                .active
                .turn
                .driver
                .latest_durable_turn_cursor()
                .is_some_and(|cursor| cursor.state().is_terminal())
            {
                break;
            }
            tokio::time::timeout(
                self.deadline.tree_before_cleanup(),
                self.prepared
                    .as_mut()
                    .unwrap()
                    .active
                    .turn
                    .driver
                    .run_next(mailbox.as_ref()),
            )
            .await
            .expect("handoff terminal deadline")
            .unwrap();
        }
        assert!(
            self.prepared()
                .active
                .turn
                .driver
                .latest_durable_turn_cursor()
                .is_some_and(|cursor| cursor.state().is_terminal())
        );
        self.prepared.as_mut().unwrap().process_cleanup.wait_zero();
    }

    pub(super) async fn switch(mut self, binding: &str, case: ExternalCase) -> Self {
        self.prepared.as_mut().unwrap().process_cleanup.wait_zero();
        self.recover();
        let ExternalPrepared {
            database,
            database_path,
            project_id,
            chat_id,
            now_millis,
            publisher,
            journal,
            journal_cleanup,
            temp_root,
            active,
            process_cleanup,
            ..
        } = self.prepared.take().unwrap();
        drop(active);
        drop(process_cleanup);
        let page = load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let started = replace_session(
            &mut database.connection(),
            ReplaceSessionCommand {
                confirm_unknown_delivery: false,
                start: StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::parse(format!(
                        "switch-{}",
                        page.projection_revision.get()
                    ))
                    .unwrap(),
                    binding_id: RequiredText::new("binding", binding).unwrap(),
                    binding_digest: hex(&FIXTURE_BINDING_DIGEST),
                    harness_version: None,
                    model_id: None,
                    now_millis,
                },
                expected_head: page.current_session_id,
            },
        )
        .unwrap();
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let environment = FixtureEnvironment {
            project_root: database_path.parent().unwrap().join("project"),
            database_path,
            database,
            project_id,
            chat_id,
            now_millis,
            publisher,
            journal,
            journal_cleanup,
            scheduler,
            sessions,
            started,
            temp_root,
        };
        Self {
            prepared: Some(prepare_external_environment(case, self.deadline, environment).await),
            deadline: self.deadline,
        }
    }

    fn recover(&self) {
        recover_runtime_after_zero_process(
            &mut self.prepared().database.connection(),
            RecoverRuntimeCommand {
                project_id: self.prepared().project_id.clone(),
                chat_id: self.prepared().chat_id.clone(),
                session_id: self.prepared().started.session_id.clone(),
                runtime_epoch: self.prepared().epoch,
                now_millis: self.prepared().now_millis + 1,
            },
        )
        .unwrap();
    }

    pub(super) async fn crash_reopen(&mut self) {
        self.prepared.as_mut().unwrap().process_cleanup.wait_zero();
        let reopened = Database::open(&self.prepared().database_path).unwrap();
        self.prepared.as_mut().unwrap().database = reopened;
        self.recover();
    }

    pub(super) fn assert_resend_refused(&self) {
        let mut command = self.prepared().prompt_intent.command.clone();
        assert!(matches!(
            context_handoff::freeze_prompt(
                &mut self.prepared().database.connection(),
                &mut command
            ),
            Err(crate::acp::store::StoreError::Conflict {
                resource: "handoffDeliveryUnknown"
            })
        ));
        let prepared = self.prepared();
        let page = load_chat_page(
            &prepared.database.connection(),
            prepared.project_id.clone(),
            prepared.chat_id.clone(),
            None,
        )
        .unwrap();
        assert!(matches!(
            replace_session(
                &mut prepared.database.connection(),
                ReplaceSessionCommand {
                    expected_head: page.current_session_id,
                    confirm_unknown_delivery: false,
                    start: StartSessionCommand {
                        project_id: prepared.project_id.clone(),
                        chat_id: prepared.chat_id.clone(),
                        expected_projection_revision: page.projection_revision,
                        client_intent_id: ClientIntentId::parse("unconfirmed-retry").unwrap(),
                        binding_id: RequiredText::new("binding", "binding-b").unwrap(),
                        binding_digest: hex(&FIXTURE_BINDING_DIGEST),
                        harness_version: None,
                        model_id: None,
                        now_millis: prepared.now_millis,
                    },
                }
            ),
            Err(crate::acp::store::StoreError::Conflict {
                resource: "handoffDeliveryUnknown"
            })
        ));
    }

    pub(super) async fn other_lane(&self) -> Self {
        let database = self.prepared().database.clone();
        let chat_id = SageChatId::parse("fixture-other").unwrap();
        database.connection().execute("INSERT INTO chats(id,project_id,title,ordinal,created_at,updated_at) VALUES(?1,?2,'Other',2,1,1)",
            rusqlite::params![chat_id.as_str(),self.prepared().project_id.as_str()]).unwrap();
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let journal_cleanup =
            JournalCleanup::spawn(self.deadline, journal.clone(), runner).unwrap();
        let scheduler = Scheduler::new();
        let sessions = SessionCoordinator::new(
            journal.clone(),
            scheduler.clone(),
            self.prepared().publisher.clone(),
        );
        let page = load_chat_page(
            &database.connection(),
            self.prepared().project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: self.prepared().project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::parse("other-start").unwrap(),
                binding_id: RequiredText::new("binding", "fixture-binding").unwrap(),
                binding_digest: hex(&FIXTURE_BINDING_DIGEST),
                harness_version: None,
                model_id: None,
                now_millis: self.prepared().now_millis,
            })
            .await
            .unwrap();
        let temp_root = TempRoot::new("handoff-other");
        std::fs::create_dir_all(temp_root.path()).unwrap();
        let environment = FixtureEnvironment {
            project_root: self
                .prepared()
                .database_path
                .parent()
                .unwrap()
                .join("project"),
            database_path: self.prepared().database_path.clone(),
            database,
            project_id: self.prepared().project_id.clone(),
            chat_id,
            now_millis: self.prepared().now_millis,
            publisher: self.prepared().publisher.clone(),
            journal,
            journal_cleanup,
            scheduler,
            sessions,
            started,
            temp_root,
        };
        Self {
            prepared: Some(
                prepare_external_environment(
                    ExternalCase::TurnTextSuccess,
                    self.deadline,
                    environment,
                )
                .await,
            ),
            deadline: self.deadline,
        }
    }

    pub(super) async fn finish(mut self) {
        self.prepared.as_mut().unwrap().process_cleanup.wait_zero();
        self.recover();
        let ExternalPrepared {
            active,
            journal,
            journal_cleanup,
            temp_root,
            database,
            publisher,
            mailbox,
            scheduler,
            process_cleanup,
            ..
        } = self.prepared.take().unwrap();
        drop(active);
        journal_cleanup.finish().unwrap();
        drop(journal);
        drop(publisher);
        drop(mailbox);
        drop(scheduler);
        drop(process_cleanup);
        drop(database);
        temp_root.remove();
    }
}
