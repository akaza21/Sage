use super::*;
use crate::acp::{
    contract::SageRootId,
    file_diff::{DIFF_SIDE_BYTES_MAX, pair_digest},
    intent::{
        FileChangeFacts, FileChangeOperation, SafeResourceLocation, SafeResourceRef,
        UnavailableResourceReason,
    },
    projection::{FileOperation, FilePairState},
};

mod codex_file_change_tests;
mod creation_promotion_tests;

fn snapshot_fixture() -> Fixture {
    let fixture = fixture();
    fixture
        .connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    fixture
}

fn pair_batch(
    owner: &AdapterStoreOwner,
    key: &str,
    old: &str,
    new: &str,
) -> AdapterInboundBatchCommand {
    let mut metadata = item(owner, key);
    metadata.payload = NormalizedPayload::file_admitted(
        crate::acp::intent::FileChangeFacts {
            resource: SafeResourceLocation::Resource {
                resource: SafeResourceRef::new(
                    SageRootId::parse("root-one").unwrap(),
                    "src/lib.rs",
                )
                .unwrap(),
            },
            summary: SafeText::new("summary", "File changed").unwrap(),
            additions: Some(1),
            deletions: Some(1),
            operation: crate::acp::intent::FileChangeOperation::Updated,
        },
        0,
        (old.len() as u64, new.len() as u64),
        pair_digest(old.as_bytes(), new.as_bytes()),
    );
    metadata.payload_digest =
        facts::hex_digest(serde_json::to_string(&metadata.payload).unwrap().as_bytes());
    let pair = FilePairCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: metadata.external_item_id.clone(),
        old_text: old.into(),
        new_text: new.into(),
        now_millis: 6,
    };
    let bytes = old.len() + new.len() + 1024;
    AdapterInboundBatchCommand::new(
        owner.clone(),
        bytes,
        bytes,
        vec![
            AdapterInboundEvent::Item(metadata),
            AdapterInboundEvent::FilePair(pair),
        ],
    )
    .unwrap()
}

fn pair_count(fixture: &Fixture) -> i64 {
    fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM acp_file_change_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn created_pair_batch(
    owner: &AdapterStoreOwner,
    key: &str,
    new: &str,
) -> AdapterInboundBatchCommand {
    let mut metadata = item(owner, key);
    metadata.payload = NormalizedPayload::file_admitted(
        FileChangeFacts {
            resource: SafeResourceLocation::Resource {
                resource: SafeResourceRef::new(
                    SageRootId::parse("root-one").unwrap(),
                    "src/new.rs",
                )
                .unwrap(),
            },
            summary: SafeText::new("summary", "File created").unwrap(),
            additions: Some(1),
            deletions: Some(0),
            operation: FileChangeOperation::Created,
        },
        0,
        (0, new.len() as u64),
        pair_digest(b"", new.as_bytes()),
    );
    metadata.payload_digest =
        facts::hex_digest(serde_json::to_string(&metadata.payload).unwrap().as_bytes());
    let pair = FilePairCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: metadata.external_item_id.clone(),
        old_text: String::new(),
        new_text: new.into(),
        now_millis: 6,
    };
    let bytes = new.len() + 1024;
    AdapterInboundBatchCommand::new(
        owner.clone(),
        bytes,
        bytes,
        vec![
            AdapterInboundEvent::Item(metadata),
            AdapterInboundEvent::FilePair(pair),
        ],
    )
    .unwrap()
}

#[test]
fn a_created_file_keeps_its_operation_through_pair_persistence_and_projection() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        created_pair_batch(&fixture.owner, "created", "created\n"),
    )
    .unwrap();
    let page = load_chat_page(&fixture.connection, project(), chat(), None).unwrap();
    let change = page
        .turns
        .as_slice()
        .iter()
        .flat_map(|turn| turn.items.as_slice())
        .find_map(|item| match &item.content {
            ItemContent::FileChangeDiff { changes, .. } => Some(changes.as_slice()[0].clone()),
            _ => None,
        })
        .expect("the created file change is projected");
    assert_eq!(change.operation, FileOperation::Created);
    assert_eq!((change.additions, change.deletions), (Some(1), Some(0)));
}

