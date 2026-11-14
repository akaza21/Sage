use super::*;
use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageChatId,
            SageProjectId, SageRootId,
        },
        intent::{BindingId, SafeResourceLocation, SafeResourceRef},
    },
    acp_protocol::file_diff::{SnapshotReadError, pair_digest},
    domain::{ProjectLayoutRequest, ResourceRef, WorkbenchTab},
    persistence::Database,
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

fn reopen_fixture() -> Fixture {
    let fixture = fixture();
    fixture
        .connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    fixture
}

fn reopen_pair_batch_for_root(
    owner: &AdapterStoreOwner,
    root_id: &SageRootId,
    key: &str,
    old: &str,
    new: &str,
) -> AdapterInboundBatchCommand {
    let mut metadata = item(owner, key);
    metadata.payload = NormalizedPayload::file_admitted(
        crate::acp::intent::FileChangeFacts {
            resource: SafeResourceLocation::Resource {
                resource: SafeResourceRef::new(root_id.clone(), "src/lib.rs").unwrap(),
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

fn reopen_pair_batch(
    owner: &AdapterStoreOwner,
    key: &str,
    old: &str,
    new: &str,
) -> AdapterInboundBatchCommand {
    reopen_pair_batch_for_root(owner, &fixture_root(), key, old, new)
}

fn reopen_reference_in(
    owner: &AdapterStoreOwner,
    connection: &rusqlite::Connection,
    root_id: &SageRootId,
    revision: u64,
    old: &str,
    new: &str,
) -> crate::acp_protocol::file_diff::DiffRef {
    crate::acp_protocol::file_diff::DiffRef {
        project_id: owner.project_id.as_str().into(),
        chat_id: owner.chat_id.as_str().into(),
        session_id: owner.session_id.as_str().into(),
        runtime_epoch: owner.runtime_epoch.get(),
        turn_id: owner.turn_id.as_str().into(),
        item_id: connection
            .query_row(
                "SELECT id FROM items WHERE external_item_id='reopen-file'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        diff_ordinal: 0,
        snapshot_revision: revision,
        root_id: root_id.as_str().into(),
        relative_path: "src/lib.rs".into(),
        content_digest: pair_digest(old.as_bytes(), new.as_bytes()),
    }
}

fn reopen_usage_in(connection: &rusqlite::Connection) -> (i64, i64, i64, i64) {
    let session: (i64, i64) = connection
        .query_row(
            "SELECT item_bytes,payload_bytes FROM acp_session_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let global: (i64, i64) = connection
        .query_row(
            "SELECT item_bytes,shared_payload_bytes FROM acp_store_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    (session.0, session.1, global.0, global.1)
}

fn reopen_blobs_in(
    connection: &rusqlite::Connection,
    revision: u64,
) -> (Vec<u8>, Vec<u8>, String, i64, i64) {
    connection
        .query_row(
            "SELECT old_blob,new_blob,content_digest,old_bytes,new_bytes
             FROM acp_file_change_snapshots WHERE snapshot_revision=?1",
            [revision as i64],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

fn fixture_root() -> SageRootId {
    SageRootId::parse("root-one").unwrap()
}

fn reopen_reference(
    fixture: &Fixture,
    revision: u64,
    old: &str,
    new: &str,
) -> crate::acp_protocol::file_diff::DiffRef {
    reopen_reference_in(
        &fixture.owner,
        &fixture.connection,
        &fixture_root(),
        revision,
        old,
        new,
    )
}

fn reopen_usage(fixture: &Fixture) -> (i64, i64, i64, i64) {
    reopen_usage_in(&fixture.connection)
}

fn file_backed_fixture() -> (Database, PathBuf, PathBuf, AdapterStoreOwner, SageRootId) {
    let directory = std::env::temp_dir().join(format!("sage-snapshot-reopen-{}", Uuid::new_v4()));
    let root = directory.join("project-root");
    fs::create_dir_all(&root).unwrap();
    let path = directory.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let bootstrap = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(bootstrap.active_tab_id.unwrap()).unwrap();
    let root_id = SageRootId::parse(project.roots[0].id.clone()).unwrap();
    let mut connection = database.connection();
    let initial_projection: u64 = connection
        .query_row(
            "SELECT projection_revision FROM chats WHERE id=?1",
            [chat_id.as_str()],
            |row| row.get::<_, i64>(0).map(|value| value as u64),
        )
        .unwrap();
    let started = start_session(
        &mut connection,
        StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: ProjectionRevision::new(initial_projection).unwrap(),
            client_intent_id: ClientIntentId::parse("reopen-start").unwrap(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: 1,
        },
    )
    .unwrap();
    let _activated = activate_session(
        &mut connection,
        ActivateSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::parse("reopen-activate").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "d".repeat(64),
            now_millis: 2,
        },
    )
    .unwrap();
    let external_session_id = ExternalId::new("external", "reopen-session").unwrap();
    let completed = complete_activation(
        &mut connection,
        CompleteActivationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: external_session_id.clone(),
            capabilities_json: capabilities(),
            now_millis: 3,
        },
    )
    .unwrap();
    let prepared = prepare_turn(
        &mut connection,
        PrepareTurnCommand {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            expected_projection_revision: completed.projection_revision,
            client_intent_id: ClientIntentId::parse("reopen-turn").unwrap(),
            prompt: Prompt::new("prompt", "test").unwrap(),
            now_millis: 4,
        },
    )
    .unwrap();
    begin_send(
        &mut connection,
        TurnMutationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 5,
        },
    )
    .unwrap();
    drop(connection);
    (
        database,
        directory,
        path,
        AdapterStoreOwner {
            project_id,
            chat_id,
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: prepared.turn_id,
            binding_digest: "a".repeat(64),
            external_session_id,
        },
        root_id,
    )
}

#[test]
fn populated_pair_and_durable_diff_ref_reopen_without_git_or_disk_fallback() {
    let (database, directory, path, owner, root_id) = file_backed_fixture();
    {
        let mut connection = database.connection();
        apply_adapter_inbound_batch(
            &mut connection,
            reopen_pair_batch_for_root(&owner, &root_id, "reopen-file", "old\n", "new\n"),
        )
        .unwrap();
    }
    let (before_blobs, before_usage, before_ref, before_high_water) = {
        let connection = database.connection();
        (
            reopen_blobs_in(&connection, 1),
            reopen_usage_in(&connection),
            reopen_reference_in(&owner, &connection, &root_id, 1, "old\n", "new\n"),
            connection
                .query_row(
                    "SELECT last_revision FROM acp_file_change_snapshot_sequences",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
        )
    };
    let tabs = database.bootstrap_snapshot().unwrap().tabs;
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: owner.project_id.as_str().into(),
            tabs: [
                tabs.into_iter().next().unwrap(),
                WorkbenchTab {
                    id: "snapshot-reopen-diff".into(),
                    kind: "diff".into(),
                    title: "lib.rs saved change".into(),
                    position: 1,
                    pinned: false,
                    dirty: false,
                    status: None,
                    updated_at: None,
                    finished_at: None,
                    resource: Some(ResourceRef {
                        root_id: root_id.as_str().into(),
                        relative_path: "src/lib.rs".into(),
                    }),
                    repository: None,
                    diff_ref: Some(before_ref.clone()),
                },
            ]
            .into(),
            active_tab_id: Some("snapshot-reopen-diff".into()),
            selected_root_id: Some(root_id.as_str().into()),
            selected_resource: None,
        })
        .unwrap();
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let roots = [root_id.clone()];
    let connection = reopened.connection();
    assert_eq!(reopen_blobs_in(&connection, 1), before_blobs);
    assert_eq!(reopen_usage_in(&connection), before_usage);
    assert_eq!(
        connection
            .query_row(
                "SELECT last_revision FROM acp_file_change_snapshot_sequences",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        before_high_water
    );
    drop(connection);
    let restored_ref = reopened
        .bootstrap_snapshot()
        .unwrap()
        .tabs
        .into_iter()
        .find(|tab| tab.id == "snapshot-reopen-diff")
        .and_then(|tab| tab.diff_ref)
        .unwrap();
    assert_eq!(restored_ref, before_ref);
    let connection = reopened.connection();
    let data = read_snapshot(&connection, &restored_ref, &roots).unwrap();
    assert_eq!(
        (data.old_text, data.new_text),
        ("old\n".into(), "new\n".into())
    );
    assert_eq!(data.content_digest, pair_digest(b"old\n", b"new\n"));
    connection
        .execute_batch(
            "DROP TRIGGER deny_file_change_snapshot_mutation;
             UPDATE acp_file_change_snapshots SET old_blob=X'FFFFFF', old_bytes=3
             WHERE snapshot_revision=1;",
        )
        .unwrap();
    assert_eq!(
        read_snapshot(&connection, &restored_ref, &roots),
        Err(SnapshotReadError::Corrupt)
    );
    connection
        .execute(
            "UPDATE acp_file_change_snapshots SET old_blob=X'6F6C640A', old_bytes=4
             WHERE snapshot_revision=1",
            [],
        )
        .unwrap();

    connection
        .execute("DELETE FROM acp_file_change_snapshots", [])
        .unwrap();
    assert_eq!(
        read_snapshot(&connection, &restored_ref, &roots),
        Err(SnapshotReadError::Unavailable)
    );
    assert!(reopen_usage_in(&connection).2 < before_usage.2);
    assert_eq!(
        connection
            .query_row(
                "SELECT last_revision FROM acp_file_change_snapshot_sequences",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        before_high_water
    );
    drop(connection);
    drop(reopened);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn snapshot_corruption_classes_fail_closed_without_mutating_usage() {
    let mut fixture = reopen_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "first"),
    )
    .unwrap();
    let roots = [SageRootId::parse("root-one").unwrap()];
    let valid = reopen_reference(&fixture, 1, "old", "first");
    assert!(read_snapshot(&fixture.connection, &valid, &roots).is_ok());
    let usage_before = reopen_usage(&fixture);

    fixture
        .connection
        .execute("DELETE FROM acp_file_change_snapshots", [])
        .unwrap();
    assert_eq!(
        read_snapshot(&fixture.connection, &valid, &roots),
        Err(SnapshotReadError::Unavailable)
    );
    let usage_after_delete = reopen_usage(&fixture);
    assert!(usage_after_delete.0 < usage_before.0);
    let reread_usage = reopen_usage(&fixture);
    assert_eq!(usage_after_delete, reread_usage);

    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "second"),
    )
    .unwrap();
    let second = reopen_reference(&fixture, 2, "old", "second");
    let usage_second = reopen_usage(&fixture);

    let mut forged = second.clone();
    forged.chat_id = "other".into();
    assert_eq!(
        read_snapshot(&fixture.connection, &forged, &roots),
        Err(SnapshotReadError::Stale)
    );

    assert!(
        fixture
            .connection
            .execute_batch(
                "DROP TRIGGER deny_file_change_snapshot_mutation;
                 UPDATE acp_file_change_snapshots SET old_bytes=9999 WHERE snapshot_revision=2;",
            )
            .is_err()
    );
    fixture
        .connection
        .execute_batch(
            "CREATE TRIGGER deny_file_change_snapshot_mutation BEFORE UPDATE ON acp_file_change_snapshots
             BEGIN SELECT RAISE(ABORT,'immutable'); END;",
        )
        .unwrap_or(());
    assert_eq!(reopen_usage(&fixture), usage_second);

    fixture
        .connection
        .execute_batch(
            "DROP TRIGGER deny_file_change_snapshot_mutation;
             UPDATE acp_file_change_snapshots
             SET old_blob=X'FFFFFF', old_bytes=3 WHERE snapshot_revision=2;",
        )
        .unwrap();
    assert_eq!(
        read_snapshot(&fixture.connection, &second, &roots),
        Err(SnapshotReadError::Corrupt)
    );
    assert_eq!(reopen_usage(&fixture), usage_second);
}

#[test]
fn high_water_never_aliases_a_deleted_revision() {
    let mut fixture = reopen_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "first"),
    )
    .unwrap();
    let first = reopen_reference(&fixture, 1, "old", "first");
    fixture
        .connection
        .execute("DELETE FROM acp_file_change_snapshots", [])
        .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "second"),
    )
    .unwrap();
    let revision: i64 = fixture
        .connection
        .query_row(
            "SELECT snapshot_revision FROM acp_file_change_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision, 2);
    let roots = [SageRootId::parse("root-one").unwrap()];
    assert_eq!(
        read_snapshot(&fixture.connection, &first, &roots),
        Err(SnapshotReadError::Unavailable)
    );
}

#[test]
fn new_pair_shares_aggregate_accounting_without_double_charge() {
    let mut fixture = reopen_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "new"),
    )
    .unwrap();
    let usage_once = reopen_usage(&fixture);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "new"),
    )
    .unwrap();
    assert_eq!(reopen_usage(&fixture), usage_once);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        reopen_pair_batch(&fixture.owner, "reopen-file", "old", "evolved"),
    )
    .unwrap();
    let reconciled: (i64, i64) = fixture
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
    assert_eq!(session, (reconciled.0, reconciled.0 + reconciled.1));
    let used: i64 = fixture
        .connection
        .query_row(
            "SELECT COALESCE((SELECT SUM(payload_bytes) FROM items),0)
             + COALESCE((SELECT SUM(old_bytes+new_bytes) FROM acp_file_change_snapshots),0)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(used > 0);
    assert!(LIMITS.wire.item_bytes_per_turn as i64 >= used);
}
