use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sage-macos-review-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn workspace_mask_excludes_editor_revoke() {
    assert_eq!(WORKSPACE_MASK, 0x2f);
    assert_eq!(WORKSPACE_MASK & NOTE_REVOKE, 0);
    let fixture = Fixture::new();
    let prepared = prepare(std::slice::from_ref(&fixture.0)).unwrap();
    assert!(prepared.target.is_none());
    assert!(prepared.initial_notice.is_none());
}

#[test]
fn editor_parent_symlink_is_rejected_but_workspace_mode_is_unchanged() {
    let fixture = Fixture::new();
    let parent = fixture.0.join("parent");
    let alias = fixture.0.join("alias");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(parent.join("notes.md"), "initial").unwrap();
    std::os::unix::fs::symlink(&parent, &alias).unwrap();
    assert!(prepare_editor(&alias.join("notes.md")).is_err());
    assert!(prepare(&[alias]).is_ok());
}

#[test]
fn held_parent_replacement_cannot_redirect_target_reopen() {
    let fixture = Fixture::new();
    let parent = fixture.0.join("parent");
    let moved = fixture.0.join("moved");
    let outside = fixture.0.join("outside");
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(parent.join("notes.md"), "initial").unwrap();
    std::fs::write(outside.join("notes.md"), "external").unwrap();
    let mut prepared = prepare_editor(&parent.join("notes.md")).unwrap();
    let original = prepared
        .target
        .as_ref()
        .unwrap()
        .file
        .as_ref()
        .unwrap()
        .metadata()
        .unwrap();
    std::fs::rename(&parent, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &parent).unwrap();
    assert!(prepared.reconcile_target().is_err());
    let retained = prepared
        .target
        .as_ref()
        .unwrap()
        .file
        .as_ref()
        .unwrap()
        .metadata()
        .unwrap();
    assert_eq!(
        (retained.dev(), retained.ino()),
        (original.dev(), original.ino())
    );
}

#[test]
fn missing_prepare_notice_is_cancelled_without_delivery() {
    let fixture = Fixture::new();
    let prepared = prepare_editor(&fixture.0.join("missing.md")).unwrap();
    assert_eq!(
        prepared.initial_notice,
        Some(unix_events::UnixWatchKind::Deleted)
    );
    run(prepared, &AtomicBool::new(true), &mut |_, _| {
        panic!("cancelled initial notice")
    })
    .unwrap();
}

#[test]
fn editor_intermediate_symlink_is_rejected() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.0.join("real/parent")).unwrap();
    std::fs::write(fixture.0.join("real/parent/notes.md"), "initial").unwrap();
    std::os::unix::fs::symlink(fixture.0.join("real"), fixture.0.join("alias")).unwrap();
    assert!(prepare_editor(&fixture.0.join("alias/parent/notes.md")).is_err());
}

#[test]
fn workspace_event_error_retains_old_resync_lifecycle() {
    let event = Kevent {
        flags: EV_ERROR,
        data: 9,
        ..Kevent::empty()
    };
    assert!(event_error(false, &event).is_ok());
    assert_eq!(
        event_error(true, &event).unwrap_err().raw_os_error(),
        Some(9)
    );
}
