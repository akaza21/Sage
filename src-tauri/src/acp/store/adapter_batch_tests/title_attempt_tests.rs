use super::*;
use crate::acp::store::title_attempt::{self, TitleAttempt};
use sage_acp_protocol::scheduler::RuntimeOwner;

fn completed() -> Fixture {
    let mut fixture = migrated_fixture();
    fixture
        .connection
        .execute_batch(
            "INSERT INTO workbench_tabs(id,project_id,kind,title,position,created_at,updated_at)
           VALUES('title-tab','project-one','chat','Chat: Prompt fallback',0,1,1);
         INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
           VALUES('project-one','chat-one','title-tab',1);
         UPDATE chats SET title='Chat: Prompt fallback',title_source='prompt';",
        )
        .unwrap();
    let reply = item(&fixture.owner, "final-answer");
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Item(reply),
                AdapterInboundEvent::Turn {
                    state: TurnState::Completed,
                    now_millis: 7,
                },
            ],
        ),
    )
    .unwrap();
    fixture
}

#[cfg(feature = "acp-fixture")]
pub(crate) fn scheduler_fixture() -> (sage_db::database::Database, crate::acp::runtime::PumpIntent)
{
    let fixture = completed();
    let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    rusqlite::backup::Backup::new(&fixture.connection, &mut database.connection())
        .unwrap()
        .run_to_completion(100, std::time::Duration::ZERO, None)
        .unwrap();
    let owner = fixture.owner;
    let intent = crate::acp::runtime::PumpIntent {
        project_id: owner.project_id,
        chat_id: owner.chat_id,
        session_id: owner.session_id,
        external_session_id: owner.external_session_id,
        runtime_epoch: owner.runtime_epoch,
        binding_digest: [0xaa; 32],
        active_turn: crate::acp::runtime::ActiveTurnIntent::store_minted(owner.turn_id).unwrap(),
    };
    (database, intent)
}

fn claim(fixture: &mut Fixture) -> Option<TitleAttempt> {
    let owner = &fixture.owner;
    title_attempt::claim(
        &mut fixture.connection,
        RuntimeOwner {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
        },
        owner.turn_id.clone(),
    )
    .unwrap()
}

fn shown(fixture: &Fixture) -> (String, String) {
    fixture
        .connection
        .query_row(
            "SELECT title,title_source FROM chats WHERE id='chat-one'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn a_durable_claim_spends_one_attempt_across_duplicates_and_reopen() {
    let mut fixture = completed();
    let attempt = claim(&mut fixture).unwrap();
    assert_eq!(attempt.input, "User:\nprompt\nAssistant:\nadapter text");
    assert!(claim(&mut fixture).is_none());
    let mut reopened = rusqlite::Connection::open_in_memory().unwrap();
    rusqlite::backup::Backup::new(&fixture.connection, &mut reopened)
        .unwrap()
        .run_to_completion(100, std::time::Duration::ZERO, None)
        .unwrap();
    fixture.connection = reopened;
    assert!(claim(&mut fixture).is_none());
    assert!(
        title_attempt::apply(
            &mut fixture.connection,
            &attempt,
            "Investigate empty sidebar chats"
        )
        .unwrap()
        .is_some()
    );
    assert_eq!(
        shown(&fixture),
        (
            "Chat: Investigate empty sidebar chats".into(),
            "harness".into()
        )
    );
    assert!(
        title_attempt::apply(
            &mut fixture.connection,
            &attempt,
            "Another late generated title"
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn existing_metadata_accepts_short_issue_and_chinese_titles_but_generated_output_does_not() {
    for raw in ["Fix tests", "Fix issue #443", "修复测试失败"] {
        let mut fixture = completed();
        let attempt = claim(&mut fixture).unwrap();
        assert!(
            title_attempt::apply(&mut fixture.connection, &attempt, raw)
                .unwrap()
                .is_none()
        );
        assert!(
            title_attempt::apply_existing(&mut fixture.connection, &attempt, raw)
                .unwrap()
                .is_some()
        );
        assert_eq!(shown(&fixture), (format!("Chat: {raw}"), "harness".into()));
    }
}

#[test]
fn user_fork_and_useful_harness_titles_are_ineligible_and_win_result_races() {
    for source in ["user", "fork", "harness"] {
        let mut fixture = completed();
        let attempt = claim(&mut fixture).unwrap();
        fixture
            .connection
            .execute(
                "UPDATE chats SET title='Chat: Owner title',title_source=?1",
                [source],
            )
            .unwrap();
        assert!(
            title_attempt::apply(
                &mut fixture.connection,
                &attempt,
                "Investigate empty sidebar chats"
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(shown(&fixture), ("Chat: Owner title".into(), source.into()));
        assert!(claim(&mut fixture).is_none());
        let mut before = completed();
        before
            .connection
            .execute("UPDATE chats SET title_source=?1", [source])
            .unwrap();
        assert!(claim(&mut before).is_none());
    }
}

#[test]
fn undo_archive_session_replacement_and_wrong_token_reject_late_results() {
    for change in ["undo", "archive", "session", "token", "epoch"] {
        let mut fixture = completed();
        let mut attempt = claim(&mut fixture).unwrap();
        match change {
            "undo" => {
                let transaction = fixture.connection.transaction().unwrap();
                let revision =
                    facts::next_projection_revision(&transaction, "project-one", "chat-one")
                        .unwrap();
                facts::append_fact_payload(
                    &transaction,
                    "project-one",
                    "chat-one",
                    attempt.owner.session_id.as_str(),
                    Some(attempt.anchor.as_str()),
                    "turnUndone",
                    "{}",
                    revision,
                    8,
                )
                .unwrap();
                transaction.commit().unwrap();
            }
            "archive" => {
                fixture
                    .connection
                    .execute("DELETE FROM chat_tabs", [])
                    .unwrap();
            }
            "session" => {
                fixture
                    .connection
                    .execute("DELETE FROM chat_session_heads", [])
                    .unwrap();
            }
            "token" => {
                attempt.token = "foreign-token".into();
            }
            "epoch" => {
                attempt.owner.runtime_epoch = RuntimeEpoch::new(2).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            title_attempt::apply(
                &mut fixture.connection,
                &attempt,
                "Investigate empty sidebar chats"
            )
            .unwrap()
            .is_none(),
            "{change}"
        );
        assert_eq!(
            shown(&fixture),
            ("Chat: Prompt fallback".into(), "prompt".into())
        );
    }
}

#[test]
fn malformed_results_leave_the_local_title_and_only_prompt_and_final_text_are_sent() {
    let mut fixture = completed();
    let attempt = claim(&mut fixture).unwrap();
    for output in [
        "",
        "New conversation",
        "First title\nSecond title",
        "{\"title\":\"Useful generated title\"}",
    ] {
        assert!(
            title_attempt::apply(&mut fixture.connection, &attempt, output)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            shown(&fixture),
            ("Chat: Prompt fallback".into(), "prompt".into())
        );
    }
}
