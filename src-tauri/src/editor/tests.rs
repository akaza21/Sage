use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use super::filesystem::{cleanup_failed_save_with, read_editable_bytes, temporary_contract_path};
use super::*;
use crate::editor_persistence::store::EditorBufferStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::{
    editor_content::MAX_EDITABLE_BYTES,
    editor_coordination::{EditorOperationCoordinator, EditorOperationKind},
};
use sage_domain::editor::{BufferRebaseRequest, BufferSnapshotRequest, DocumentVersion};

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    database: Option<Database>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-editor-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
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
        self.database = Some(crate::database::open_database(&self.root.join("sage.db")).unwrap());
    }

    fn resource(&self, path: &str) -> ResourceRef {
        self.workspace.resource(path).unwrap()
    }

    fn resource_unchecked(&self, path: &str) -> ResourceRef {
        ResourceRef {
            root_id: self.workspace.root_id().into(),
            relative_path: path.into(),
        }
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
    super::open_document(database, workspace, &spelled(workspace, path))
}

fn save_document(
    database: &Database,
    workspace: &Workspace,
    path: &str,
) -> AppResult<EditorSaveResult> {
    super::save_document(database, workspace, &spelled(workspace, path))
}

fn discard_document(
    database: &Database,
    workspace: &Workspace,
    path: &str,
) -> AppResult<EditorDocument> {
    super::discard_document(database, workspace, &spelled(workspace, path))
}

/// The resource as the UI names it; the editor validates it, and the File
/// may have been deleted.
fn spelled(workspace: &Workspace, path: &str) -> ResourceRef {
    ResourceRef {
        root_id: workspace.root_id().into(),
        relative_path: path.into(),
    }
}

fn sqlite_sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = database_path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

#[test]
fn sqlite_sidecar_path_preserves_the_database_filename() {
    let database_path = Path::new("fixture/sage.db");

    assert_eq!(
        sqlite_sidecar_path(database_path, "-wal"),
        PathBuf::from("fixture/sage.db-wal")
    );
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
fn discard_of_a_deleted_file_drops_its_recovery_buffer() {
    let fixture = dirty_fixture("discard-missing.txt", b"base\n", "dirty\n");
    fs::remove_file(fixture.root.join("discard-missing.txt")).unwrap();

    let discarded = discard_document(
        fixture.database(),
        &fixture.workspace,
        "discard-missing.txt",
    )
    .unwrap();

    assert!(!discarded.dirty);
    assert_eq!(discarded.conflict.unwrap().kind, "deleted");
    assert!(fixture.buffer("discard-missing.txt").is_none());
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
fn bounded_read_accepts_the_cap_and_rejects_one_byte_more() {
    let exact = vec![b'a'; MAX_EDITABLE_BYTES as usize];
    assert_eq!(
        read_editable_bytes(exact.as_slice()).unwrap().len(),
        exact.len()
    );
    let over = vec![b'a'; MAX_EDITABLE_BYTES as usize + 1];
    assert!(matches!(
        read_editable_bytes(over.as_slice()),
        Err(AppError::ResourceLimit(_))
    ));
}

#[test]
fn held_read_rejects_a_file_that_grows_after_its_metadata_check() {
    use std::io::Write;

    let fixture = Fixture::new();
    let path = "growing.txt";
    fixture.write(path, b"small\n");
    let file = fs::File::open(fixture.root.join(path)).unwrap();
    assert!(file.metadata().unwrap().len() <= MAX_EDITABLE_BYTES);

    fs::OpenOptions::new()
        .append(true)
        .open(fixture.root.join(path))
        .unwrap()
        .write_all(&vec![b'a'; MAX_EDITABLE_BYTES as usize])
        .unwrap();

    assert!(matches!(
        read_editable_bytes(&file),
        Err(AppError::ResourceLimit(_))
    ));
}

fn dirty_fixture(path: &str, base: &[u8], dirty: &str) -> Fixture {
    let fixture = Fixture::new();
    fixture.write(path, base);
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, dirty, 1),
    )
    .unwrap();
    fixture
}

fn temp_entries(directory: &Path) -> usize {
    fs::read_dir(directory)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".sage-")
        })
        .count()
}

/// The save failed without losing the edit: the buffer is still dirty and no
/// temporary file is left behind or still recorded.
fn assert_failed_save_kept_buffer(fixture: &Fixture, path: &str, dirty: &str, directory: &Path) {
    let buffer = fixture.buffer(path).expect("the dirty buffer is kept");
    assert_eq!(buffer.content, dirty);
    assert!(buffer.pending_temp_path.is_none());
    assert_eq!(temp_entries(directory), 0);
}

