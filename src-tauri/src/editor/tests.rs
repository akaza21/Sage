use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use super::filesystem::{cleanup_failed_save_with, temporary_contract_path};
use super::*;
use crate::{
    domain::BufferSnapshotRequest,
    editor_content::MAX_EDITABLE_BYTES,
    editor_coordination::{EditorOperationCoordinator, EditorOperationKind},
};

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    database: Option<Database>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-editor-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage-test.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let workspace = Workspace::open(&project.id, &project.roots[0].id, &root).unwrap();
        Self {
            root,
            workspace,
            database: Some(database),
        }
    }

    fn database(&self) -> &Database {
        self.database.as_ref().unwrap()
    }

    fn write(&self, path: &str, content: &[u8]) {
        let absolute = self.root.join(path);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute, content).unwrap();
    }

    fn reopen_database(&mut self) {
        drop(self.database.take());
        self.database = Some(Database::open(&self.root.join("sage-test.db")).unwrap());
    }

    fn resource(&self, path: &str) -> ResourceRef {
        self.workspace.resource(path).unwrap()
    }

    fn buffer(&self, path: &str) -> Option<PersistedEditorBuffer> {
        let resource = ResourceRef {
            root_id: self.workspace.root_id().into(),
            relative_path: path.into(),
        };
        self.database()
            .editor_buffer(self.workspace.project_id(), &resource)
            .unwrap()
    }

    fn dirty_buffers(&self) -> Vec<PersistedEditorBuffer> {
        self.database()
            .dirty_editor_buffers(self.workspace.project_id(), None)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.database.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn snapshot_request(
    document: &EditorDocument,
    content: impl Into<String>,
    revision: u64,
) -> BufferSnapshotRequest {
    BufferSnapshotRequest {
        resource: document.resource.clone(),
        base: document.base.clone(),
        line_ending: document.line_ending.clone(),
        content: content.into(),
        revision,
    }
}

fn open_document(
    database: &Database,
    workspace: &Workspace,
    path: &str,
) -> AppResult<EditorDocument> {
    super::open_document(database, workspace, &workspace.resource(path)?)
}

fn save_document(
    database: &Database,
    workspace: &Workspace,
    path: &str,
) -> AppResult<EditorSaveResult> {
    super::save_document(database, workspace, &workspace.resource(path)?)
}

fn discard_document(
    database: &Database,
    workspace: &Workspace,
    path: &str,
) -> AppResult<EditorDocument> {
    super::discard_document(database, workspace, &workspace.resource(path)?)
}

#[test]
fn unicode_crlf_round_trips_and_successful_save_cleans_temp() {
    let fixture = Fixture::new();
    let path = "notes/unicode.txt";
    fixture.write(path, "héllo\r\n世界\r\n".as_bytes());
    let permissions_before = fs::metadata(fixture.root.join(path))
        .unwrap()
        .permissions()
        .readonly();
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert_eq!(opened.line_ending, "crlf");
    assert_eq!(opened.buffer.content, "héllo\n世界\n");
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "héllo\n世界!\n", 1),
    )
    .unwrap();
    let result = save_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert_eq!(result.outcome, "saved");
    assert!(!result.document.dirty);
    assert_eq!(
        fs::read(fixture.root.join(path)).unwrap(),
        "héllo\r\n世界!\r\n".as_bytes()
    );
    assert_eq!(
        fs::metadata(fixture.root.join(path))
            .unwrap()
            .permissions()
            .readonly(),
        permissions_before
    );
    assert!(
        fs::read_dir(fixture.root.join("notes"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".sage-"))
    );
}

#[test]
fn same_size_rapid_external_change_blocks_save_and_preserves_buffer() {
    let fixture = Fixture::new();
    let path = "same-size.txt";
    fixture.write(path, b"alpha\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "gamma\n", 1),
    )
    .unwrap();
    fixture.write(path, b"bravo\n");

    let result = save_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert_eq!(result.outcome, "conflict");
    assert!(result.document.conflict.is_some());
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"bravo\n");
    assert_eq!(fixture.buffer(path).unwrap().content, "gamma\n");
}

