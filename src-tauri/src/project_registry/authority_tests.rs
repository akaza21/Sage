use super::*;

#[cfg(windows)]
#[test]
fn authority_identity_preserves_case_while_accepting_equivalent_prefixes() {
    let base = std::env::temp_dir().canonicalize().unwrap();
    let upper = base.join("Repo");
    let lower = base.join("repo");
    assert_ne!(authority_path_key(&upper), authority_path_key(&lower));
    assert_eq!(
        conservative_overlap_key(&upper),
        conservative_overlap_key(&lower)
    );
    let ordinary = sage_core::win32_path::simplified(&upper);
    assert_eq!(authority_path_key(&upper), authority_path_key(&ordinary));
}

#[cfg(windows)]
#[test]
fn stored_root_rejects_case_sensitive_junction_substitution() {
    use super::create::ProjectCreateStore;
    use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId, SageRootId};
    use std::{fs, process::Command};

    let base = std::env::temp_dir().join(format!("sage-case-{}", Uuid::new_v4()));
    fs::create_dir(&base).unwrap();
    let enabled = Command::new("fsutil")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(&base)
        .arg("enable")
        .output()
        .expect("run fsutil to measure case-sensitivity support");
    // fsutil can return success even when setting the flag was denied.
    use std::{
        mem::size_of,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FileCaseSensitiveInfo,
        GetFileInformationByHandleEx,
    };
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(&base)
        .unwrap();
    let mut info = FILE_CASE_SENSITIVE_INFO { Flags: 0 };
    assert_ne!(
        unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle().cast(),
                FileCaseSensitiveInfo,
                (&mut info as *mut FILE_CASE_SENSITIVE_INFO).cast(),
                size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
            )
        },
        0,
        "query the case-sensitive directory flag: {}",
        std::io::Error::last_os_error()
    );
    drop(directory);
    // FILE_CS_FLAG_CASE_SENSITIVE_DIR is not exported by windows-sys.
    const CASE_SENSITIVE_DIR: u32 = 1;
    if info.Flags & CASE_SENSITIVE_DIR == 0 {
        eprintln!(
            "SKIP case-sensitive substitution: fsutil could not enable it: status={} stdout={} stderr={}",
            enabled.status,
            String::from_utf8_lossy(&enabled.stdout),
            String::from_utf8_lossy(&enabled.stderr)
        );
        fs::remove_dir(&base).unwrap();
        return;
    }
    let root = base.join("Repo");
    let other = base.join("repo");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&other).unwrap();
    assert_ne!(
        sage_workspace::root_identity_path(&root).unwrap(),
        sage_workspace::root_identity_path(&other).unwrap()
    );
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_with_chat(&root).unwrap();
    fs::remove_dir(&root).unwrap();
    let junction = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&root)
        .arg(&other)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "junction creation failed: {junction:?}"
    );
    assert_eq!(root.canonicalize().unwrap(), other.canonicalize().unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let state = crate::state::AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id.clone()).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let root_id = SageRootId::parse(project.roots[0].id.clone()).unwrap();
        assert!(database.project_root_authority(&context, &root_id).is_err());
        assert!(Workspace::open_registered_root(&project.id, root_id.as_str(), &root).is_err());
    });
    fs::remove_dir(&root).unwrap();
    fs::remove_dir_all(&base).unwrap();
}

#[cfg(windows)]
#[test]
fn stored_workspace_rejects_junction_substitution_without_case_sensitivity() {
    use super::create::ProjectCreateStore;
    use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId};
    use std::{fs, process::Command};
    let base = std::env::temp_dir().join(format!("sage-junction-{}", Uuid::new_v4()));
    let root = base.join("Repo");
    let target = base.join("other").join("repo");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&target).unwrap();
    let canonical = root.canonicalize().unwrap();
    assert!(Workspace::open_registered_root("project", "root", &canonical).is_ok());
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_with_chat(&root).unwrap();
    fs::remove_dir(&root).unwrap();
    let junction = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&root)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "junction creation failed: {junction:?}"
    );
    assert_ne!(
        authority_path_key(&canonical),
        authority_path_key(&root.canonicalize().unwrap())
    );
    assert!(Workspace::open_registered_root("project", "root", &canonical).is_err());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let state = crate::state::AppState::new(database.clone());
        let project_id = SageProjectId::parse(project.id).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        assert!(database.selected_project_root_authority(&context).is_err());
        assert!(
            database
                .resolve_active_resource(&ResourceRef {
                    root_id: project.roots[0].id.clone(),
                    relative_path: String::new(),
                })
                .is_err()
        );
    });
    fs::remove_dir(&root).unwrap();
    fs::remove_dir_all(&base).unwrap();
}
