use super::*;
use crate::chat_archive::{ChatArchiveStore, delete::ArchivedChatDeleteStore};
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};
use sage_domain::chat::{ChatDeleteRequest, ChatLifecycleRequest};

pub(super) fn delete_chat(database: &Database, chat: &str) {
    let current = super::tests::layout(database);
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: current.project_id.clone(),
            chat_id: chat.into(),
            current_layout: current.clone(),
        })
        .unwrap();
    database
        .delete_archived_chat(&ChatDeleteRequest {
            project_id: current.project_id,
            chat_id: chat.into(),
        })
        .unwrap();
}

#[test]
fn home_project_does_not_exhaust_private_storage_after_repeated_deletes() {
    let home = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-home-{}", Uuid::new_v4()));
    fs::create_dir_all(home.join("profile")).unwrap();
    let database = crate::database::open_database(&home.join("profile/sage.db")).unwrap();
    let project = database.create_project_with_chat(&home).unwrap();
    fs::write(home.join("keep.txt"), "home files").unwrap();
    for _ in 0..=MAX_PRIVATE_PROJECTS {
        let chat = Uuid::new_v4().to_string();
        create(&database, None, &chat).unwrap();
        let scope = super::tests::layout(&database);
        let folder = database.root_records(&scope.project_id).unwrap()[0]
            .absolute_path
            .clone();
        fs::write(folder.join("scratch.txt"), "private work").unwrap();
        delete_chat(&database, &chat);
        assert!(!folder.exists(), "deleted chat must relinquish its folder");
        assert!(list(&database).unwrap().is_empty());
    }
    assert_eq!(
        fs::read_to_string(home.join("keep.txt")).unwrap(),
        "home files"
    );
    assert!(database.project_summary(&project.id).is_ok());
    drop(database);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn round3_capacity_keeps_failed_cleanup_owned_until_files_are_reclaimed() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-capacity-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("sage.db");
    let database = crate::database::open_database(&path).unwrap();
    let base = storage_root(&database).unwrap();
    fs::create_dir_all(&base).unwrap();
    let mut folders = Vec::new();
    for index in 0..MAX_PRIVATE_PROJECTS {
        let id = Uuid::new_v4().to_string();
        let folder = base.join(&id);
        let mut deep = folder.clone();
        for _ in 0..65 {
            deep = deep.join("d");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("keep"), "pending reclamation").unwrap();
        database.connection().execute("INSERT INTO projects(id,name,kind,created_at,updated_at,last_opened_at) VALUES (?1,'Choose project','privateChat',?2,?2,0)", params![id, index]).unwrap();
        folders.push((id, folder, deep));
    }
    for _ in 0..3 {
        let result = create(&database, None, &Uuid::new_v4().to_string());
        assert!(
            result.is_err(),
            "allocation must wait for actual folder reclamation"
        );
        assert!(result.unwrap_err().public_message().contains("cleanup"));
        for (id, _, deep) in &folders {
            assert!(
                database.project_summary(id).is_ok(),
                "retain cleanup ownership"
            );
            assert_eq!(
                fs::read_to_string(deep.join("keep")).unwrap(),
                "pending reclamation"
            );
        }
        assert_eq!(
            fs::read_dir(&base).unwrap().count(),
            MAX_PRIVATE_PROJECTS as usize
        );
    }
    for (_, folder, _) in &folders {
        fs::remove_dir_all(folder.join("d")).unwrap();
    }
    drop(database);
    let reopened = crate::database::open_database(&path).unwrap();
    cleanup::empty_projects(&reopened).unwrap();
    assert!(folders.iter().all(|(_, folder, _)| !folder.exists()));
    assert_eq!(fs::read_dir(base).unwrap().count(), 0);
    create(&reopened, None, &Uuid::new_v4().to_string()).unwrap();
    assert_eq!(list(&reopened).unwrap().len(), 1);
    assert!(list(&reopened).unwrap()[0].rail_chats.is_empty());
    assert_eq!(
        reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn registration_refuses_managed_private_folders_for_new_and_existing_projects() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-registration-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let real = database
        .create_project_with_chat(&root.join("user"))
        .unwrap();
    create(&database, None, &Uuid::new_v4().to_string()).unwrap();
    let scope = super::tests::layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    fs::create_dir(folder.join("nested")).unwrap();
    for path in [
        &folder,
        &folder.join("nested"),
        &storage_root(&database).unwrap(),
    ] {
        let error = database.create_project_with_chat(path).unwrap_err();
        assert!(error.public_message().contains("managed private Chat"));
        assert!(
            database
                .create_named_project_if_active(
                    "Unsafe",
                    std::slice::from_ref(path),
                    Some(&scope.project_id)
                )
                .is_err()
        );
    }
    // Adding a root uses the same admission rule.
    database
        .connection()
        .execute("UPDATE app_state SET active_project_id=?1", [&real.id])
        .unwrap();
    assert!(
        database
            .add_project_root(&real.id, &folder)
            .unwrap_err()
            .public_message()
            .contains("managed private Chat")
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_private_chat_preserves_overlapping_registered_project_files() {
    for relation in ["same", "child", "parent"] {
        let root = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("private-overlap-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("user")).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let real = database
            .create_project_with_chat(&root.join("user"))
            .unwrap();
        let chat = Uuid::new_v4().to_string();
        create(&database, None, &chat).unwrap();
        let scope = super::tests::layout(&database);
        let folder = database.root_records(&scope.project_id).unwrap()[0]
            .absolute_path
            .clone();
        fs::create_dir(folder.join("nested")).unwrap();
        let registered = match relation {
            "same" => folder.clone(),
            "child" => folder.join("nested"),
            _ => folder.parent().unwrap().to_path_buf(),
        };
        // Model a registration accepted before the admission fix.
        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?1, canonical_key=?2 WHERE project_id=?3",
                params![
                    registered.to_str().unwrap(),
                    crate::project_registry::conservative_overlap_key(&registered),
                    real.id
                ],
            )
            .unwrap();
        fs::write(folder.join("nested/keep.txt"), "user work").unwrap();
        delete_chat(&database, &chat);
        if relation == "parent" {
            assert!(
                !folder.exists(),
                "an ancestor does not own the managed folder"
            );
            assert!(registered.exists(), "the ancestor root itself survives");
        } else {
            assert_eq!(
                fs::read_to_string(folder.join("nested/keep.txt")).unwrap(),
                "user work",
                "{relation}"
            );
        }
        assert!(database.project_summary(&real.id).is_ok());
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn cleanup_retries_reach_later_folders_and_failures_do_not_exhaust_live_chat_capacity() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-retries-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let base = storage_root(&database).unwrap();
    fs::create_dir_all(&base).unwrap();
    let mut removable = Vec::new();
    for index in 0..128 {
        let id = Uuid::new_v4().to_string();
        let folder = base.join(&id);
        fs::create_dir(&folder).unwrap();
        let failing = index < 64;
        database.connection().execute("INSERT INTO projects(id,name,kind,created_at,updated_at,last_opened_at) VALUES (?1,'Choose project','privateChat',?2,?3,0)", params![id, if failing { 2 } else { 1 }, if failing { 0 } else { 1 }]).unwrap();
        if failing {
            let mut deep = folder;
            for _ in 0..65 {
                deep = deep.join("d");
            }
            fs::create_dir_all(&deep).unwrap();
            fs::write(deep.join("keep"), "depth bound").unwrap();
        } else {
            fs::write(folder.join("remove"), "stale").unwrap();
            removable.push(folder);
        }
    }
    // Admission waits for real reclamation, while fair rotation lets a later
    // attempt reach removable receipts beyond the first failing batch.
    let chat = Uuid::new_v4().to_string();
    assert!(
        create(&database, None, &chat)
            .unwrap_err()
            .public_message()
            .contains("cleanup")
    );
    assert!(removable.iter().all(|folder| folder.exists()));
    create(&database, None, &chat).unwrap();
    let _ = cleanup::empty_projects(&database);
    let _ = cleanup::empty_projects(&database);
    assert!(removable.iter().all(|folder| !folder.exists()));
    assert_eq!(list(&database).unwrap().len(), 1);
    assert!(list(&database).unwrap()[0].rail_chats.is_empty());
    assert!(
        database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE kind='privateChat'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap()
            <= MAX_PRIVATE_PROJECTS
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_unlinks_directory_links_without_deleting_the_external_target() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-links-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("external")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let chat = Uuid::new_v4().to_string();
    create(&database, None, &chat).unwrap();
    let scope = super::tests::layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    let external = root.join("external").canonicalize().unwrap();
    fs::write(external.join("keep"), "outside private root").unwrap();
    #[cfg(windows)]
    {
        // Junctions require no symlink privilege on Windows.
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(folder.join(".venv"))
            .arg(&external)
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&external, folder.join(".venv")).unwrap();
        std::os::unix::fs::symlink(external.join("keep"), folder.join("tool")).unwrap();
        std::os::unix::fs::symlink(external.join("missing"), folder.join("dangling")).unwrap();
    }
    delete_chat(&database, &chat);
    assert!(!folder.exists());
    assert_eq!(
        fs::read_to_string(external.join("keep")).unwrap(),
        "outside private root"
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_preserves_files_when_another_root_identity_cannot_be_resolved() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-invalid-root-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let real = database
        .create_project_with_chat(&root.join("user"))
        .unwrap();
    let chat = Uuid::new_v4().to_string();
    create(&database, None, &chat).unwrap();
    let scope = super::tests::layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    fs::write(folder.join("keep"), "wait for root identity").unwrap();
    // A malformed persisted root cannot be resolved on either platform. It
    // must fail closed, rather than be mistaken for a missing directory.
    database
        .connection()
        .execute(
            "UPDATE project_roots SET absolute_path=?1 WHERE project_id=?2",
            params!["invalid\0root", real.id],
        )
        .unwrap();
    delete_chat(&database, &chat);
    assert_eq!(
        fs::read_to_string(folder.join("keep")).unwrap(),
        "wait for root identity"
    );
    database
        .connection()
        .execute(
            "UPDATE project_roots SET absolute_path=?1 WHERE project_id=?2",
            params![
                root.join("user").canonicalize().unwrap().to_str().unwrap(),
                real.id
            ],
        )
        .unwrap();
    cleanup::empty_projects(&database).unwrap();
    assert!(!folder.exists());
    drop(database);
    fs::remove_dir_all(root).unwrap();
}