fn unverified_creation_batch(owner: &AdapterStoreOwner, key: &str) -> AdapterInboundBatchCommand {
    let mut metadata = item(owner, key);
    metadata.payload = NormalizedPayload::file_summary_only(
        FileChangeFacts {
            resource: SafeResourceLocation::unavailable(UnavailableResourceReason::Missing),
            summary: SafeText::new("summary", "File created").unwrap(),
            additions: Some(1),
            deletions: Some(0),
            operation: FileChangeOperation::Created,
        },
        false,
    );
    metadata.payload_digest =
        facts::hex_digest(serde_json::to_string(&metadata.payload).unwrap().as_bytes());
    AdapterInboundBatchCommand::new(
        owner.clone(),
        1024,
        1024,
        vec![AdapterInboundEvent::Item(metadata)],
    )
    .unwrap()
}

fn projected_change(
    connection: &rusqlite::Connection,
) -> crate::acp::projection::FileChangeProjection {
    load_chat_page(connection, project(), chat(), None)
        .unwrap()
        .turns
        .as_slice()
        .iter()
        .flat_map(|turn| turn.items.as_slice())
        .find_map(|item| match &item.content {
            ItemContent::FileChangeDiff { changes, .. } => Some(changes.as_slice()[0].clone()),
            _ => None,
        })
        .expect("the file change is projected")
}

#[test]
fn an_unverified_creation_is_promoted_to_one_openable_pair_at_completion() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        unverified_creation_batch(&fixture.owner, "created"),
    )
    .unwrap();
    let pending = projected_change(&fixture.connection);
    assert_eq!(pending.pair_state, FilePairState::Unavailable);
    assert_eq!(
        pending.resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Missing)
    );
    assert_eq!(pair_count(&fixture), 0);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        created_pair_batch(&fixture.owner, "created", "created\n"),
    )
    .unwrap();
    let promoted = projected_change(&fixture.connection);
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    assert!(matches!(
        promoted.resource,
        SafeResourceLocation::Resource { .. }
    ));
    assert_eq!(pair_count(&fixture), 1);
    assert_reconciled(&fixture);
}

#[test]
fn a_persisted_pair_is_never_revoked_or_redirected_by_a_later_resource() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        created_pair_batch(&fixture.owner, "created", "created\n"),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            unverified_creation_batch(&fixture.owner, "created")
        ),
        Err(StoreError::Conflict {
            resource: "filePairResource",
            ..
        })
    ));
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "created", "old", "new")
        ),
        Err(StoreError::Conflict {
            resource: "filePairResource",
            ..
        })
    ));
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(pair_count(&fixture), 1);
    assert_reconciled(&fixture);
}

fn assert_reconciled(fixture: &Fixture) {
    let expected: (i64, i64) = fixture
        .connection
        .query_row(
            "SELECT COALESCE((SELECT SUM(payload_bytes) FROM items),0)
          + COALESCE((SELECT SUM(old_bytes+new_bytes) FROM acp_file_change_snapshots),0),
          COALESCE((SELECT SUM(payload_bytes) FROM session_facts),0)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let session: (i64, i64) = fixture
        .connection
        .query_row(
            "SELECT item_bytes,payload_bytes FROM acp_session_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let global: (i64, i64) = fixture
        .connection
        .query_row(
            "SELECT item_bytes,shared_payload_bytes FROM acp_store_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let project: i64 = fixture
        .connection
        .query_row("SELECT payload_bytes FROM acp_project_usage", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(session, (expected.0, expected.0 + expected.1));
    assert_eq!(global, session);
    assert_eq!(project, session.1);
}

#[test]
fn completed_pair_retransmission_is_idempotent_and_ledgers_remain_exact() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "new"),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    assert_reconciled(&fixture);
    let duplicate = apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "new"),
    )
    .unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(pair_count(&fixture), 1);
    assert_reconciled(&fixture);
}

