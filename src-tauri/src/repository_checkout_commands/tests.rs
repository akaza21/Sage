use std::{fs, path::Path};

use super::*;
use crate::{
    acp::{store::*, transcript_quote::Prompt},
    project_registry::create::ProjectCreateStore,
};
use sage_acp_protocol::{
    contract::*,
    intent::BindingId,
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
        // Seed an admitted, ready session; the turn itself uses the production
        // journal store mutations below, including queued/send/terminal states.
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
        state.database.connection().execute_batch(
            "UPDATE agent_sessions SET state='ready',residency='hot';
             INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
             SELECT project_id,chat_id,id,1,'running',2 FROM agent_sessions;",
        ).unwrap();
        let command = PrepareTurnCommand {
            project_id: SageProjectId::parse(&a.id).unwrap(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: SageChatId::parse(chat).unwrap(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            expected_projection_revision: started.projection_revision,
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

#[tokio::test]
async fn checkout_holds_turn_start_authority_through_the_git_mutation() {
    let mut fixture = Fixture::new();
    // Start and checkout both belong to the active project in this ordering.
    fixture.request.project_id = fixture.command.project_id.as_str().into();
    fixture.request.root_id = fixture
        .state
        .database
        .root_records(&fixture.request.project_id)
        .unwrap()[0]
        .id
        .clone();
    fixture
        .state
        .database
        .connection()
        .execute(
            "UPDATE app_state SET active_project_id=?1",
            [&fixture.request.project_id],
        )
        .unwrap();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let checkout =
        checkout_git_local_branch_with(&fixture.state, fixture.request.clone(), 0, async {
            entered.send(()).unwrap();
            released.await.unwrap();
        });
    let start = async {
        waiting.await.unwrap();
        let project = SageProjectId::parse(&fixture.request.project_id).unwrap();
        let mut authority = std::pin::pin!(
            fixture
                .state
                .lock_active_project(&project, ProjectGeneration::ZERO,)
        );
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(authority.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(fixture.branch(), "main");
        release.send(()).unwrap();
        let _authority = authority.await.unwrap();
        assert_eq!(fixture.branch(), "topic");
        fixture.prepare()
    };
    let (result, _) = tokio::join!(checkout, start);
    assert!(result.unwrap().error.is_none());
    let refused = checkout_git_local_branch_inner(
        &fixture.state,
        GitRepositoryCheckoutRequest {
            branch: "main".into(),
            ..fixture.request.clone()
        },
        0,
    )
    .await;
    assert!(refused.is_err());
    assert_eq!(fixture.branch(), "topic");
}

#[tokio::test]
async fn repository_identity_covers_subdirectories_and_linked_worktrees_but_not_other_repositories()
{
    let fixture = Fixture::new();
    fixture.prepare();
    let root = fixture.base.join("repository");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let linked = fixture.base.join("linked");
    let linked_text = linked.to_str().unwrap();
    git(
        &root,
        &["worktree", "add", "-q", "-b", "linked", linked_text, "main"],
    );
    let other = fixture.base.join("other");
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    let active_root = fixture
        .state
        .database
        .root_records(fixture.command.project_id.as_str())
        .unwrap()[0]
        .id
        .clone();
    for (path, blocked) in [(&nested, true), (&linked, true), (&other, false)] {
        fixture
            .state
            .database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?1 WHERE id=?2",
                rusqlite::params![path.canonicalize().unwrap().to_str().unwrap(), active_root],
            )
            .unwrap();
        let result =
            checkout_git_local_branch_inner(&fixture.state, fixture.request.clone(), 0).await;
        if blocked {
            assert!(matches!(result, Err(AppError::InvalidRequest(message))
                if message == "Can't switch branches while an agent is working in this repository."));
            assert_eq!(fixture.branch(), "main");
        } else {
            assert!(result.unwrap().error.is_none());
            assert_eq!(fixture.branch(), "topic");
        }
    }
}
