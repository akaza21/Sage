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
        if message == crate::repository_mutation_guard::REPOSITORY_BUSY));
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
                if message == crate::repository_mutation_guard::REPOSITORY_BUSY));
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