#[test]
fn deleting_an_owned_item_cascades_snapshots_and_releases_exact_usage() {
    let mut fixture = snapshot_fixture();
    let initial_count: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .unwrap();
    for new in ["first", "second"] {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", "old", new),
        )
        .unwrap();
    }
    assert_eq!(pair_count(&fixture), 2);
    let transaction = fixture.connection.transaction().unwrap();
    transaction
        .execute("DELETE FROM items WHERE external_item_id='file'", [])
        .unwrap();
    transaction.commit().unwrap();
    assert_eq!(pair_count(&fixture), 0);
    assert_reconciled(&fixture);
    for table in ["acp_session_usage", "acp_project_usage", "acp_store_usage"] {
        let count: i64 = fixture
            .connection
            .query_row(&format!("SELECT item_count FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, initial_count);
    }
}

#[test]
fn compacting_a_snapshot_releases_only_its_bytes_and_rollback_restores_it() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "new"),
    )
    .unwrap();
    {
        let transaction = fixture.connection.transaction().unwrap();
        transaction
            .execute("DELETE FROM acp_file_change_snapshots", [])
            .unwrap();
        transaction.rollback().unwrap();
    }
    assert_eq!(pair_count(&fixture), 1);
    assert_reconciled(&fixture);
    fixture
        .connection
        .execute("DELETE FROM acp_file_change_snapshots", [])
        .unwrap();
    assert_eq!(pair_count(&fixture), 0);
    assert_reconciled(&fixture);
    assert_eq!(
        crate::acp::store::read_snapshot(
            &fixture.connection,
            &reference(&fixture, 1, "old", "new"),
            &[SageRootId::parse("root-one").unwrap()]
        ),
        Err(crate::acp::file_diff::SnapshotReadError::Unavailable)
    );
}

#[test]
fn compaction_never_reuses_an_issued_snapshot_revision() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "first"),
    )
    .unwrap();
    fixture
        .connection
        .execute("DELETE FROM acp_file_change_snapshots", [])
        .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "second"),
    )
    .unwrap();
    let revision: i64 = fixture
        .connection
        .query_row(
            "SELECT snapshot_revision FROM acp_file_change_snapshots",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revision, 2);
    assert_reconciled(&fixture);
}

#[test]
fn evolving_one_item_retains_exactly_32_immutable_versions_then_summary_only() {
    let mut fixture = snapshot_fixture();
    for version in 1..=33 {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", "old", &format!("new-{version}")),
        )
        .unwrap();
        assert_eq!(pair_count(&fixture), version.min(32));
        assert_reconciled(&fixture);
    }
    let payload: String = fixture
        .connection
        .query_row(
            "SELECT payload_json FROM items WHERE external_item_id='file'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(matches!(
        serde_json::from_str::<NormalizedPayload>(&payload).unwrap(),
        NormalizedPayload::FileChangeDiff {
            truncated: true,
            pair_available: false,
            ..
        }
    ));
    let first: Vec<u8> = fixture
        .connection
        .query_row(
            "SELECT new_blob FROM acp_file_change_snapshots WHERE snapshot_revision=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(first, b"new-1");
}

#[test]
fn exact_raw_side_cap_survives_json_escaping_and_is_fully_charged() {
    let mut fixture = snapshot_fixture();
    let old = "\"".repeat(DIFF_SIDE_BYTES_MAX);
    let new = "\\".repeat(DIFF_SIDE_BYTES_MAX);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", &old, &new),
    )
    .unwrap();
    let stored: (Vec<u8>, Vec<u8>) = fixture
        .connection
        .query_row(
            "SELECT old_blob,new_blob FROM acp_file_change_snapshots",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (old.into_bytes(), new.into_bytes()));
    assert_reconciled(&fixture);
}

