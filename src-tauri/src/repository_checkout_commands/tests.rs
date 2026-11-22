use std::{fs, path::Path};

use super::*;
use crate::{
    acp::{store::*, transcript_quote::Prompt},
    project_registry::create::ProjectCreateStore,
};
use sage_acp_protocol::{
    contract::*,
    intent::{BindingId, CapabilitySnapshotV1, ExternalId},
    lifecycle::{DeliveryState, TurnState},
};

struct Fixture {
    base: std::path::PathBuf,
    state: AppState,
    command: PrepareTurnCommand,
    request: GitRepositoryCheckoutRequest,
}

impl Fixture {
    fn new() -> Self {
        let base = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-checkout-{}", uuid::Uuid::new_v4()));
        let root = base.join("repository");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Sage Test",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        );
        git(&root, &["branch", "topic"]);
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let a = database.create_project_with_chat(&root).unwrap();
        let chat: String = database
            .connection()
            .query_row("SELECT id FROM chats WHERE project_id=?1", [&a.id], |r| {
                r.get(0)
            })
            .unwrap();
        let b = database.create_project_with_chat(&root).unwrap();
        database
            .connection()
            .execute("UPDATE app_state SET active_project_id=?1", [&b.id])
            .unwrap();
        let state = AppState::new(database);
        // Negotiate a ready session through the native store before opening
        // turns through the production journal's database implementation.
        let started = start_session(
            &mut state.database.connection(),
            StartSessionCommand {
                project_id: SageProjectId::parse(&a.id).unwrap(),
                chat_id: SageChatId::parse(&chat).unwrap(),
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("binding", "fixture").unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
        )
        .unwrap();
        activate_session(
            &mut state.database.connection(),
            ActivateSessionCommand {
                project_id: SageProjectId::parse(&a.id).unwrap(),
                chat_id: SageChatId::parse(&chat).unwrap(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                artifact_digest: "b".repeat(64),
                now_millis: 2,
            },
        )
        .unwrap();
        let capabilities: String = state
            .database
            .connection()
            .query_row(
                "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut capabilities: CapabilitySnapshotV1 = serde_json::from_str(&capabilities).unwrap();
        capabilities.protocol_version = RequiredText::new("protocol", "fixture").unwrap();
        capabilities.prompt = true;
        let ready = complete_activation(
            &mut state.database.connection(),
            CompleteActivationCommand {
                project_id: SageProjectId::parse(&a.id).unwrap(),
                chat_id: SageChatId::parse(&chat).unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                binding_digest: "a".repeat(64),
                artifact_digest: "b".repeat(64),
                external_session_id: ExternalId::new("external", "fixture-session").unwrap(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: 3,
            },
        )
        .unwrap();
        let command = PrepareTurnCommand {
            project_id: SageProjectId::parse(&a.id).unwrap(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: SageChatId::parse(chat).unwrap(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            expected_projection_revision: ready.projection_revision,
            client_intent_id: ClientIntentId::new(),
            prompt: Prompt::new("prompt", "work in this repository").unwrap(),
            now_millis: 3,
        };
        let request = GitRepositoryCheckoutRequest {
            project_id: b.id.clone(),
            root_id: b.roots[0].id.clone(),
            branch: "topic".into(),
        };
        Self {
            base,
            state,
            command,
            request,
        }
    }

    fn prepare(&self) -> TurnMutationCommand {
        let commit = DatabaseAcpStore::new(self.state.database.clone())
            .commit(&[StoreMutation::PrepareTurn(self.command.clone())])
            .unwrap();
        let [StoreMutationResult::Prepared(prepared)] = commit.results.as_slice() else {
            panic!("expected a durable queued turn")
        };
        TurnMutationCommand {
            project_id: self.command.project_id.clone(),
            chat_id: self.command.chat_id.clone(),
            session_id: self.command.session_id.clone(),
            runtime_epoch: self.command.runtime_epoch,
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 4,
        }
    }

    fn branch(&self) -> String {
        let output = sage_git::process::fixture_git()
            .arg("-C")
            .arg(self.base.join("repository"))
            .args(["branch", "--show-current"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        sage_git::process::fixture_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn shared_repository_refuses_queued_and_running_turns_until_terminal() {
    let fixture = Fixture::new();
    let mut turn = fixture.prepare();
    for running in [false, true] {
        if running {
            let mut connection = fixture.state.database.connection();
            turn.expected_revision = begin_send(&mut connection, turn.clone()).unwrap().revision;
            turn.expected_revision =
                record_send_observation(&mut connection, turn.clone(), SendObservation::Correlated)
                    .unwrap()
                    .revision;
        }
        let error = checkout_git_local_branch_inner(&fixture.state, fixture.request.clone(), 0)
            .await
            .unwrap_err();
        assert!(
            matches!(checkout_command_error(error), GitFileMutationCommandError::Failed { message }
            if message == "Can't switch branches while an agent is working in this repository.")
        );
        assert_eq!(fixture.branch(), "main");
    }
    apply_turn_terminal(
        &mut fixture.state.database.connection(),
        turn,
        TurnState::Completed,
        DeliveryState::Correlated,
    )
    .unwrap();
    let result = checkout_git_local_branch_inner(&fixture.state, fixture.request.clone(), 0)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(fixture.branch(), "topic");
}

impl Fixture {
    fn leased(&self, project_id: &str) -> bool {
        crate::repository_mutation_guard::project_leased(
            &self.state.database.connection(),
            project_id,
        )
        .unwrap()
    }

    fn harness_turn(&self) -> TurnMutationCommand {
        TurnMutationCommand {
            project_id: self.command.project_id.clone(),
            chat_id: self.command.chat_id.clone(),
            session_id: self.command.session_id.clone(),
            runtime_epoch: self.command.runtime_epoch,
            turn_id: SageTurnId::new(),
            expected_revision: EntityRevision::ZERO,
            now_millis: 4,
        }
    }

    fn set_active_root(&self, path: &str) {
        let root = self
            .state
            .database
            .root_records(self.command.project_id.as_str())
            .unwrap()[0]
            .id
            .clone();
        self.state
            .database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?1 WHERE id=?2",
                rusqlite::params![path, root],
            )
            .unwrap();
    }
}

#[tokio::test]
async fn turn_starts_are_refused_during_the_checkout_lease_without_holding_the_writer() {
    let fixture = Fixture::new();
    let project = fixture.command.project_id.as_str().to_owned();
    let result =
        checkout_git_local_branch_with(&fixture.state, fixture.request.clone(), 0, async {
            // Git runs with the store writer free: other writes are not stalled.
            let writer = fixture
                .state
                .database
                .connection_ownership_probe()
                .upgrade()
                .unwrap();
            assert!(
                writer.try_lock().is_ok(),
                "the lease must not hold the writer"
            );
            assert!(fixture.leased(&project));
            let mut store = DatabaseAcpStore::new(fixture.state.database.clone());
            for mutation in [
                StoreMutation::PrepareTurn(fixture.command.clone()),
                StoreMutation::OpenHarnessTurn(fixture.harness_turn()),
            ] {
                assert!(matches!(
                    store.commit(&[mutation]),
                    Err(StoreError::RepositoryBusy)
                ));
            }
            assert_eq!(fixture.branch(), "main");
        })
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(fixture.branch(), "topic");
    assert!(!fixture.leased(&project));
    let mut store = DatabaseAcpStore::new(fixture.state.database.clone());
    let opened = store
        .commit(&[StoreMutation::OpenHarnessTurn(fixture.harness_turn())])
        .unwrap();
    assert!(matches!(
        opened.results.as_slice(),
        [StoreMutationResult::Turn(turn)] if turn.state == TurnState::Running
    ));
    let refused = checkout_git_local_branch_inner(
        &fixture.state,
        GitRepositoryCheckoutRequest {
            branch: "main".into(),
            ..fixture.request.clone()
        },
        0,
    )
    .await;
    assert!(matches!(refused, Err(AppError::InvalidRequest(message))
        if message == busy()));
    assert_eq!(fixture.branch(), "topic");
}

#[tokio::test]
async fn the_lease_clears_after_a_failed_and_a_panicking_checkout() {
    let fixture = Fixture::new();
    let project = fixture.command.project_id.as_str().to_owned();
    // `main` is the current branch, so Git refuses the switch.
    let failed = checkout_git_local_branch_inner(
        &fixture.state,
        GitRepositoryCheckoutRequest {
            branch: "main".into(),
            ..fixture.request.clone()
        },
        0,
    )
    .await;
    assert!(failed.is_err());
    assert!(!fixture.leased(&project));
    let panicked = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(checkout_git_local_branch_with(
                        &fixture.state,
                        fixture.request.clone(),
                        0,
                        async { panic!("checkout worker panicked mid-lease") },
                    ))
            })
            .join()
    });
    assert!(panicked.is_err());
    assert!(!fixture.leased(&project));
    assert_eq!(fixture.branch(), "main");
    fixture.prepare();
}

#[tokio::test]
async fn repository_identity_covers_parents_subdirectories_and_linked_worktrees_only() {
    let fixture = Fixture::new();
    fixture.prepare();
    let root = fixture.base.join("repository");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let linked = fixture.base.join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
            "main",
        ],
    );
    let other = fixture.base.join("other");
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    // An agent at the parent folder (not a repository) reaches every file
    // the checkout rewrites.
    for (path, blocked) in [
        (&fixture.base, true),
        (&nested, true),
        (&linked, true),
        (&other, false),
    ] {
        fixture.set_active_root(path.canonicalize().unwrap().to_str().unwrap());
        let result =
            checkout_git_local_branch_inner(&fixture.state, fixture.request.clone(), 0).await;
        if blocked {
            assert!(matches!(result, Err(AppError::InvalidRequest(message))
                if message == busy()));
            assert_eq!(fixture.branch(), "main");
        } else {
            assert!(result.unwrap().error.is_none());
            assert_eq!(fixture.branch(), "topic");
        }
    }
}

#[tokio::test]
async fn an_unverifiable_busy_folder_is_named_and_an_idle_one_is_leased() {
    let fixture = Fixture::new();
    let missing = fixture.base.join("missing");
    fixture.set_active_root(missing.to_str().unwrap());
    let project = fixture.command.project_id.as_str().to_owned();
    let name: String = fixture
        .state
        .database
        .connection()
        .query_row("SELECT name FROM projects WHERE id=?1", [&project], |row| {
            row.get(0)
        })
        .unwrap();
    // Idle, the unverifiable Project is leased for the switch, not refused.
    let checkout =
        checkout_git_local_branch_with(&fixture.state, fixture.request.clone(), 0, async {
            assert!(fixture.leased(&project));
        })
        .await
        .unwrap();
    assert!(checkout.error.is_none());
    fixture.prepare();
    let refused = checkout_git_local_branch_inner(
        &fixture.state,
        GitRepositoryCheckoutRequest {
            branch: "main".into(),
            ..fixture.request.clone()
        },
        0,
    )
    .await
    .unwrap_err();
    let GitFileMutationCommandError::Failed { message } = checkout_command_error(refused) else {
        panic!("expected a refusal")
    };
    assert_eq!(
        message,
        format!(
            "Can't switch branches: Sage couldn't verify the folder of project \"{name}\", \
             where an agent is working. Reconnect or remove that folder, then retry."
        )
    );
    assert_eq!(fixture.branch(), "topic");
}

#[tokio::test]
async fn a_registry_beyond_its_root_limit_fails_closed() {
    let fixture = Fixture::new();
    let project = fixture.command.project_id.as_str();
    let limit =
        crate::project_registry::MAX_PROJECTS * crate::project_registry::MAX_ROOTS_PER_PROJECT;
    {
        let connection = fixture.state.database.connection();
        for index in 0..limit {
            connection
                .execute(
                    "INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                     VALUES(?1,?2,?3,?3,?4)",
                    rusqlite::params![
                        format!("overflow-{index}"),
                        project,
                        format!("/overflow/{index}"),
                        index + 100
                    ],
                )
                .unwrap();
        }
    }
    let refused = checkout_git_local_branch_inner(&fixture.state, fixture.request.clone(), 0)
        .await
        .unwrap_err();
    assert!(matches!(refused, AppError::ResourceLimit(message)
        if message == "Can't switch branches: the Project registry exceeds its safety limit."));
    assert_eq!(fixture.branch(), "main");
    assert!(!fixture.leased(project));
}

/// Per-test limit, the repo's shared test wait budget: a hang fails fast.
const TEST_LIMIT: std::time::Duration = sage_test_support::test_wait::BUDGET;

fn busy() -> String {
    crate::repository_mutation_guard::RepositoryOperation::SWITCH_BRANCHES.busy()
}

#[test]
fn git_that_may_still_run_keeps_the_lease_until_restart() {
    // Serialised like the other containment tests; the guard stays outside
    // the runtime so no lock is held across an await.
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::time::timeout(TEST_LIMIT, kept_lease_after_potentially_running_git())
            .await
            .expect("the containment test finishes within its limit")
    });
}

async fn kept_lease_after_potentially_running_git() {
    let fixture = Fixture::new();
    let project = fixture.command.project_id.as_str().to_owned();
    let target = fixture
        .state
        .database
        .root_records(&fixture.request.project_id)
        .unwrap()
        .remove(0);
    let workspace =
        Workspace::open_registered_root(&target.project_id, &target.id, &target.absolute_path)
            .unwrap();
    let root = fixture.base.join("repository");
    let (policy, reaper) =
        sage_git::process::GitProcessPolicy::potentially_running(std::time::Duration::ZERO);
    let error = crate::repository_mutation_guard::with_idle_repository(
        &fixture.state.database,
        &workspace,
        crate::repository_mutation_guard::RepositoryOperation::SWITCH_BRANCHES,
        || async {
            sage_git::run_git_with_policy(
                &root,
                &["rev-parse", "--is-inside-work-tree"],
                1_024,
                policy,
            )
            .await
            .map(drop)
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        AppError::GitContainment {
            state: sage_core::error::GitContainmentState::PotentiallyRunning,
            ..
        }
    ));
    assert!(
        fixture.leased(&project),
        "the lease outlives a Git that may still run"
    );
    assert!(matches!(
        DatabaseAcpStore::new(fixture.state.database.clone())
            .commit(&[StoreMutation::PrepareTurn(fixture.command.clone())]),
        Err(StoreError::RepositoryBusy)
    ));
    let GitFileMutationCommandError::Containment { message, .. } = checkout_command_error(error)
    else {
        panic!("expected a containment error")
    };
    assert!(message.ends_with(crate::repository_mutation_guard::LEASE_KEPT));
    reaper.release();
    reaper
        .wait_for_exit(sage_test_support::test_wait::BUDGET)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_prompt_sent_during_the_lease_is_refused_unsent_and_the_session_is_untouched() {
    let fixture = Fixture::new();
    let sessions = || -> (i64, i64) {
        fixture
            .state
            .database
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM turns),
                        (SELECT revision FROM agent_sessions WHERE id=?1)",
                [fixture.command.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    let before = sessions();
    let checkout =
        checkout_git_local_branch_with(&fixture.state, fixture.request.clone(), 0, async {
            let refused = crate::commands::acp::send_acp_turn_inner(
                &fixture.state,
                crate::acp::ipc::SendAcpTurnRequest::Session {
                    project_id: fixture.command.project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: fixture.command.chat_id.clone(),
                    expected_projection_revision: fixture.command.expected_projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: fixture.command.session_id.clone(),
                    runtime_epoch: fixture.command.runtime_epoch,
                    prompt: Prompt::new("prompt", "edit while the branch switches").unwrap(),
                    attached_context: Default::default(),
                    staged: None,
                },
            )
            .await;
            assert!(matches!(
                refused,
                Err(crate::acp::ipc::SendAcpTurnFailure {
                    failure: crate::acp::ipc::AcpIpcError::Rejected { message },
                    prompt_dispatch: crate::acp::ipc::PromptDispatch::NotStarted,
                }) if message.as_str() == crate::repository_mutation_guard::TURN_BLOCKED
            ));
        });
    tokio::time::timeout(TEST_LIMIT, checkout)
        .await
        .expect("the send test finishes within its limit")
        .unwrap();
    assert_eq!(sessions(), before);
    assert_eq!(fixture.branch(), "topic");
}

struct HarnessModeAuthority;

impl crate::acp::runtime::ProjectIntentAuthority for HarnessModeAuthority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<crate::acp::ipc::IntentSnapshot> {
        None
    }

    fn harness_turn_mode(
        &self,
        _: &sage_acp_protocol::scheduler::RuntimeOwner,
    ) -> Option<&'static str> {
        Some("default")
    }
}

/// Closes the journal on every exit path, a failed assertion included: the
/// runner is a blocking task the test runtime waits for when it shuts down.
struct ClosesJournal(crate::acp::runtime::JournalHandle);

impl Drop for ClosesJournal {
    fn drop(&mut self) {
        self.0.close_when_drained();
    }
}

#[tokio::test]
async fn a_harness_started_turn_during_the_lease_is_declined_and_the_session_stays_reusable() {
    use crate::acp::{
        bindings::adapter::HarnessTurnAuthority,
        runtime::{
            CoordinatorError, JournalError, JournalHandle, ProjectionMailbox, SendCoordinator,
        },
    };
    let fixture = Fixture::new();
    let (journal, runner) =
        JournalHandle::new(DatabaseAcpStore::new(fixture.state.database.clone()));
    let runner = runner.spawn();
    let closes = ClosesJournal(journal.clone());
    let coordinator = SendCoordinator::new(
        std::sync::Arc::new(HarnessModeAuthority),
        journal,
        std::sync::Arc::new(ProjectionMailbox::default()),
    );
    let scheduler = sage_acp_protocol::scheduler::Scheduler::new();
    let tree = scheduler
        .reserve_tree(sage_acp_protocol::scheduler::RuntimeOwner {
            project_id: fixture.command.project_id.clone(),
            chat_id: fixture.command.chat_id.clone(),
            session_id: fixture.command.session_id.clone(),
            runtime_epoch: fixture.command.runtime_epoch,
        })
        .unwrap();
    let mode = || Some(HarnessTurnAuthority::SessionMode("default"));
    // The journal drops commits past their deadline, so stamp real time.
    let now = sage_core::time::now_millis;
    tokio::time::timeout(TEST_LIMIT, async {
        checkout_git_local_branch_with(&fixture.state, fixture.request.clone(), 0, async {
            let Err(failure) = coordinator.open_harness_turn(&tree, mode(), now()).await else {
                panic!("a harness-started turn must not open during the lease")
            };
            // A plain refusal: nothing committed, so the runner declines the
            // run and keeps the session instead of entering recovery.
            assert!(failure.recovery.is_none());
            assert!(
                matches!(
                    failure.error,
                    CoordinatorError::Journal(JournalError::Store(StoreError::RepositoryBusy))
                ),
                "unexpected refusal: {:?}",
                failure.error
            );
            assert!(!scheduler.active().unwrap().turn_reserved);
        })
        .await
        .unwrap();
        let opened = coordinator
            .open_harness_turn(&tree, mode(), now())
            .await
            .unwrap_or_else(|failure| {
                panic!(
                    "the session takes the next harness turn: {:?}",
                    failure.error
                )
            });
        assert!(scheduler.active().unwrap().turn_reserved);
        drop(opened);
    })
    .await
    .expect("the harness lease test finishes within its limit");
    drop(closes);
    runner.await.unwrap();
}
