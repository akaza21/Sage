use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use sage_core::error::{AppError, AppResult};

use crate::domain::{DirectoryListing, ResourceRef, TextFile, WorkspaceEntry};

#[cfg(windows)]
pub(crate) mod atomic_write_windows;
pub(crate) mod directory_skip;
pub(crate) mod file_create;
pub(crate) mod file_rename;
pub(crate) mod file_walk;
mod git_resource;
mod identity;
pub(crate) mod ignore_rules;
pub(crate) mod name_order;
pub(crate) mod notify;
pub(crate) mod path_glob;
pub(crate) mod path_index;
pub(crate) mod resource_trash;
pub(crate) mod search;
mod text_read;
#[allow(
    unused_imports,
    reason = "workspace read entrypoints are consumed by deferred ACP adapters"
)]
pub(crate) use text_read::{
    MAX_TEXT_FILE_BYTES, decode_text_bytes, open_workspace_text_file, read_text_bytes_bounded,
    select_text_lines,
};

#[cfg(windows)]
pub(crate) use identity::root_identity_file;
#[cfg(unix)]
pub(crate) use identity::root_identity_metadata;
pub(crate) use identity::{leaf_identity, root_identity_path};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use file_create::platform::open_parent as open_held_parent;

/// Windows uses the rename walk: each handle denies delete sharing, so no
/// component can be renamed or removed, but shares write, so a file inside
/// the held directory can still be replaced. Returns the root first and the
/// parent last.
#[cfg(windows)]
pub(crate) fn open_held_parent(
    root: &Path,
    expected_root: RootIdentity,
    relative_path: &str,
) -> AppResult<Vec<fs::File>> {
    file_rename::platform::open_parent(file_rename::platform::ParentAuthority {
        root,
        expected_root,
        relative_path,
    })
}

pub(crate) const MAX_DIRECTORY_ENTRIES: usize = 500;