fn fail_at(stage: SaveStage) -> impl FnMut(SaveStage) -> std::io::Result<()> {
    move |current| {
        if current == stage {
            Err(std::io::Error::other("injected save failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn save_failing_partway_through_the_temp_write_removes_it_and_keeps_the_target() {
    let path = "partial.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        fail_at(SaveStage::TempWritten),
    );

    assert!(matches!(result, Err(AppError::Io(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"base\n");
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
}

#[test]
fn save_whose_replace_fails_after_the_temp_exists_removes_it_and_keeps_the_target() {
    let path = "replace-fails.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let mut temp_existed = false;

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage != SaveStage::BeforeReplace {
                return Ok(());
            }
            temp_existed = temp_entries(&fixture.root) == 1;
            Err(std::io::Error::other("injected replace failure"))
        },
    );

    assert!(temp_existed);
    assert!(matches!(result, Err(AppError::Io(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"base\n");
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
}

#[cfg(unix)]
#[test]
fn read_refuses_a_target_or_parent_swapped_for_a_symlink_after_resolution() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture.root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("doc.txt"), b"outside\n").unwrap();
    fixture.write("doc.txt", b"inside\n");
    fixture.write("dir/doc.txt", b"inside\n");
    fixture.resource("doc.txt");
    fixture.resource("dir/doc.txt");

    fs::remove_file(fixture.root.join("doc.txt")).unwrap();
    symlink(outside.join("doc.txt"), fixture.root.join("doc.txt")).unwrap();
    assert!(read_disk_document(&fixture.workspace, "doc.txt").is_err());

    fs::rename(fixture.root.join("dir"), fixture.root.join("dir-old")).unwrap();
    symlink(&outside, fixture.root.join("dir")).unwrap();
    assert!(read_disk_document(&fixture.workspace, "dir/doc.txt").is_err());

    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn save_refuses_a_target_swapped_for_a_symlink_just_before_the_replace() {
    use std::os::unix::fs::symlink;

    let path = "target.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let outside = fixture.root.with_extension("outside.txt");
    fs::write(&outside, b"base\n").unwrap();

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::BeforeReplace {
                fs::remove_file(fixture.root.join(path))?;
                symlink(&outside, fixture.root.join(path))?;
            }
            Ok(())
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"base\n");
    assert_eq!(fs::read_link(fixture.root.join(path)).unwrap(), outside);
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
    fs::remove_file(outside).unwrap();
}

/// Moves `dir` aside to `dir-held` and puts a link to `outside` in its place.
#[cfg(unix)]
fn swap_parent_for_link(root: &Path, outside: &Path) -> std::io::Result<()> {
    fs::rename(root.join("dir"), root.join("dir-held"))?;
    std::os::unix::fs::symlink(outside, root.join("dir"))
}

#[cfg(unix)]
#[test]
fn save_refuses_a_parent_swapped_after_it_is_held() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let outside = fixture.root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("doc.txt"), b"base\n").unwrap();

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::ParentHeld {
                swap_parent_for_link(&fixture.root, &outside)?;
            }
            Ok(())
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read(outside.join("doc.txt")).unwrap(), b"base\n");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(
        fs::read(fixture.root.join("dir-held/doc.txt")).unwrap(),
        b"base\n"
    );
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root.join("dir-held"));
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn save_reports_a_parent_swapped_after_the_replace_and_keeps_the_buffer() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let outside = fixture.root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("doc.txt"), b"base\n").unwrap();

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::AfterReplace {
                swap_parent_for_link(&fixture.root, &outside)?;
            }
            Ok(())
        },
    );

    // The bytes reached the held folder, and the save says so instead of
    // reporting the requested resource as saved and clean.
    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert_eq!(
        fs::read(fixture.root.join("dir-held/doc.txt")).unwrap(),
        b"dirty\n"
    );
    assert_eq!(fs::read(outside.join("doc.txt")).unwrap(), b"base\n");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root.join("dir-held"));
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn save_keeps_the_buffer_when_the_target_is_replaced_after_the_replace() {
    let path = "replaced.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::AfterReplace {
                fs::remove_file(fixture.root.join(path))?;
                fs::write(fixture.root.join(path), b"other\n")?;
            }
            Ok(())
        },
    );

    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"other\n");
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
}