#[test]
fn ordinary_items_cannot_spend_bytes_already_owned_by_snapshots() {
    let mut fixture = snapshot_fixture();
    let side = "x".repeat(DIFF_SIDE_BYTES_MAX);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", &side, &side),
    )
    .unwrap();
    let used: i64 = fixture
        .connection
        .query_row(
            "SELECT COALESCE((SELECT SUM(payload_bytes) FROM items),0)
          + COALESCE((SELECT SUM(old_bytes+new_bytes) FROM acp_file_change_snapshots),0)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let minimum = serialized_text_payload_bytes(0);
    let maximum = LIMITS.wire.normalized_item_bytes as usize;
    let mut remaining = LIMITS.wire.item_bytes_per_turn as usize - used as usize;
    let mut sizes = Vec::new();
    while remaining > maximum {
        sizes.push(maximum);
        remaining -= maximum;
    }
    if remaining < minimum {
        let previous = sizes.pop().unwrap();
        sizes.push(previous - minimum);
        remaining += minimum;
    }
    sizes.push(remaining);
    for (index, bytes) in sizes.into_iter().enumerate() {
        apply_item_ingress(
            &mut fixture.connection,
            sized_item(&fixture.owner, &format!("fill-{index}"), bytes),
        )
        .unwrap();
    }
    assert_reconciled(&fixture);
    assert!(matches!(
        apply_item_ingress(
            &mut fixture.connection,
            sized_item(&fixture.owner, "one-over", minimum)
        ),
        Err(StoreError::Capacity {
            resource: "turnItemBytes",
            ..
        })
    ));
    assert_reconciled(&fixture);
}

#[test]
fn removed_project_root_rejects_pair_without_mutating_the_batch() {
    let mut fixture = snapshot_fixture();
    fixture
        .connection
        .execute("DELETE FROM project_roots", [])
        .unwrap();
    let before = projection(&fixture.connection);
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", "old", "new")
        ),
        Err(StoreError::Conflict {
            resource: "filePairResource",
            ..
        })
    ));
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(pair_count(&fixture), 0);
    assert_reconciled(&fixture);
}

#[test]
fn returning_to_historical_content_mints_a_new_version_without_replacing_old_refs() {
    let mut fixture = snapshot_fixture();
    for new in ["one", "two", "one"] {
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", "old", new),
        )
        .unwrap();
    }
    assert_eq!(pair_count(&fixture), 3);
    assert_reconciled(&fixture);
}

#[test]
fn snapshot_write_failure_rolls_back_metadata_facts_and_all_usage() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "first"),
    )
    .unwrap();
    let before = projection(&fixture.connection);
    let payload: String = fixture
        .connection
        .query_row(
            "SELECT payload_json FROM items WHERE external_item_id='file'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    fixture
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_snapshot_write BEFORE INSERT ON acp_file_change_snapshots
         BEGIN SELECT RAISE(ABORT,'snapshot test fault'); END;",
        )
        .unwrap();
    assert!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", "old", "second")
        )
        .is_err()
    );
    assert_eq!(projection(&fixture.connection), before);
    assert_eq!(pair_count(&fixture), 1);
    let after: String = fixture
        .connection
        .query_row(
            "SELECT payload_json FROM items WHERE external_item_id='file'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(payload, after);
    assert_reconciled(&fixture);
}

#[test]
fn raw_side_plus_one_keeps_only_truthful_summary_and_no_partial_blob() {
    for (old, new) in [
        ("x".repeat(DIFF_SIDE_BYTES_MAX + 1), String::new()),
        (String::new(), "x".repeat(DIFF_SIDE_BYTES_MAX + 1)),
    ] {
        let mut fixture = snapshot_fixture();
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            pair_batch(&fixture.owner, "file", &old, &new),
        )
        .unwrap();
        assert_eq!(pair_count(&fixture), 0);
        let json: String = fixture
            .connection
            .query_row(
                "SELECT payload_json FROM items WHERE external_item_id='file'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<NormalizedPayload>(&json).unwrap(),
            NormalizedPayload::FileChangeDiff {
                pair_available: false,
                truncated: true,
                ..
            }
        ));
        assert_reconciled(&fixture);
    }
}