#[test]
fn restart_recovers_an_unsaved_buffer() {
    let mut fixture = Fixture::new();
    let path = "restart.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "unsaved λ\n", 4),
    )
    .unwrap();

    fixture.reopen_database();
    let recovered = open_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert!(recovered.dirty);
    assert_eq!(recovered.buffer.revision, 4);
    assert_eq!(recovered.buffer.content, "unsaved λ\n");
    assert_eq!(recovered.base.fingerprint, opened.disk.fingerprint);
    assert_eq!(recovered.disk.fingerprint, opened.disk.fingerprint);
}

#[test]
fn discard_restores_the_current_disk_projection() {
    let fixture = Fixture::new();
    let path = "discard.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "dirty\n", 1),
    )
    .unwrap();
    fixture.write(path, b"other\n");

    let discarded = discard_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert!(!discarded.dirty);
    assert_eq!(discarded.buffer.content, "other\n");
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn discard_read_failure_preserves_the_durable_buffer() {
    let fixture = Fixture::new();
    let path = "discard-missing.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "dirty\n", 1),
    )
    .unwrap();
    fs::remove_file(fixture.root.join(path)).unwrap();

    assert!(discard_document(fixture.database(), &fixture.workspace, path).is_err());
    assert_eq!(fixture.buffer(path).unwrap().content, "dirty\n");
}

#[test]
fn stale_snapshot_revision_cannot_replace_newer_durable_content() {
    let fixture = Fixture::new();
    let path = "revision.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "newer\n", 2),
    )
    .unwrap();
    let stale = snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "stale\n", 1),
    )
    .unwrap();

    assert_eq!(stale.revision, 2);
    assert_eq!(fixture.buffer(path).unwrap().content, "newer\n");
}

#[test]
fn delayed_clean_snapshot_cannot_delete_newer_dirty_content() {
    let fixture = Fixture::new();
    let path = "clean-revision.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "newer\n", 2),
    )
    .unwrap();

    let stale_clean = snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "base\n", 1),
    )
    .unwrap();

    assert_eq!(stale_clean.revision, 2);
    assert!(stale_clean.dirty);
    let durable = fixture.buffer(path).unwrap();
    assert_eq!(durable.revision, 2);
    assert_eq!(durable.content, "newer\n");
}