#[test]
fn save_keeps_the_buffer_when_identical_bytes_replace_the_installed_file() {
    let path = "same-content.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    // Create the replacement first so it cannot reuse the saved file's identity.
    let replacement = fixture.root.join("replacement.txt");
    fs::write(&replacement, b"dirty\n").unwrap();

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::AfterReplace {
                fs::rename(&replacement, fixture.root.join(path))?;
            }
            Ok(())
        },
    );

    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"dirty\n");
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
}

#[test]
fn save_keeps_the_buffer_when_the_target_is_deleted_after_the_replace() {
    let path = "deleted.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::AfterReplace {
                fs::remove_file(fixture.root.join(path))?;
            }
            Ok(())
        },
    );

    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert!(!fixture.root.join(path).exists());
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
}

#[cfg(unix)]
#[test]
fn save_keeps_the_buffer_when_the_parent_is_renamed_before_verification() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::AfterReplace {
                fs::rename(fixture.root.join("dir"), fixture.root.join("dir-moved"))?;
            }
            Ok(())
        },
    );

    assert!(matches!(result, Err(AppError::InvalidRequest(_))));
    assert_eq!(
        fs::read(fixture.root.join("dir-moved/doc.txt")).unwrap(),
        b"dirty\n"
    );
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root.join("dir-moved"));
}

/// Documents the accepted Unix semantics: once the saved bytes are verified
/// at the requested path, renaming the folder before the recovery buffer is
/// cleared is the user moving a saved file, so the save stands.
#[cfg(unix)]
#[test]
fn post_verification_parent_rename_on_unix_is_accepted_as_a_user_move() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::BeforeRecoveryClear {
                fs::rename(fixture.root.join("dir"), fixture.root.join("dir-moved"))?;
            }
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(result.outcome, "saved");
    assert!(!result.document.dirty);
    assert_eq!(result.document.buffer.content, "dirty\n");
    assert_eq!(
        fs::read(fixture.root.join("dir-moved/doc.txt")).unwrap(),
        b"dirty\n"
    );
    assert!(fixture.buffer(path).is_none());
}

/// Documents the accepted Unix semantics: once the saved bytes are verified
/// at the requested path, deleting the file before the recovery buffer is
/// cleared is the user removing a saved file, so the save stands.
#[cfg(unix)]
#[test]
fn post_verification_target_delete_on_unix_is_accepted_as_a_user_move() {
    let path = "deleted-after.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::BeforeRecoveryClear {
                fs::remove_file(fixture.root.join(path))?;
            }
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(result.outcome, "saved");
    assert_eq!(result.document.buffer.content, "dirty\n");
    assert!(!fixture.root.join(path).exists());
    assert!(fixture.buffer(path).is_none());
}

/// On Windows the verified target handle and the parent guards stay open
/// until the recovery buffer is cleared, so the saved file cannot be
/// deleted, replaced or moved in that window.
#[cfg(windows)]
#[test]
fn held_handles_block_changes_to_the_saved_file_until_recovery_is_cleared() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let target = fixture.root.join(path);
    let replacement = fixture.root.join("dir/replacement.txt");
    fs::write(&replacement, b"other\n").unwrap();
    let mut refused = (false, false, false);

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::BeforeRecoveryClear {
                refused = (
                    fs::remove_file(&target).is_err(),
                    fs::rename(&replacement, &target).is_err(),
                    fs::rename(fixture.root.join("dir"), fixture.root.join("dir-moved")).is_err(),
                );
            }
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(refused, (true, true, true));
    assert_eq!(result.outcome, "saved");
    assert_eq!(fs::read(&target).unwrap(), b"dirty\n");
    assert_eq!(fs::read(&replacement).unwrap(), b"other\n");
    assert!(fixture.buffer(path).is_none());
}

#[cfg(windows)]
#[test]
fn held_parent_cannot_be_renamed_while_a_save_holds_it() {
    let path = "dir/doc.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let mut rename_refused = false;

    let result = save_document_with(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource(path),
        |stage| {
            if stage == SaveStage::ParentHeld {
                rename_refused =
                    fs::rename(fixture.root.join("dir"), fixture.root.join("dir-moved")).is_err();
            }
            Ok(())
        },
    )
    .unwrap();

    assert!(rename_refused);
    assert_eq!(result.outcome, "saved");
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"dirty\n");
    assert!(fixture.buffer(path).is_none());
    // Once the save releases its hold, the same rename succeeds.
    fs::rename(fixture.root.join("dir"), fixture.root.join("dir-moved")).unwrap();
}

