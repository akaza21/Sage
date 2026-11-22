use super::*;
use crate::acp::store::turn_undo::fixture::Fixture;
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageRootId, SageTurnId},
    intent::ExternalId,
};

const FIRST: &str = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964ff";
const SECOND: &str = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964fe";

fn recorded() -> Fixture {
    let fixture = Fixture::new();
    fixture.turn(FIRST, 0);
    fixture.turn(SECOND, 1);
    fixture
}

/// One adapter-batch transaction recording one write in `turn`.
fn write(fixture: &Fixture, turn: &str, path: &str, digest: Option<[u8; 32]>) -> bool {
    let owner = AdapterStoreOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
        turn_id: SageTurnId::parse(turn).unwrap(),
        binding_digest: "c2".repeat(32),
        external_session_id: ExternalId::new("externalSessionId", "thread-a").unwrap(),
    };
    let resource = SafeResourceRef::new(SageRootId::parse("root-one").unwrap(), path).unwrap();
    let mut connection = fixture.database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    let applied = record(&transaction, &owner, &resource, digest.as_ref(), 1).unwrap();
    transaction.commit().unwrap();
    applied
}

fn last(fixture: &Fixture) -> LastWrites {
    last_writes(&fixture.database.connection(), "session-a").unwrap()
}

fn rows(fixture: &Fixture) -> i64 {
    fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM acp_agent_file_writes", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn a_file_keeps_one_row_holding_the_agents_latest_write() {
    let fixture = recorded();
    let key = ("root-one".to_owned(), "src/a.rs".to_owned());
    assert!(write(&fixture, FIRST, "src/a.rs", Some([1; 32])));
    assert!(!write(&fixture, FIRST, "src/a.rs", Some([1; 32])));
    assert!(write(&fixture, FIRST, "src/a.rs", None));
    assert_eq!(last(&fixture).get(&key), Some(&None));
    assert!(write(&fixture, SECOND, "src/a.rs", Some([2; 32])));
    assert_eq!(last(&fixture).get(&key), Some(&Some([2; 32])));
    assert!(write(&fixture, SECOND, "src/b.rs", Some([3; 32])));
    assert_eq!(rows(&fixture), 2);
}

#[test]
fn a_write_from_a_cut_turn_verifies_nothing_and_keeps_no_older_write() {
    let fixture = recorded();
    assert!(write(&fixture, FIRST, "src/a.rs", Some([1; 32])));
    assert!(write(&fixture, SECOND, "src/a.rs", Some([2; 32])));
    fixture.undo(SECOND, false);
    assert!(last(&fixture).is_empty());
}

#[test]
fn recording_stops_short_of_the_session_fact_reserve_and_the_batch_still_applies() {
    let fixture = recorded();
    let reserve = sage_core::limits::LIMITS.durable.facts_per_session * HEADROOM_PERCENT / 100;
    fixture
        .database
        .connection()
        .execute(
            "UPDATE acp_session_usage SET fact_count=?1 WHERE session_id='session-a'",
            [reserve as i64],
        )
        .unwrap();
    assert!(!write(&fixture, FIRST, "src/a.rs", Some([1; 32])));
    assert!(last(&fixture).is_empty());
}