fn reference(
    fixture: &Fixture,
    revision: u64,
    old: &str,
    new: &str,
) -> crate::acp::file_diff::DiffRef {
    crate::acp::file_diff::DiffRef {
        project_id: fixture.owner.project_id.as_str().into(),
        chat_id: fixture.owner.chat_id.as_str().into(),
        session_id: fixture.owner.session_id.as_str().into(),
        runtime_epoch: fixture.owner.runtime_epoch.get(),
        turn_id: fixture.owner.turn_id.as_str().into(),
        item_id: fixture
            .connection
            .query_row(
                "SELECT id FROM items WHERE external_item_id='file'",
                [],
                |r| r.get(0),
            )
            .unwrap(),
        diff_ordinal: 0,
        snapshot_revision: revision,
        root_id: "root-one".into(),
        relative_path: "src/lib.rs".into(),
        content_digest: pair_digest(old.as_bytes(), new.as_bytes()),
    }
}

#[test]
fn old_refs_resolve_old_bytes_while_wrong_owner_and_corrupt_bytes_fail_closed() {
    use crate::acp::file_diff::SnapshotReadError;
    let mut fixture = snapshot_fixture();
    let roots = [SageRootId::parse("root-one").unwrap()];
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "first"),
    )
    .unwrap();
    let first = reference(&fixture, 1, "old", "first");
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "second"),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&fixture.connection, &first, &roots)
            .unwrap()
            .new_text,
        "first"
    );
    for field in 0..6 {
        let mut forged = first.clone();
        match field {
            0 => forged.project_id = "other".into(),
            1 => forged.chat_id = "other".into(),
            2 => forged.session_id = "other".into(),
            3 => forged.turn_id = "other".into(),
            4 => forged.runtime_epoch += 1,
            _ => forged.relative_path = "src/other.rs".into(),
        }
        assert_eq!(
            read_snapshot(&fixture.connection, &forged, &roots),
            Err(SnapshotReadError::Stale)
        );
    }
    assert_eq!(
        read_snapshot(&fixture.connection, &first, &[]),
        Err(SnapshotReadError::Forbidden)
    );
    fixture
        .connection
        .execute_batch(
            "DROP TRIGGER deny_file_change_snapshot_mutation;
         UPDATE acp_file_change_snapshots SET old_blob=X'FFFFFF' WHERE snapshot_revision=1;",
        )
        .unwrap();
    assert_eq!(
        read_snapshot(&fixture.connection, &first, &roots),
        Err(SnapshotReadError::Corrupt)
    );
}

#[test]
fn snapshot_sql_owner_is_bound_to_its_actual_item_and_versions_cannot_be_overwritten() {
    let mut fixture = snapshot_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        pair_batch(&fixture.owner, "file", "old", "new"),
    )
    .unwrap();
    assert!(
        fixture
            .connection
            .execute(
                "UPDATE acp_file_change_snapshots SET new_blob=X'6E6577'",
                [],
            )
            .is_err()
    );
    assert!(
        fixture
            .connection
            .execute(
                "INSERT INTO acp_file_change_snapshots SELECT item_id,'foreign',chat_id,session_id,
         turn_id,runtime_epoch,diff_ordinal,2,root_id,relative_path,old_blob,new_blob,
         old_bytes,new_bytes,content_digest,created_at FROM acp_file_change_snapshots",
                [],
            )
            .is_err()
    );
    assert_eq!(pair_count(&fixture), 1);
    assert_reconciled(&fixture);
}