#[cfg(windows)]
#[test]
fn save_reports_a_target_held_open_without_delete_sharing_and_removes_the_temp() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    let path = "held.txt";
    let fixture = dirty_fixture(path, b"base\n", "dirty\n");
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(fixture.root.join(path))
        .unwrap();

    let result = save_document(fixture.database(), &fixture.workspace, path);

    assert!(matches!(result, Err(AppError::Io(_))));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"base\n");
    assert_failed_save_kept_buffer(&fixture, path, "dirty\n", &fixture.root);
    drop(holder);

    // The durable edit can be saved once the external sharing lock is released.
    let saved = save_document(fixture.database(), &fixture.workspace, path).unwrap();
    assert_eq!(saved.outcome, "saved");
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"dirty\n");
    assert!(fixture.buffer(path).is_none());
    assert_eq!(temp_entries(&fixture.root), 0);
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
    fn sizes(database_path: &Path) -> (u64, u64) {
        (
            file_size(database_path),
            file_size(&sqlite_sidecar_path(database_path, "-wal")),
        )
    }

    let fixture = Fixture::new();
    let database_path = fixture.root.join("sage.db");
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
    let one = sizes(&database_path);
    for document in opened_documents.iter().skip(1) {
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(document, content.clone(), 1),
        )
        .unwrap();
    }
    let sixteen = sizes(&database_path);
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
    let repeated = sizes(&database_path);
    println!(
        "sqlite_growth buffer_bytes=65536 one_db={} one_wal={} sixteen_db={} sixteen_wal={} repeated_100_db={} repeated_100_wal={}",
        one.0, one.1, sixteen.0, sixteen.1, repeated.0, repeated.1
    );
}

fn rebase_request(
    document: &EditorDocument,
    expected_disk: DocumentVersion,
    content: &str,
    revision: u64,
) -> BufferRebaseRequest {
    BufferRebaseRequest {
        resource: document.resource.clone(),
        expected_disk,
        line_ending: document.line_ending.clone(),
        content: content.into(),
        revision,
    }
}

#[test]
fn keep_mine_rebases_the_buffer_so_the_save_overwrites_the_change() {
    let fixture = dirty_fixture("keep.txt", b"base\n", "mine\n");
    fixture.write("keep.txt", b"theirs\n");
    let seen = open_document(fixture.database(), &fixture.workspace, "keep.txt").unwrap();
    assert_eq!(seen.conflict.as_ref().unwrap().kind, "external-change");

    let rebased = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&seen, seen.disk.clone(), "mine\n", 2),
    )
    .unwrap();

    assert_eq!(rebased.outcome, "rebased");
    assert!(rebased.document.conflict.is_none());
    assert!(rebased.document.dirty);
    assert_eq!(rebased.document.base.fingerprint, seen.disk.fingerprint);
    let saved = save_document(fixture.database(), &fixture.workspace, "keep.txt").unwrap();
    assert_eq!(saved.outcome, "saved");
    assert_eq!(fs::read(fixture.root.join("keep.txt")).unwrap(), b"mine\n");
    assert!(fixture.buffer("keep.txt").is_none());
}

#[test]
fn keep_mine_is_refused_when_disk_moved_on_again() {
    let fixture = dirty_fixture("again.txt", b"base\n", "mine\n");
    fixture.write("again.txt", b"first change\n");
    let seen = open_document(fixture.database(), &fixture.workspace, "again.txt").unwrap();
    fixture.write("again.txt", b"second change\n");

    let refused = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&seen, seen.disk.clone(), "mine, edited\n", 2),
    )
    .unwrap();

    assert_eq!(refused.outcome, "conflict");
    assert_ne!(refused.document.disk.fingerprint, seen.disk.fingerprint);
    let buffer = fixture.buffer("again.txt").unwrap();
    assert_eq!(buffer.content, "mine\n");
    assert_eq!(buffer.base_fingerprint, seen.base.fingerprint);
    assert_eq!(
        fs::read(fixture.root.join("again.txt")).unwrap(),
        b"second change\n"
    );
}