#[test]
fn delayed_snapshot_cannot_resurrect_after_successful_save() {
    let fixture = Fixture::new();
    let path = "save-race.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "saved\n", 1),
    )
    .unwrap();
    let delayed_request = snapshot_request(&opened, "stale\n", 2);
    let operations = EditorOperationCoordinator::default();
    let delayed_ticket = operations.issue_ticket().unwrap();
    let save_ticket = operations.issue_ticket().unwrap();

    let saved = operations
        .execute(save_ticket, EditorOperationKind::Terminal, || {
            save_document(fixture.database(), &fixture.workspace, path)
        })
        .unwrap();
    let delayed = operations.execute(delayed_ticket, EditorOperationKind::Snapshot, || {
        snapshot_buffer(fixture.database(), &fixture.workspace, delayed_request)
    });

    assert_eq!(saved.outcome, "saved");
    assert!(matches!(delayed, Err(AppError::InvalidRequest(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"saved\n");
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn delayed_snapshot_cannot_resurrect_after_discard() {
    let fixture = Fixture::new();
    let path = "discard-race.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "dirty\n", 1),
    )
    .unwrap();
    let delayed_request = snapshot_request(&opened, "stale\n", 2);
    let operations = EditorOperationCoordinator::default();
    let delayed_ticket = operations.issue_ticket().unwrap();
    let discard_ticket = operations.issue_ticket().unwrap();

    operations
        .execute(discard_ticket, EditorOperationKind::Terminal, || {
            discard_document(fixture.database(), &fixture.workspace, path)
        })
        .unwrap();
    let delayed = operations.execute(delayed_ticket, EditorOperationKind::Snapshot, || {
        snapshot_buffer(fixture.database(), &fixture.workspace, delayed_request)
    });

    assert!(matches!(delayed, Err(AppError::InvalidRequest(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"base\n");
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn snapshot_without_a_durable_row_rejects_a_changed_disk_base() {
    let fixture = Fixture::new();
    let path = "missing-row.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    fixture.write(path, b"external\n");

    let result = snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "stale\n", 1),
    );

    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn recovery_removes_pre_replace_temp_and_keeps_dirty_buffer() {
    let fixture = Fixture::new();
    let path = "pre-replace.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "dirty\n", 1),
    )
    .unwrap();
    let temp_path = temporary_contract_path(path).unwrap();
    fixture
        .database()
        .set_editor_pending_temp(
            fixture.workspace.project_id(),
            &fixture.resource(path),
            &temp_path,
        )
        .unwrap();
    fixture.write(&temp_path, b"dirty\n");

    recover_pending_saves(fixture.database(), &fixture.workspace).unwrap();

    assert!(!fixture.root.join(&temp_path).exists());
    let buffer = fixture.buffer(path).unwrap();
    assert_eq!(buffer.content, "dirty\n");
    assert!(buffer.pending_temp_path.is_none());
}

#[test]
fn recovery_finalizes_a_post_replace_save() {
    let fixture = Fixture::new();
    let path = "post-replace.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "saved\n", 1),
    )
    .unwrap();
    let temp_path = temporary_contract_path(path).unwrap();
    fixture
        .database()
        .set_editor_pending_temp(
            fixture.workspace.project_id(),
            &fixture.resource(path),
            &temp_path,
        )
        .unwrap();
    fixture.write(&temp_path, b"saved\n");
    fixture.write(path, b"saved\n");

    recover_pending_saves(fixture.database(), &fixture.workspace).unwrap();

    assert!(!fixture.root.join(&temp_path).exists());
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn failed_temp_removal_keeps_recovery_record_until_absence_is_known() {
    let fixture = Fixture::new();
    let path = "cleanup-failure.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "dirty\n", 1),
    )
    .unwrap();
    let temp_path = temporary_contract_path(path).unwrap();
    fixture
        .database()
        .set_editor_pending_temp(
            fixture.workspace.project_id(),
            &fixture.resource(path),
            &temp_path,
        )
        .unwrap();
    let absolute_temp = fixture.root.join(&temp_path);

    cleanup_failed_save_with(
        fixture.database(),
        fixture.workspace.project_id(),
        &fixture.resource(path),
        &absolute_temp,
        |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected removal failure",
            ))
        },
    );
    assert_eq!(
        fixture.buffer(path).unwrap().pending_temp_path.as_deref(),
        Some(temp_path.as_str())
    );

    cleanup_failed_save_with(
        fixture.database(),
        fixture.workspace.project_id(),
        &fixture.resource(path),
        &absolute_temp,
        |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
    );
    assert!(fixture.buffer(path).unwrap().pending_temp_path.is_none());
}

#[test]
fn oversized_file_and_buffer_are_rejected() {
    let fixture = Fixture::new();
    let path = "oversized.txt";
    fixture.write(path, &vec![b'a'; MAX_EDITABLE_BYTES as usize + 1]);
    assert!(matches!(
        open_document(fixture.database(), &fixture.workspace, path),
        Err(AppError::ResourceLimit(_))
    ));

    fixture.write(path, b"base");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    assert!(matches!(
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened, "a".repeat(MAX_EDITABLE_BYTES as usize + 1), 1)
        ),
        Err(AppError::ResourceLimit(_))
    ));
    assert!(fixture.buffer(path).is_none());
}

#[test]
fn dirty_buffer_count_is_bounded_without_losing_existing_rows() {
    let fixture = Fixture::new();
    for index in 0..16 {
        let path = format!("count-{index}.txt");
        fixture.write(&path, b"a");
        let opened = open_document(fixture.database(), &fixture.workspace, &path).unwrap();
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened, "b", 1),
        )
        .unwrap();
    }
    let overflow_path = "count-overflow.txt";
    fixture.write(overflow_path, b"a");
    let overflow = open_document(fixture.database(), &fixture.workspace, overflow_path).unwrap();

    assert!(matches!(
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&overflow, "b", 1)
        ),
        Err(AppError::ResourceLimit(_))
    ));
    assert_eq!(fixture.dirty_buffers().len(), 16);
}

