use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};

use uuid::Uuid;

use super::*;
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sage-{label}-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn exact_handle_rename_rejects_substitution_and_retains_effect_authority() {
    let root = TempRoot::new("exact-handle-rename");
    let stage = root.0.join("stage");
    let destination = root.0.join("destination");
    fs::create_dir(&stage).unwrap();
    fs::create_dir(&destination).unwrap();
    let source_path = stage.join("package");
    fs::create_dir(&source_path).unwrap();
    fs::write(source_path.join("identity"), b"original").unwrap();

    let (_, proof) = open_exact_state_root(&source_path).unwrap();
    let source_identity = node_identity(&proof).unwrap();
    fs::rename(&source_path, stage.join("displaced")).unwrap();
    fs::create_dir(&source_path).unwrap();
    assert!(matches!(
        open_rename_directory(&source_path, source_identity),
        Err(ElevatedHelperError::Drift)
    ));

    let source = open_rename_directory(&stage.join("displaced"), source_identity).unwrap();
    let destination_handle = open_destination_root(&destination).unwrap();
    rename_exact(&source, &destination_handle, OsStr::new("published"), false).unwrap();

    assert_eq!(
        node_identity(
            &open_exact_state_root(&destination.join("published"))
                .unwrap()
                .1
        )
        .unwrap(),
        source_identity
    );
    assert_ne!(
        open_mutation_directory(&source_path, None).unwrap().1,
        source_identity
    );

    assert!(fs::rename(destination.join("published"), destination.join("displaced")).is_err());
    let stage_handle = open_destination_root(&stage).unwrap();
    rename_exact(&source, &stage_handle, OsStr::new("returned"), false).unwrap();
    assert_eq!(
        node_identity(&open_exact_state_root(&stage.join("returned")).unwrap().1).unwrap(),
        source_identity
    );
    drop(stage_handle);
    drop(destination_handle);
    drop(source);
}

#[test]
fn recovery_rejects_same_user_replacement_of_journaled_state_identity() {
    let root = TempRoot::new("state-identity-swap");
    let state = root.0.join("state");
    fs::create_dir(&state).unwrap();
    let (held, journaled) = open_mutation_directory(&state, None).unwrap();
    drop(held);
    fs::rename(&state, root.0.join("displaced-state")).unwrap();
    fs::create_dir(&state).unwrap();

    assert!(matches!(
        open_mutation_directory(&state, Some(journaled)),
        Err(ElevatedHelperError::Drift)
    ));
}

#[test]
fn torn_initial_journal_temp_recovers_to_one_atomic_final_file() {
    let root = TempRoot::new("torn-initial-journal");
    let final_path = root.0.join("journal.json");
    let parent = open_destination_root(&root.0).unwrap();
    let bytes = br#"{"authenticated":"journal"}"#;
    inject_torn_initial_write(3);

    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            create_initial_journal(&final_path, bytes, &parent, "00")
        }))
        .is_err()
    );
    assert!(!final_path.exists());
    let temporary = PathBuf::from(format!("{}.creating", final_path.display()));
    assert_eq!(fs::read(&temporary).unwrap(), &bytes[..3]);

    create_initial_journal(&final_path, bytes, &parent, "11").unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), bytes);
    assert!(!temporary.exists());
    assert!(!PathBuf::from(format!("{}.creating-00", final_path.display())).exists());
    assert!(!PathBuf::from(format!("{}.creating-11", final_path.display())).exists());
    drop(parent);
}

#[test]
fn destination_parent_and_state_authority_deny_external_rename() {
    let root = TempRoot::new("destination-root-authority");
    let parent = root.0.join("state-parent");
    fs::create_dir(&parent).unwrap();
    let authority = open_destination_root(&parent).unwrap();
    assert!(fs::rename(&parent, root.0.join("moved-parent")).is_err());
    assert!(parent.is_dir());
    drop(authority);
    fs::rename(&parent, root.0.join("moved-parent")).unwrap();

    let state = root.0.join("state");
    fs::create_dir(&state).unwrap();
    let (state_authority, _) = open_mutation_directory(&state, None).unwrap();
    assert!(fs::rename(&state, root.0.join("moved-state")).is_err());
    assert!(state.is_dir());
    drop(state_authority);
}

#[test]
fn cleanup_retries_after_each_native_effect_without_requiring_handoff_or_stage() {
    for point in [
        CleanupCrashPoint::HandoffRemoved,
        CleanupCrashPoint::StageRemoved,
    ] {
        let root = TempRoot::new("cleanup-substate");
        let stage = root.0.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("handoff.json"), b"sealed").unwrap();
        let mut stage_authority = Some(open_destination_root(&stage).unwrap());
        inject_cleanup_crash(point);

        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                cleanup_completed_stage(&stage, &mut stage_authority)
            }))
            .is_err(),
            "{point:?}"
        );
        cleanup_completed_stage(&stage, &mut stage_authority).unwrap();
        assert!(!stage.exists(), "{point:?}");
    }
}

#[test]
fn bounded_directory_scan_accepts_exact_limit_and_rejects_limit_plus_one() {
    let root = TempRoot::new("bounded-helper-scan");
    for name in ["handoff.json", "package", "state"] {
        fs::write(root.0.join(name), b"x").unwrap();
    }
    assert_eq!(
        bounded_names(&root.0, 3, ElevatedHelperError::Drift)
            .unwrap()
            .len(),
        3
    );
    fs::write(root.0.join("hostile-extra"), b"x").unwrap();
    assert_eq!(
        bounded_names(&root.0, 3, ElevatedHelperError::Drift),
        Err(ElevatedHelperError::Drift)
    );
}

#[test]
fn bounded_cleanup_scan_accepts_exact_limit_and_rejects_hostile_plus_one() {
    let root = TempRoot::new("bounded-helper-cleanup");
    fs::write(root.0.join("handoff.json"), b"x").unwrap();
    assert_eq!(
        bounded_names(&root.0, 1, ElevatedHelperError::Cleanup)
            .unwrap()
            .len(),
        1
    );
    fs::write(root.0.join("hostile-extra"), b"x").unwrap();
    assert_eq!(
        bounded_names(&root.0, 1, ElevatedHelperError::Cleanup),
        Err(ElevatedHelperError::Cleanup)
    );
}

#[test]
fn cleanup_directory_cap_stops_iteration_before_collecting_the_remainder() {
    struct MoreEntriesThanAllowed {
        next: usize,
    }

    impl Iterator for MoreEntriesThanAllowed {
        type Item = Result<usize, ()>;

        fn next(&mut self) -> Option<Self::Item> {
            assert!(
                self.next <= 3,
                "directory iteration exceeded the cap boundary"
            );
            let entry = self.next;
            self.next += 1;
            Some(Ok(entry))
        }
    }

    assert_eq!(
        bounded_directory_entries(
            MoreEntriesThanAllowed { next: 0 },
            3,
            ElevatedHelperError::Cleanup,
        ),
        Err(ElevatedHelperError::Cleanup)
    );
}