#[test]
fn a_deleted_file_opens_as_its_buffer_and_keep_mine_saves_it_back() {
    let fixture = dirty_fixture("gone/kept.txt", b"base\n", "kept\n");
    fs::remove_file(fixture.root.join("gone/kept.txt")).unwrap();

    let deleted = open_document(fixture.database(), &fixture.workspace, "gone/kept.txt").unwrap();
    assert_eq!(deleted.conflict.as_ref().unwrap().kind, "deleted");
    assert_eq!(deleted.buffer.content, "kept\n");
    assert_eq!(deleted.disk.fingerprint, ABSENT_FINGERPRINT);

    let rebased = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, deleted.disk.clone(), "kept\n", 2),
    )
    .unwrap();
    assert_eq!(rebased.outcome, "rebased");
    assert!(rebased.document.conflict.is_none());
    assert!(rebased.document.dirty);

    let saved = save_document(fixture.database(), &fixture.workspace, "gone/kept.txt").unwrap();
    assert_eq!(saved.outcome, "saved");
    assert_eq!(
        fs::read(fixture.root.join("gone/kept.txt")).unwrap(),
        b"kept\n"
    );
    assert!(fixture.buffer("gone/kept.txt").is_none());
}

#[test]
fn a_kept_deleted_file_is_not_overwritten_if_it_comes_back() {
    let fixture = dirty_fixture("back.txt", b"base\n", "kept\n");
    fs::remove_file(fixture.root.join("back.txt")).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, "back.txt").unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, deleted.disk.clone(), "kept\n", 2),
    )
    .unwrap();
    fixture.write("back.txt", b"recreated elsewhere\n");

    let result = save_document(fixture.database(), &fixture.workspace, "back.txt").unwrap();

    assert_eq!(result.outcome, "conflict");
    assert_eq!(
        fs::read(fixture.root.join("back.txt")).unwrap(),
        b"recreated elsewhere\n"
    );
    assert_eq!(fixture.buffer("back.txt").unwrap().content, "kept\n");
}

fn absent() -> DocumentVersion {
    super::absent::absent_version()
}

#[test]
fn a_kept_file_whose_folders_were_deleted_too_is_saved_with_them() {
    let fixture = dirty_fixture("gone/deeper/kept.txt", b"base\n", "kept\n");
    fs::remove_dir_all(fixture.root.join("gone")).unwrap();
    let deleted = open_document(
        fixture.database(),
        &fixture.workspace,
        "gone/deeper/kept.txt",
    )
    .unwrap();
    assert_eq!(deleted.conflict.as_ref().unwrap().kind, "deleted");

    let rebased = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();
    assert_eq!(rebased.outcome, "rebased");
    let saved = save_document(
        fixture.database(),
        &fixture.workspace,
        "gone/deeper/kept.txt",
    )
    .unwrap();

    assert_eq!(saved.outcome, "saved");
    assert_eq!(
        fs::read(fixture.root.join("gone/deeper/kept.txt")).unwrap(),
        b"kept\n"
    );
    assert_eq!(temp_entries(&fixture.root.join("gone/deeper")), 0);
}

#[test]
fn an_absent_base_needs_a_file_the_editor_opened() {
    let fixture = Fixture::new();
    let never = ResourceRef {
        root_id: fixture.workspace.root_id().into(),
        relative_path: "never-opened.txt".into(),
    };
    let document = EditorDocument {
        resource: never.clone(),
        disk: absent(),
        base: absent(),
        buffer: EditorBufferVersion {
            fingerprint: ABSENT_FINGERPRINT.into(),
            byte_len: 0,
            revision: 0,
            content: String::new(),
        },
        line_ending: "lf".into(),
        dirty: false,
        conflict: None,
    };

    let rebase = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&document, absent(), "created\n", 1),
    );
    let snapshot = snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&document, "created\n", 1),
    );

    assert!(rebase.is_err());
    assert!(snapshot.is_err());
    assert!(fixture.buffer("never-opened.txt").is_none());
    assert!(!fixture.root.join("never-opened.txt").exists());
}

#[test]
fn keep_mine_cannot_create_a_file_in_protected_metadata() {
    let fixture = dirty_fixture(".git/hooks/post-commit", b"base\n", "mine\n");
    fs::remove_file(fixture.root.join(".git/hooks/post-commit")).unwrap();
    let deleted = open_document(
        fixture.database(),
        &fixture.workspace,
        ".git/hooks/post-commit",
    )
    .unwrap();

    let refused = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "mine\n", 2),
    );

    assert!(refused.is_err());
    assert!(!fixture.root.join(".git/hooks/post-commit").exists());
    assert_eq!(
        fixture.buffer(".git/hooks/post-commit").unwrap().content,
        "mine\n"
    );
}