#[derive(Debug, Clone)]
pub struct Workspace {
    project_id: String,
    root_id: String,
    root: PathBuf,
    root_identity: RootIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootIdentity {
    volume: u64,
    file_id: [u8; 16],
}

impl RootIdentity {
    pub(crate) fn stable_bytes(self) -> [u8; 24] {
        let mut bytes = [0_u8; 24];
        bytes[..8].copy_from_slice(&self.volume.to_be_bytes());
        bytes[8..].copy_from_slice(&self.file_id);
        bytes
    }
}

impl Workspace {
    pub fn open(project_id: &str, root_id: &str, root: &Path) -> AppResult<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(AppError::InvalidRequest(
                "workspace root must be a directory".into(),
            ));
        }
        Ok(Self {
            project_id: project_id.into(),
            root_id: root_id.into(),
            root_identity: root_identity_path(&root)?,
            root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn root_id(&self) -> &str {
        &self.root_id
    }

    pub(crate) fn root_identity(&self) -> RootIdentity {
        self.root_identity
    }

    pub(crate) fn ensure_root_identity(&self) -> AppResult<()> {
        if root_identity_path(&self.root)? != self.root_identity {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before Git access".into(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn resource(&self, relative_path: &str) -> AppResult<ResourceRef> {
        let resource = self.resource_unchecked(relative_path);
        let relative_path = self.validate_resource(&resource, false)?;
        Ok(self.resource_unchecked(&relative_path))
    }

    pub fn list_directory(&self, resource: &ResourceRef) -> AppResult<DirectoryListing> {
        let relative_path = self.validate_resource(resource, true)?;
        let directory = self.resolve_existing(&relative_path, true)?;
        let mut entries = Vec::new();
        let mut truncated = false;

        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            if entries.len() == MAX_DIRECTORY_ENTRIES {
                truncated = true;
                break;
            }

            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                "directory"
            } else if file_type.is_file() {
                "file"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            };
            let entry_name = entry.file_name();
            let name = entry_name.to_str().ok_or_else(|| {
                AppError::InvalidRequest(
                    "filesystem entry name is not valid UTF-8 and cannot be a ResourceRef".into(),
                )
            })?;
            let path = join_contract_path(&relative_path, name);
            validate_relative_path(&path, false)?;
            let byte_len = if file_type.is_file() {
                Some(entry.metadata()?.len())
            } else {
                None
            };
            entries.push(WorkspaceEntry {
                name: name.into(),
                resource: self.resource_unchecked(&path),
                kind: kind.into(),
                byte_len,
                ignored: false,
            });
        }
        // Explorer dims (or hides) what the ignore files ignore, whatever
        // `search.useIgnoreFiles` says, as VS Code dims Git-ignored entries.
        let rules = ignore_rules::IgnoreRules::load(
            &self.root,
            true,
            ignore_rules::global_excludes_path().as_deref(),
        );
        let (chain, folder_ignored) =
            rules.chain_for(&relative_path, ignore_rules::RuleFiles::probe(&directory));
        for entry in &mut entries {
            entry.ignored = folder_ignored
                || rules.is_ignored(
                    &chain,
                    &entry.resource.relative_path,
                    entry.kind == "directory",
                );
        }

        entries.sort_unstable_by(|left, right| {
            entry_rank(&left.kind)
                .cmp(&entry_rank(&right.kind))
                .then_with(|| name_order::explorer_name_cmp(&left.name, &right.name))
        });

        Ok(DirectoryListing {
            resource: self.resource_unchecked(&relative_path),
            entries,
            truncated,
        })
    }

    pub fn read_text_file(&self, resource: &ResourceRef) -> AppResult<TextFile> {
        let relative_path = self.validate_resource(resource, false)?;
        // Open through a held root before metadata/read. No later pathname is
        // reopened, closing final-link and parent/path replacement races.
        let mut file =
            open_workspace_text_file(&self.root, self.root_identity, &relative_path, None)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(AppError::InvalidRequest("resource is not a file".into()));
        }
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            return Err(AppError::ResourceLimit(format!(
                "file exceeds the {} MiB text-view limit",
                MAX_TEXT_FILE_BYTES / 1024 / 1024
            )));
        }
        // Metadata is only early rejection; the same handle gets the bounded
        // +1 probe, so a growing source cannot evade admission.
        let bytes = read_text_bytes_bounded(&mut file)?;
        let content = decode_text_bytes(bytes)?;
        let line_ending = detect_line_ending(&content).into();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);

        Ok(TextFile {
            resource: self.resource_unchecked(&relative_path),
            byte_len: content.len() as u64,
            content,
            line_ending,
            modified_at,
        })
    }

    /// P-02 leaf-identity read. Unlike `read_text_file`, this never
    /// canonicalizes the leaf and reopens it: component validation plus the
    /// held-root walk compare the admitted final handle before reading it.
    pub(crate) fn read_text_file_with_identity(
        &self,
        resource: &ResourceRef,
        expected_file: RootIdentity,
    ) -> AppResult<TextFile> {
        if resource.root_id != self.root_id {
            return Err(AppError::InvalidRequest(
                "resource root does not match workspace".into(),
            ));
        }
        validate_relative_path(&resource.relative_path, false)?;
        let mut file = open_workspace_text_file(
            &self.root,
            self.root_identity,
            &resource.relative_path,
            Some(expected_file),
        )?;
        let metadata = file.metadata()?;
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            return Err(AppError::ResourceLimit(format!(
                "file exceeds the {} MiB text-view limit",
                MAX_TEXT_FILE_BYTES / 1024 / 1024
            )));
        }
        let bytes = read_text_bytes_bounded(&mut file)?;
        let content = decode_text_bytes(bytes)?;
        let line_ending = detect_line_ending(&content).into();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        Ok(TextFile {
            resource: self.resource_unchecked(&resource.relative_path),
            byte_len: content.len() as u64,
            content,
            line_ending,
            modified_at,
        })
    }

    pub fn validate_resource(&self, resource: &ResourceRef, allow_root: bool) -> AppResult<String> {
        if resource.root_id != self.root_id {
            return Err(AppError::InvalidRequest(
                "resource root does not match the resolved project root".into(),
            ));
        }
        validate_relative_path(&resource.relative_path, allow_root)?;
        let resolved = self.resolve_existing(&resource.relative_path, allow_root)?;
        let canonical = canonical_contract_relative(&self.root, &resolved)?;
        if canonical != resource.relative_path {
            return Err(AppError::InvalidRequest(
                "resource path is not the canonical root-relative identity".into(),
            ));
        }
        Ok(canonical)
    }

    pub(crate) fn canonical_resource_identity(
        &self,
        resource: &ResourceRef,
        allow_root: bool,
    ) -> AppResult<ResourceRef> {
        if resource.root_id != self.root_id {
            return Err(AppError::InvalidRequest(
                "resource root does not match the resolved project root".into(),
            ));
        }
        validate_relative_path(&resource.relative_path, allow_root)?;
        let resolved = self.resolve_existing(&resource.relative_path, allow_root)?;
        let canonical = canonical_contract_relative(&self.root, &resolved)?;
        validate_relative_path(&canonical, allow_root)?;
        Ok(self.resource_unchecked(&canonical))
    }

    pub(crate) fn resolve_existing_file(&self, relative_path: &str) -> AppResult<PathBuf> {
        self.resolve_existing(relative_path, false)
    }

    fn resolve_existing(&self, relative_path: &str, allow_root: bool) -> AppResult<PathBuf> {
        validate_relative_path(relative_path, allow_root)?;

        let relative = Path::new(relative_path);
        let resolved = self.root.join(relative).canonicalize()?;
        if !resolved.starts_with(&self.root) {
            return Err(AppError::InvalidRequest(
                "resolved path escapes the workspace".into(),
            ));
        }
        Ok(resolved)
    }

    fn resource_unchecked(&self, relative_path: &str) -> ResourceRef {
        ResourceRef {
            root_id: self.root_id.clone(),
            relative_path: relative_path.into(),
        }
    }
}

