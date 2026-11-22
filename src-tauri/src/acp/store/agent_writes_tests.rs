use super::*;
use crate::acp::store::turn_undo::fixture::Fixture;
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageRootId, SageTurnId},
    intent::ExternalId,
};

const TURN: &str = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964ff";

fn recorded() -> Fixture {
    let fixture = Fixture::new();
    fixture.turn(TURN, 0);
    fixture
}

/// One adapter-batch transaction recording one write.
fn write(fixture: &Fixture, path: &str, digest: Option<[u8; 32]>) -> bool {
    let owner = AdapterStoreOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
        turn_id: SageTurnId::parse(TURN).unwrap(),
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

fn last(fixture: &Fixture) -> Option<LastWrites> {
    last_writes(&fixture.database.connection(), "session-a").unwrap()
}

#[test]
fn a_file_keeps_one_record_per_turn_until_its_bytes_change() {
    let fixture = recorded();
    assert!(write(&fixture, "src/a.rs", Some([1; 32])));
    assert!(!write(&fixture, "src/a.rs", Some([1; 32])));
    assert!(write(&fixture, "src/a.rs", None));
    assert!(!write(&fixture, "src/a.rs", None));
    assert!(write(&fixture, "src/a.rs", Some([2; 32])));
    let key = ("root-one".to_owned(), "src/a.rs".to_owned());
    assert_eq!(last(&fixture).unwrap().get(&key), Some(&Some([2; 32])));
    assert!(write(&fixture, "src/a.rs", None));
    assert_eq!(last(&fixture).unwrap().get(&key), Some(&None));
}

#[test]
fn a_session_at_its_cap_records_nothing_more_and_verifies_nothing() {
    let fixture = recorded();
    for index in 0..SESSION_WRITES {
        assert!(write(&fixture, &format!("f/{index}.rs"), Some([3; 32])));
    }
    assert!(!write(&fixture, "f/late.rs", Some([3; 32])));
    assert_eq!(last(&fixture), None);
}

#[test]
fn a_full_session_fact_store_skips_the_record_and_the_batch_still_applies() {
    let fixture = recorded();
    fixture
        .database
        .connection()
        .execute(
            "UPDATE acp_session_usage SET fact_count=?1 WHERE session_id='session-a'",
            [sage_core::limits::LIMITS.durable.facts_per_session as i64],
        )
        .unwrap();
    assert!(!write(&fixture, "src/a.rs", Some([1; 32])));
    assert!(last(&fixture).unwrap().is_empty());
}