#[test]
fn an_opened_file_deleted_before_any_snapshot_keeps_its_edits() {
    let fixture = Fixture::new();
    fixture.write("fresh.txt", b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, "fresh.txt").unwrap();
    fs::remove_file(fixture.root.join("fresh.txt")).unwrap();

    snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&opened, "edited\n", 1),
    )
    .unwrap();

    let reopened = open_document(fixture.database(), &fixture.workspace, "fresh.txt").unwrap();
    assert_eq!(reopened.buffer.content, "edited\n");
    assert_eq!(reopened.conflict.unwrap().kind, "deleted");
}

#[test]
fn keep_mine_with_no_durable_buffer_starts_one_on_the_new_disk_version() {
    let fixture = Fixture::new();
    fixture.write("plain.txt", b"base\n");
    open_document(fixture.database(), &fixture.workspace, "plain.txt").unwrap();
    fixture.write("plain.txt", b"theirs\n");
    let seen = open_document(fixture.database(), &fixture.workspace, "plain.txt").unwrap();
    assert!(fixture.buffer("plain.txt").is_none());

    let rebased = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&seen, seen.disk.clone(), "mine\n", 1),
    )
    .unwrap();

    assert_eq!(rebased.outcome, "rebased");
    assert!(rebased.document.dirty);
    assert_eq!(
        save_document(fixture.database(), &fixture.workspace, "plain.txt")
            .unwrap()
            .outcome,
        "saved"
    );
    assert_eq!(fs::read(fixture.root.join("plain.txt")).unwrap(), b"mine\n");
}

#[test]
fn a_failed_create_leaves_no_file_and_keeps_the_buffer() {
    let fixture = dirty_fixture("lost.txt", b"base\n", "kept\n");
    fs::remove_file(fixture.root.join("lost.txt")).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, "lost.txt").unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();
    let buffer = fixture.buffer("lost.txt").unwrap();

    let failed = super::absent::create_document_with(
        fixture.database(),
        &fixture.workspace,
        fixture.resource_unchecked("lost.txt"),
        buffer,
        || Err(std::io::Error::other("the disk is full")),
    );

    assert!(failed.is_err());
    assert!(!fixture.root.join("lost.txt").exists());
    assert_eq!(temp_entries(&fixture.root), 0);
    let kept = fixture.buffer("lost.txt").unwrap();
    assert_eq!(kept.content, "kept\n");
    assert!(kept.pending_temp_path.is_none());
}

#[test]
fn a_file_that_appears_just_before_the_create_is_not_overwritten() {
    let fixture = dirty_fixture("raced.txt", b"base\n", "kept\n");
    fs::remove_file(fixture.root.join("raced.txt")).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, "raced.txt").unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();
    let buffer = fixture.buffer("raced.txt").unwrap();
    let target = fixture.root.join("raced.txt");

    let result = super::absent::create_document_with(
        fixture.database(),
        &fixture.workspace,
        fixture.resource_unchecked("raced.txt"),
        buffer,
        || fs::write(&target, b"theirs\n"),
    )
    .unwrap();

    assert_eq!(result.outcome, "conflict");
    assert_eq!(fs::read(&target).unwrap(), b"theirs\n");
    assert_eq!(temp_entries(&fixture.root), 0);
    assert_eq!(fixture.buffer("raced.txt").unwrap().content, "kept\n");
}

#[test]
fn a_failed_create_removes_only_the_folders_it_made() {
    let path = "kept/gone/deeper/lost.txt";
    let fixture = dirty_fixture(path, b"base\n", "kept\n");
    fs::remove_dir_all(fixture.root.join("kept/gone")).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();
    let buffer = fixture.buffer(path).unwrap();

    let failed = super::absent::create_document_with(
        fixture.database(),
        &fixture.workspace,
        fixture.resource_unchecked(path),
        buffer,
        || Err(std::io::Error::other("the disk is full")),
    );

    assert!(failed.is_err());
    assert!(fixture.root.join("kept").is_dir());
    assert!(!fixture.root.join("kept/gone").exists());
    assert_eq!(fixture.buffer(path).unwrap().content, "kept\n");
}

#[test]
fn an_unbuffered_snapshot_of_a_deleted_file_must_name_the_version_opened() {
    let fixture = Fixture::new();
    fixture.write("opened.txt", b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, "opened.txt").unwrap();
    fs::remove_file(fixture.root.join("opened.txt")).unwrap();
    let mut other = opened.clone();
    other.base.fingerprint = "0".repeat(opened.base.fingerprint.len());

    let refused = snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        snapshot_request(&other, "edited\n", 1),
    );

    assert!(refused.is_err());
    assert!(fixture.buffer("opened.txt").is_none());
}

