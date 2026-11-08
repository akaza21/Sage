use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use uuid::Uuid;

use super::*;
use crate::editor::recover_pending_saves;

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    database: Option<Database>,
}

impl Fixture {
    fn new(path: &str, content: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!("sage-disk-replace-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(path), content).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
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

    fn resource(&self, path: &str) -> ResourceRef {
        self.workspace.resource(path).unwrap()
    }

    fn fingerprint(&self, path: &str) -> String {
        read_for_replace(self.database(), &self.workspace, &self.resource(path))
            .unwrap()
            .fingerprint
    }

    fn record(&self, path: &str) -> Option<PersistedEditorBuffer> {
        self.database()
            .editor_buffer(self.workspace.project_id(), &self.resource(path))
            .unwrap()
    }

    fn replace_with(
        &self,
        path: &str,
        observe: impl FnMut(ReplaceStage) -> std::io::Result<()>,
    ) -> AppResult<DiskReplace> {
        let expected = self.fingerprint(path);
        replace_on_disk_with(
            self.database(),
            &self.workspace,
            &self.resource(path),
            &expected,
            |text| Ok((text.replace("old", "new"), 1)),
            observe,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.database.take());
        let _ = fs::remove_dir_all(&self.root);
    }
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

#[test]
fn a_write_after_the_temp_file_makes_the_replace_stale_and_leaves_nothing() {
    let path = "notes.txt";
    let fixture = Fixture::new(path, b"old\n");
    let mut recorded = None;

    let result = fixture.replace_with(path, |stage| {
        if stage == ReplaceStage::BeforeStaleCheck {
            recorded = fixture
                .record(path)
                .and_then(|record| record.pending_temp_path);
            fs::write(fixture.root.join(path), b"theirs old\n")?;
        }
        Ok(())
    });

    assert_eq!(result.unwrap(), DiskReplace::Stale);
    assert!(recorded.is_some_and(|temp| temp.contains(".sage-")));
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"theirs old\n");
    assert_eq!(temp_entries(&fixture.root), 0);
    assert!(fixture.record(path).is_none());
}

#[test]
fn a_crash_before_the_swap_leaves_recovery_the_temp_file_and_the_replaced_text() {
    let path = "crash.txt";
    let fixture = Fixture::new(path, b"old one\n");

    let crashed = catch_unwind(AssertUnwindSafe(|| {
        fixture.replace_with(path, |stage| {
            if stage == ReplaceStage::BeforeReplace {
                panic!("injected crash");
            }
            Ok(())
        })
    }));
    assert!(crashed.is_err());
    assert_eq!(temp_entries(&fixture.root), 1);

    recover_pending_saves(fixture.database(), &fixture.workspace).unwrap();

    assert_eq!(temp_entries(&fixture.root), 0);
    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"old one\n");
    let kept = fixture.record(path).unwrap();
    assert_eq!(kept.content, "new one\n");
    assert!(kept.pending_temp_path.is_none());
}

#[test]
fn a_crash_after_the_swap_leaves_recovery_nothing_to_keep() {
    let path = "swapped.txt";
    let fixture = Fixture::new(path, b"old one\n");

    let crashed = catch_unwind(AssertUnwindSafe(|| {
        fixture.replace_with(path, |stage| {
            if stage == ReplaceStage::AfterReplace {
                panic!("injected crash");
            }
            Ok(())
        })
    }));
    assert!(crashed.is_err());
    assert!(fixture.record(path).is_some());

    recover_pending_saves(fixture.database(), &fixture.workspace).unwrap();

    assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"new one\n");
    assert_eq!(temp_entries(&fixture.root), 0);
    assert!(fixture.record(path).is_none());
}

#[test]
fn a_failure_before_the_swap_removes_the_temp_file_and_the_record() {
    for stage in [
        ReplaceStage::Recorded,
        ReplaceStage::TempWritten,
        ReplaceStage::BeforeStaleCheck,
        ReplaceStage::BeforeReplace,
    ] {
        let path = "fails.txt";
        let fixture = Fixture::new(path, b"old\n");

        let result = fixture.replace_with(path, |current| {
            if current == stage {
                Err(std::io::Error::other("injected failure"))
            } else {
                Ok(())
            }
        });

        assert!(result.is_err(), "{stage:?}");
        assert_eq!(fs::read(fixture.root.join(path)).unwrap(), b"old\n");
        assert_eq!(temp_entries(&fixture.root), 0, "{stage:?}");
        assert!(fixture.record(path).is_none(), "{stage:?}");
    }
}

#[test]
fn an_unverified_swap_is_written_and_kept_as_unsaved_edits() {
    let path = "moved.txt";
    let fixture = Fixture::new(path, b"old\n");

    let result = fixture.replace_with(path, |stage| {
        if stage == ReplaceStage::AfterReplace {
            fs::write(fixture.root.join(path), b"someone else\n")?;
        }
        Ok(())
    });

    let message = result.unwrap_err().to_string();
    assert!(message.contains("was written"), "{message}");
    assert_eq!(fixture.record(path).unwrap().content, "new\n");
}

#[test]
fn a_byte_order_mark_is_kept_as_a_save_keeps_it() {
    let path = "bom.txt";
    let fixture = Fixture::new(path, b"\xef\xbb\xbfold\r\n");

    assert!(matches!(
        fixture.replace_with(path, |_| Ok(())),
        Ok(DiskReplace::Written { .. })
    ));
    assert_eq!(
        fs::read(fixture.root.join(path)).unwrap(),
        b"\xef\xbb\xbfnew\r\n"
    );
}

#[test]
fn files_a_save_could_not_write_back_are_refused() {
    for (name, bytes) in [
        ("latin1.txt", b"old caf\xe9\n".as_slice()),
        ("mixed.txt", b"old\r\nold\n".as_slice()),
    ] {
        let fixture = Fixture::new(name, bytes);
        let resource = fixture.resource(name);
        assert!(
            read_for_replace(fixture.database(), &fixture.workspace, &resource).is_err(),
            "{name}"
        );
        let applied = replace_on_disk(
            fixture.database(),
            &fixture.workspace,
            &resource,
            &fingerprint(bytes),
            |text| Ok((text.replace("old", "new"), 1)),
        );
        assert!(applied.is_err(), "{name}");
        assert_eq!(fs::read(fixture.root.join(name)).unwrap(), bytes, "{name}");
        assert_eq!(temp_entries(&fixture.root), 0, "{name}");
    }
}
