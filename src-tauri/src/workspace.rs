use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::{
    domain::{DirectoryListing, ResourceRef, TextFile, WorkspaceEntry},
    error::{AppError, AppResult},
};

// The integration owner registers this isolated command in the Tauri handler.
#[allow(dead_code)]
pub(crate) mod search;

const MAX_DIRECTORY_ENTRIES: usize = 500;
const MAX_TEXT_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Workspace {
    project_id: String,
    root_id: String,
    root: PathBuf,
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

        for entry in fs::read_dir(directory)? {
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
            });
        }

        entries.sort_unstable_by(|left, right| {
            entry_rank(&left.kind)
                .cmp(&entry_rank(&right.kind))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(DirectoryListing {
            resource: self.resource_unchecked(&relative_path),
            entries,
            truncated,
        })
    }

    pub fn read_text_file(&self, resource: &ResourceRef) -> AppResult<TextFile> {
        let relative_path = self.validate_resource(resource, false)?;
        let path = self.resolve_existing(&relative_path, false)?;
        let metadata = path.metadata()?;
        if !metadata.is_file() {
            return Err(AppError::InvalidRequest("resource is not a file".into()));
        }
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            return Err(AppError::ResourceLimit(format!(
                "file exceeds the {} MiB text-view limit",
                MAX_TEXT_FILE_BYTES / 1024 / 1024
            )));
        }

        let bytes = fs::read(path)?;
        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            return Err(AppError::InvalidRequest(
                "binary files cannot be opened in the text view".into(),
            ));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| AppError::InvalidRequest("file is not valid UTF-8".into()))?;
        let line_ending = detect_line_ending(&content).into();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);

        Ok(TextFile {
            resource: self.resource_unchecked(&relative_path),
            byte_len: metadata.len(),
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

    pub(crate) fn canonical_ingress_resource(
        &self,
        resource: &ResourceRef,
    ) -> AppResult<ResourceRef> {
        if resource.root_id != self.root_id {
            return Err(AppError::InvalidRequest(
                "resource root does not match the resolved project root".into(),
            ));
        }
        validate_relative_path(&resource.relative_path, false)?;
        if self.root.join(&resource.relative_path).try_exists()? {
            self.canonical_resource_identity(resource, false)
        } else {
            Ok(resource.clone())
        }
    }

    pub(crate) fn resolve_existing_file(&self, relative_path: &str) -> AppResult<PathBuf> {
        self.resolve_existing(relative_path, false)
    }

    pub(crate) fn resolve_for_write(&self, relative_path: &str) -> AppResult<PathBuf> {
        validate_relative_path(relative_path, false)?;
        let candidate = self.root.join(relative_path);
        let parent = candidate
            .parent()
            .ok_or_else(|| AppError::InvalidRequest("file has no parent directory".into()))?
            .canonicalize()?;
        if !parent.starts_with(&self.root) {
            return Err(AppError::InvalidRequest(
                "write path escapes the workspace".into(),
            ));
        }
        Ok(candidate)
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

fn entry_rank(kind: &str) -> u8 {
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

    #[cfg(unix)]
    #[test]
    fn directory_ingress_rejects_backslash_and_non_utf8_names() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let (root, workspace) = fixture_workspace();
        fs::write(root.join("back\\slash.txt"), "unsupported\n").unwrap();
        assert!(
            workspace
                .list_directory(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: String::new(),
                })
                .unwrap_err()
                .to_string()
                .contains("canonical root-relative spelling")
        );
        fs::remove_file(root.join("back\\slash.txt")).unwrap();
        for bytes in [b"non-utf8-\xff".to_vec(), b"non-utf8-\xfe".to_vec()] {
            fs::write(root.join(OsString::from_vec(bytes)), "unsupported\n").unwrap();
        }
        assert!(
            workspace
                .list_directory(&ResourceRef {
                    root_id: "root-test".into(),
                    relative_path: String::new(),
                })
                .unwrap_err()
                .to_string()
                .contains("not valid UTF-8")
        );
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