#[cfg(unix)]
#[test]
fn link_and_copy_create_the_target_only_when_it_is_missing() {
    use super::filesystem::HeldParent;

    let fixture = Fixture::new();
    fixture.write("dir/temp", b"mine\n");
    fixture.write("dir/taken", b"theirs\n");
    let parent = HeldParent::open(&fixture.workspace, "dir").unwrap();

    assert!(parent.link_new("temp", "linked").unwrap());
    assert!(parent.copy_new("temp", "copied").unwrap());
    assert!(!parent.link_new("temp", "taken").unwrap());
    assert!(!parent.copy_new("temp", "taken").unwrap());

    let dir = fixture.root.join("dir");
    assert_eq!(fs::read(dir.join("linked")).unwrap(), b"mine\n");
    assert_eq!(fs::read(dir.join("copied")).unwrap(), b"mine\n");
    assert_eq!(fs::read(dir.join("taken")).unwrap(), b"theirs\n");
}

#[test]
fn keep_mine_on_a_deleted_file_that_came_back_shows_it_as_changed() {
    let fixture = Fixture::new();
    fixture.write("back.txt", b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, "back.txt").unwrap();
    fs::remove_file(fixture.root.join("back.txt")).unwrap();
    fixture.write("back.txt", b"theirs\n");

    let refused = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&opened, absent(), "mine\n", 1),
    )
    .unwrap();

    assert_eq!(refused.outcome, "conflict");
    assert_eq!(refused.document.conflict.unwrap().kind, "external-change");
    assert_ne!(refused.document.disk.fingerprint, ABSENT_FINGERPRINT);
    // Keep mine on the File as it is now goes through.
    let kept = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&opened, refused.document.disk.clone(), "mine\n", 1),
    )
    .unwrap();
    assert_eq!(kept.outcome, "rebased");
    assert_eq!(
        save_document(fixture.database(), &fixture.workspace, "back.txt")
            .unwrap()
            .outcome,
        "saved"
    );
    assert_eq!(fs::read(fixture.root.join("back.txt")).unwrap(), b"mine\n");
}

#[test]
fn keep_mine_on_a_file_that_came_back_with_the_same_text_has_no_conflict() {
    let fixture = Fixture::new();
    fixture.write("same.txt", b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, "same.txt").unwrap();
    fs::remove_file(fixture.root.join("same.txt")).unwrap();
    fixture.write("same.txt", b"mine\n");

    let result = rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&opened, absent(), "mine\n", 1),
    )
    .unwrap();

    assert_eq!(result.outcome, "matched");
    assert!(result.document.conflict.is_none());
    assert_eq!(result.document.buffer.content, "mine\n");
}

#[test]
fn keep_mine_never_reports_older_recovery_text_as_matching_a_restored_file() {
    // A recovery copy of "older" over "base"; the editor has moved on to
    // "newest", not yet snapshotted. The File is deleted, then restored
    // unchanged before its notice arrives.
    let fixture = dirty_fixture("restored.txt", b"base\n", "older\n");
    fs::remove_file(fixture.root.join("restored.txt")).unwrap();
    fixture.write("restored.txt", b"base\n");
    let resource = fixture.resource_unchecked("restored.txt");
    let request = BufferRebaseRequest {
        resource,
        expected_disk: absent(),
        line_ending: "lf".into(),
        content: "newest\n".into(),
        revision: 3,
    };

    let result = rebase_buffer(fixture.database(), &fixture.workspace, request.clone()).unwrap();

    assert_eq!(result.outcome, "conflict");
    assert_eq!(result.document.conflict.unwrap().kind, "external-change");
    assert_ne!(result.document.disk.fingerprint, ABSENT_FINGERPRINT);
    assert_eq!(fixture.buffer("restored.txt").unwrap().content, "older\n");
    assert_eq!(
        fs::read(fixture.root.join("restored.txt")).unwrap(),
        b"base\n"
    );

    // Disk holding the editor's own text matches, and the copy goes.
    fixture.write("restored.txt", b"newest\n");
    let matched = rebase_buffer(fixture.database(), &fixture.workspace, request).unwrap();
    assert_eq!(matched.outcome, "matched");
    assert!(matched.document.conflict.is_none());
    assert!(fixture.buffer("restored.txt").is_none());
}