#[test]
fn total_dirty_buffer_bytes_are_bounded() {
    let fixture = Fixture::new();
    let one_mebibyte = "x".repeat(MAX_EDITABLE_BYTES as usize);
    for index in 0..8 {
        let path = format!("bytes-{index}.txt");
        fixture.write(&path, b"a");
        let opened = open_document(fixture.database(), &fixture.workspace, &path).unwrap();
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened, one_mebibyte.clone(), 1),
        )
        .unwrap();
    }
    let overflow_path = "bytes-overflow.txt";
    fixture.write(overflow_path, b"a");
    let overflow = open_document(fixture.database(), &fixture.workspace, overflow_path).unwrap();

    assert!(matches!(
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&overflow, "b", 1)
        ),
        Err(AppError::ResourceLimit(_))
    ));
    assert_eq!(
        fixture
            .dirty_buffers()
            .iter()
            .map(|buffer| buffer.byte_len)
            .sum::<u64>(),
        8 * MAX_EDITABLE_BYTES
    );
}

#[test]
#[ignore = "manual release-mode performance evidence"]
fn benchmarks_one_mebibyte_save_latency() {
    use std::time::Instant;

    let fixture = Fixture::new();
    let operations = EditorOperationCoordinator::default();
    let content = "z".repeat(MAX_EDITABLE_BYTES as usize);
    let mut elapsed_millis = Vec::with_capacity(20);
    for index in 0..20 {
        let path = format!("save-benchmark-{index}.txt");
        fixture.write(&path, b"a");
        let opened = open_document(fixture.database(), &fixture.workspace, &path).unwrap();
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened, content.clone(), 1),
        )
        .unwrap();
        let started = Instant::now();
        let ticket = operations.issue_ticket().unwrap();
        let result = operations
            .execute(ticket, EditorOperationKind::Terminal, || {
                save_document(fixture.database(), &fixture.workspace, &path)
            })
            .unwrap();
        assert_eq!(result.outcome, "saved");
        elapsed_millis.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    elapsed_millis.sort_by(f64::total_cmp);
    println!(
        "atomic_save_1mib iterations=20 p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        elapsed_millis[9], elapsed_millis[18], elapsed_millis[19]
    );
}

#[test]
#[ignore = "manual SQLite and WAL growth evidence"]
fn measures_bounded_sqlite_growth() {
    fn file_size(path: &Path) -> u64 {
        fs::metadata(path).map_or(0, |metadata| metadata.len())
    }
    fn sizes(root: &Path) -> (u64, u64) {
        (
            file_size(&root.join("sage-test.db")),
            file_size(&root.join("sage-test.db-wal")),
        )
    }

    let fixture = Fixture::new();
    let content = "x".repeat(64 * 1024);
    let mut opened_documents = Vec::new();
    for index in 0..16 {
        let path = format!("growth-{index}.txt");
        fixture.write(&path, b"a");
        opened_documents
            .push(open_document(fixture.database(), &fixture.workspace, &path).unwrap());
    }
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened_documents[0], content.clone(), 1),
    )
    .unwrap();
    let one = sizes(&fixture.root);
    for document in opened_documents.iter().skip(1) {
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(document, content.clone(), 1),
        )
        .unwrap();
    }
    let sixteen = sizes(&fixture.root);
    for revision in 2..=101 {
        let mut updated = content.clone();
        updated.replace_range(..1, if revision % 2 == 0 { "y" } else { "x" });
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened_documents[0], updated, revision),
        )
        .unwrap();
    }
    let repeated = sizes(&fixture.root);
    println!(
        "sqlite_growth buffer_bytes=65536 one_db={} one_wal={} sixteen_db={} sixteen_wal={} repeated_100_db={} repeated_100_wal={}",
        one.0, one.1, sixteen.0, sixteen.1, repeated.0, repeated.1
    );
}