fn validate_relative_path(relative_path: &str, allow_root: bool) -> AppResult<()> {
    if relative_path.is_empty() {
        return if allow_root {
            Ok(())
        } else {
            Err(AppError::InvalidRequest(
                "file path must not be empty".into(),
            ))
        };
    }
    if relative_path.contains('\\')
        || relative_path.starts_with('/')
        || relative_path.ends_with('/')
        || relative_path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AppError::InvalidRequest(
            "path must use one canonical root-relative spelling".into(),
        ));
    }
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(AppError::InvalidRequest(
            "path must stay within the workspace".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_contract_path(relative_path: &str, allow_root: bool) -> AppResult<()> {
    validate_relative_path(relative_path, allow_root)
}

pub fn development_workspace_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return std::env::current_dir().ok().and_then(|current| {
            current
                .ancestors()
                .find(|path| path.join(".git").exists())
                .map(Path::to_path_buf)
        });
    }
    None
}

pub(crate) fn entry_rank(kind: &str) -> u8 {
    match kind {
        "directory" => 0,
        "file" => 1,
        "symlink" => 2,
        _ => 3,
    }
}

fn join_contract_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}

fn canonical_contract_relative(root: &Path, resolved: &Path) -> AppResult<String> {
    let relative = resolved
        .strip_prefix(root)
        .map_err(|_| AppError::InvalidRequest("resolved path escapes the workspace".into()))?;
    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_owned).ok_or_else(|| {
                AppError::InvalidRequest("resource path is not valid Unicode".into())
            }),
            _ => Err(AppError::InvalidRequest(
                "resolved path is not root-relative".into(),
            )),
        })
        .collect::<AppResult<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "crlf"
    } else if content.contains('\n') {
        "lf"
    } else {
        "none"
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use uuid::Uuid;

    use super::*;

    fn fixture_workspace() -> (PathBuf, Workspace) {
        let root = std::env::temp_dir().join(format!("sage-workspace-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::File::create(root.join("src").join("main.rs"))
            .unwrap()
            .write_all(b"fn main() {}\\n")
            .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    #[test]
    fn lists_directories_without_recursing() {
        let (root, workspace) = fixture_workspace();
        let listing = workspace
            .list_directory(&ResourceRef {
                root_id: "root-test".into(),
                relative_path: String::new(),
            })
            .unwrap();

        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].resource.relative_path, "src");
        assert_eq!(listing.entries[0].kind, "directory");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marks_ignored_entries_and_everything_inside_an_ignored_folder() {
        let (root, workspace) = fixture_workspace();
        fs::write(root.join(".gitignore"), "*.log\ntarget/\n").unwrap();
        fs::write(root.join("debug.log"), "").unwrap();
        fs::create_dir_all(root.join("target").join("debug")).unwrap();
        fs::write(root.join("target").join("debug").join("app"), "").unwrap();
        let ignored = |relative: &str| {
            workspace
                .list_directory(&workspace.resource_unchecked(relative))
                .unwrap()
                .entries
                .into_iter()
                .map(|entry| (entry.name, entry.ignored))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ignored(""),
            [
                ("src".to_string(), false),
                ("target".to_string(), true),
                (".gitignore".to_string(), false),
                ("debug.log".to_string(), true),
            ]
        );
        assert_eq!(ignored("target/debug"), [("app".to_string(), true)]);
        let listing = workspace
            .list_directory(&workspace.resource_unchecked(""))
            .unwrap();
        let json = serde_json::to_value(&listing.entries).unwrap();
        assert_eq!(json[0].get("ignored"), None);
        assert_eq!(json[1]["ignored"], true);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_utf8_files_and_rejects_parent_traversal() {
        let (root, workspace) = fixture_workspace();
        let document = workspace
            .read_text_file(&workspace.resource("src/main.rs").unwrap())
            .unwrap();

        assert_eq!(document.content, "fn main() {}\\n");
        assert!(
            workspace
                .read_text_file(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: "../outside.txt".into(),
                })
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_dot_and_repeated_separator_aliases() {
        let (root, workspace) = fixture_workspace();
        for alias in ["src/./main.rs", "src//main.rs", "src\\main.rs"] {
            assert!(
                workspace
                    .read_text_file(&ResourceRef {
                        root_id: "root-test".into(),
                        relative_path: alias.into(),
                    })
                    .is_err(),
                "alias should be rejected: {alias}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_identity_preserves_the_high_half_of_a_128_bit_file_id() {
        let low = RootIdentity {
            volume: 7,
            file_id: [0; 16],
        };
        let mut high_id = [0_u8; 16];
        high_id[0] = 1;
        let high = RootIdentity {
            volume: 7,
            file_id: high_id,
        };

        assert_ne!(low, high);
        assert_ne!(low.stable_bytes(), high.stable_bytes());
    }

    #[test]
    fn bounded_reader_accepts_exact_cap_and_rejects_plus_one() {
        let root = std::env::temp_dir().join(format!("sage-workspace-cap-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let exact = root.join("exact.txt");
        fs::write(&exact, vec![b'x'; MAX_TEXT_FILE_BYTES as usize]).unwrap();
        assert_eq!(
            read_text_bytes_bounded(&mut fs::File::open(&exact).unwrap())
                .unwrap()
                .len(),
            MAX_TEXT_FILE_BYTES as usize
        );
        let plus_one = root.join("plus-one.txt");
        fs::write(
            &plus_one,
            vec![b'x'; text_read::TEXT_FILE_READ_PROBE_BYTES as usize],
        )
        .unwrap();
        assert!(matches!(
            read_text_bytes_bounded(&mut fs::File::open(&plus_one).unwrap()),
            Err(AppError::ResourceLimit(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn held_handle_rechecks_growth_after_metadata_preflight() {
        let root = std::env::temp_dir().join(format!("sage-workspace-growth-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("growth.txt");
        fs::write(&source, b"small").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let hook =
            text_read::test_hook::AfterMetadataGuard::install(std::sync::Arc::clone(&barrier));
        let writer_source = source.clone();
        let writer_barrier = std::sync::Arc::clone(&barrier);
        let writer = std::thread::spawn(move || {
            writer_barrier.wait();
            fs::write(&writer_source, vec![b'x'; MAX_TEXT_FILE_BYTES as usize + 1]).unwrap();
            writer_barrier.wait();
        });
        let result = hook.read(&mut fs::File::open(&source).unwrap());
        writer.join().unwrap();
        drop(hook);
        assert!(matches!(result, Err(AppError::ResourceLimit(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acp_line_selection_preserves_source_newlines_and_rejects_zero() {
        let source = "one\r\ntwo\r\nthree";
        assert_eq!(
            select_text_lines(source, None, Some(2)).unwrap(),
            "one\r\ntwo\r\n"
        );
        assert_eq!(select_text_lines(source, Some(3), None).unwrap(), "three");
        assert_eq!(select_text_lines(source, Some(4), None).unwrap(), "");
        assert!(select_text_lines(source, Some(0), None).is_err());
        assert!(select_text_lines(source, None, Some(0)).is_err());
    }

    #[test]
    fn directory_ingress_rejects_backslash_resource_paths_portably() {
        let (root, workspace) = fixture_workspace();
        let error = workspace
            .list_directory(&ResourceRef {
                root_id: "root-test".into(),
                relative_path: "back\\slash.txt".into(),
            })
            .unwrap_err();
        assert!(matches!(
            error,
            AppError::InvalidRequest(message)
                if message.contains("canonical root-relative spelling")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_listing_rejects_backslash_entry_before_resource_egress() {
        let (root, workspace) = fixture_workspace();
        let unsupported = root.join("back\\slash.txt");
        fs::write(&unsupported, "unsupported\n").unwrap();

        let error = workspace
            .list_directory(&ResourceRef {
                root_id: "root-test".into(),
                relative_path: String::new(),
            })
            .unwrap_err();
        assert!(matches!(
            error,
            AppError::InvalidRequest(message)
                if message.contains("canonical root-relative spelling")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn filesystem_rejected_invalid_name(error: &std::io::Error) -> bool {
        const LINUX_EILSEQ: i32 = 84;
        error.kind() == std::io::ErrorKind::InvalidFilename
            || error.raw_os_error() == Some(LINUX_EILSEQ)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_ingress_rejects_linux_non_utf8_names() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let (root, workspace) = fixture_workspace();
        let unsupported = root.join(OsString::from_vec(b"non-utf8-\xff".to_vec()));
        match fs::write(&unsupported, "unsupported\n") {
            Ok(()) => {}
            Err(error) if filesystem_rejected_invalid_name(&error) => {
                eprintln!(
                    "skipping raw invalid-byte entry fixture: filesystem rejected the test name: {error}"
                );
                fs::remove_dir_all(root).unwrap();
                return;
            }
            Err(error) => {
                let cleanup = fs::remove_dir_all(&root);
                panic!(
                    "raw invalid-byte entry setup failed unexpectedly: {error}; cleanup: {cleanup:?}"
                );
            }
        }
        let error = workspace
            .list_directory(&ResourceRef {
                root_id: "root-test".into(),
                relative_path: String::new(),
            })
            .unwrap_err();
        assert!(matches!(
            error,
            AppError::InvalidRequest(message) if message.contains("not valid UTF-8")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn final_fifo_is_rejected_without_blocking_the_source_open() {
        use std::{
            ffi::CString,
            os::raw::{c_char, c_int},
            time::Duration,
        };

        unsafe extern "C" {
            fn mkfifo(path: *const c_char, mode: u32) -> c_int;
        }
        let (root, workspace) = fixture_workspace();
        let fifo = root.join("in-root.fifo");
        let name = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a valid, NUL-terminated path in this test root.
        assert_eq!(unsafe { mkfifo(name.as_ptr(), 0o600) }, 0);
        let resource = workspace.resource("in-root.fifo").unwrap();
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let read_workspace = workspace.clone();
        let worker = std::thread::spawn(move || {
            let _ = result_tx.send(read_workspace.read_text_file(&resource));
        });
        let result = match result_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Actively release a regressed blocking FIFO open before
                // joining: a timeout assertion must never strand this test.
                let unblock = std::fs::OpenOptions::new().write(true).open(&fifo);
                assert!(unblock.is_ok(), "failed to unblock FIFO reader");
                let result = result_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("FIFO reader remained blocked after active cleanup");
                worker.join().expect("FIFO reader worker panicked");
                panic!("FIFO source open exceeded 250 ms: {result:?}");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                worker.join().expect("FIFO reader worker panicked");
                panic!("FIFO reader worker disconnected");
            }
        };
        worker.join().expect("FIFO reader worker panicked");
        assert!(result.is_err());
        fs::remove_file(fifo).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn held_handle_read_rejects_a_final_symlink_and_parent_swap() {
        use std::os::unix::fs::symlink;

        let (root, workspace) = fixture_workspace();
        let outside = root.with_extension("outside.txt");
        fs::write(&outside, "outside\n").unwrap();
        symlink(&outside, root.join("final-link.txt")).unwrap();
        assert!(
            workspace
                .read_text_file(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: "final-link.txt".into()
                })
                .is_err()
        );
        fs::rename(root.join("src"), root.join("src-old")).unwrap();
        symlink(root.parent().unwrap(), root.join("src")).unwrap();
        assert!(
            workspace
                .read_text_file(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: "src/main.rs".into()
                })
                .is_err()
        );
        fs::remove_file(root.join("src")).unwrap();
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_case_alias_for_an_existing_resource() {
        let (root, workspace) = fixture_workspace();
        assert!(
            workspace
                .read_text_file(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: "SRC/MAIN.RS".into(),
                })
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