#[test]
fn reload_of_a_file_that_is_gone_keeps_the_buffer() {
    let fixture = dirty_fixture("gone.txt", b"base\n", "mine\n");
    fs::remove_file(fixture.root.join("gone.txt")).unwrap();

    let reverted = revert_document(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource_unchecked("gone.txt"),
    )
    .unwrap();

    assert_eq!(reverted.conflict.unwrap().kind, "deleted");
    assert_eq!(reverted.buffer.content, "mine\n");
    assert_eq!(fixture.buffer("gone.txt").unwrap().content, "mine\n");
    // Reload of a File that is there still drops the buffer.
    fixture.write("gone.txt", b"back\n");
    let reloaded = revert_document(
        fixture.database(),
        &fixture.workspace,
        &fixture.resource_unchecked("gone.txt"),
    )
    .unwrap();
    assert_eq!(reloaded.buffer.content, "back\n");
    assert!(fixture.buffer("gone.txt").is_none());
}

#[test]
fn a_kept_file_is_created_again_in_a_folder_of_many_entries() {
    let path = "big/gone/kept.txt";
    let fixture = dirty_fixture(path, b"base\n", "kept\n");
    for index in 0..600 {
        fixture.write(&format!("big/entry-{index}.txt"), b"");
    }
    fs::remove_dir_all(fixture.root.join("big/gone")).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();

    let saved = save_document(fixture.database(), &fixture.workspace, path).unwrap();

    assert_eq!(saved.outcome, "saved");
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"kept\n");
}

/// Keeps `path` after its folder `folder` is deleted, then puts `link` in
/// the folder's place.
fn kept_behind_a_link(path: &str, folder: &str, link: impl FnOnce(&Path)) -> Fixture {
    let fixture = dirty_fixture(path, b"base\n", "kept\n");
    fs::remove_dir_all(fixture.root.join(folder)).unwrap();
    let deleted = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    rebase_buffer(
        fixture.database(),
        &fixture.workspace,
        rebase_request(&deleted, absent(), "kept\n", 2),
    )
    .unwrap();
    link(&fixture.root.join(folder));
    fixture
}

fn assert_refused_behind_a_link(fixture: &Fixture, path: &str, outside: &Path) {
    let refused = save_document(fixture.database(), &fixture.workspace, path);

    let message = refused.unwrap_err().to_string();
    assert!(
        message.contains("escapes the workspace") || message.contains("link or not a folder"),
        "{message}"
    );
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    assert_eq!(fixture.buffer(path).unwrap().content, "kept\n");
}

#[cfg(unix)]
#[test]
fn recreating_a_folder_never_follows_a_link_out_of_the_project() {
    let outside = std::env::temp_dir().join(format!("sage-outside-{}", Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();
    for (path, folder) in [
        ("linked/kept.txt", "linked"),
        ("deep/linked/more/kept.txt", "deep/linked"),
    ] {
        let fixture = kept_behind_a_link(path, folder, |at| {
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&outside, at).unwrap();
        });
        assert_refused_behind_a_link(&fixture, path, &outside);
    }
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn recreating_a_folder_never_follows_a_junction_out_of_the_project() {
    let outside = std::env::temp_dir().join(format!("sage-outside-{}", Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();
    let fixture = kept_behind_a_link("joined/kept.txt", "joined", |at| {
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(at)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
    });
    assert_refused_behind_a_link(&fixture, "joined/kept.txt", &outside);
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn a_snapshot_says_whether_its_text_is_the_stored_buffer() {
    let fixture = Fixture::new();
    let path = "acked.txt";
    fixture.write(path, b"base\n");
    let opened = open_document(fixture.database(), &fixture.workspace, path).unwrap();
    let snapshot = |content: &str, revision| {
        snapshot_buffer(
            fixture.database(),
            &fixture.workspace,
            snapshot_request(&opened, content, revision),
        )
    };

    assert!(snapshot("two\n", 2).unwrap().applied);
    // The same revision with the same text is already applied.
    assert!(snapshot("two\n", 2).unwrap().applied);
    // An older revision is not applied, and says what is stored.
    let older = snapshot("one\n", 1).unwrap();
    assert!(!older.applied);
    assert_eq!(older.revision, 2);
    // The same revision with other text is an error, never an ack, whether
    // the text is dirty or the File's own.
    assert!(snapshot("other\n", 2).is_err());
    assert!(snapshot("base\n", 2).is_err());
    assert_eq!(fixture.buffer(path).unwrap().content, "two\n");
}
