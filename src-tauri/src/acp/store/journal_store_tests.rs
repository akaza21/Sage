use super::{StoreError, StoreMutation, StoreMutationResult, commit_batch, extended_tests::*};

#[test]
fn standalone_mutation_dispatches_once_and_rejects_mixed_batch_before_write() {
    let mut connection = setup();
    let commit = commit_batch(
        &mut connection,
        &[StoreMutation::StartSession(start(0, "journal-start"))],
    )
    .unwrap();
    assert!(matches!(
        commit.results.as_slice(),
        [StoreMutationResult::Session(session)]
            if session.disposition == super::MutationDisposition::Applied
    ));

    let mixed = [
        StoreMutation::StartSession(start(1, "journal-second")),
        StoreMutation::RecoverAfterRestart { now_millis: 20 },
    ];
    assert!(matches!(
        commit_batch(&mut connection, &mixed),
        Err(StoreError::BatchShape)
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
