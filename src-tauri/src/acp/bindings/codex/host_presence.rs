//! Fail-closed host presence for the Codex catalog.
//!
//! A present file is not launch, discovery, or admission authority.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::path::{Component, Path, PathBuf};

const PATH_DIR_CAP: usize = 16;
const PATH_BYTES_MAX: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexHostPresence {
    Absent,
    Present,
}

impl CodexHostPresence {
    pub(crate) fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

pub(crate) fn expected_codex_basename() -> &'static str {
    if cfg!(windows) {
        "codex.exe"
    } else {
        "codex"
    }
}

pub(crate) fn observe_codex_host_presence() -> CodexHostPresence {
    observe_codex_host_presence_among(&production_path_candidates())
}

pub(crate) fn observe_codex_host_presence_among(candidates: &[PathBuf]) -> CodexHostPresence {
    if candidates
        .iter()
        .take(PATH_DIR_CAP)
        .any(|candidate| authentic_codex_file(candidate))
    {
        CodexHostPresence::Present
    } else {
        CodexHostPresence::Absent
    }
}

fn production_path_candidates() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .take(PATH_DIR_CAP)
        .filter(|dir| is_local_probe_root(dir))
        .map(|dir| dir.join(expected_codex_basename()))
        .collect()
}

fn authentic_codex_file(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path.as_os_str().len() > PATH_BYTES_MAX {
        return false;
    }
    if path.file_name() != Some(OsStr::new(expected_codex_basename())) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if !usable_probe_directory(parent) {
        return false;
    }
    match path.symlink_metadata() {
        Ok(meta) if meta.is_file() && !is_link(&meta) => true,
        _ => false,
    }
}

fn usable_probe_directory(dir: &Path) -> bool {
    if !is_local_probe_root(dir) {
        return false;
    }
    match dir.symlink_metadata() {
        Ok(meta) if meta.is_dir() && !is_link(&meta) => true,
        _ => false,
    }
}

fn is_local_probe_root(dir: &Path) -> bool {
    if dir.as_os_str().is_empty() || dir.as_os_str().len() > PATH_BYTES_MAX {
        return false;
    }
    if dir
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        matches!(
            dir.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        ) && dir.is_absolute()
    }
    #[cfg(not(windows))]
    {
        dir.is_absolute()
    }
}

fn is_link(meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sage-codex-presence-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_regular(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"not-a-launcher").unwrap();
        path
    }

    #[test]
    fn missing_probe_file_is_absent() {
        let dir = scratch_dir();
        let missing = dir.join(expected_codex_basename());
        assert_eq!(
            observe_codex_host_presence_among(&[missing]),
            CodexHostPresence::Absent
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn regular_expected_basename_is_present() {
        let dir = scratch_dir();
        let file = write_regular(&dir, expected_codex_basename());
        assert_eq!(
            observe_codex_host_presence_among(&[file]),
            CodexHostPresence::Present
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wrong_basename_is_absent() {
        let dir = scratch_dir();
        let file = write_regular(&dir, "codex.txt");
        assert_eq!(
            observe_codex_host_presence_among(&[file]),
            CodexHostPresence::Absent
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn directory_with_expected_name_is_absent() {
        let dir = scratch_dir();
        let nested = dir.join(expected_codex_basename());
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            observe_codex_host_presence_among(&[nested]),
            CodexHostPresence::Absent
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relative_candidate_is_absent() {
        assert_eq!(
            observe_codex_host_presence_among(&[PathBuf::from(expected_codex_basename())]),
            CodexHostPresence::Absent
        );
    }

    #[test]
    fn seventeenth_candidate_is_ignored() {
        let dir = scratch_dir();
        let mut candidates: Vec<PathBuf> = (0..PATH_DIR_CAP)
            .map(|index| {
                dir.join(format!("missing-{index}"))
                    .join(expected_codex_basename())
            })
            .collect();
        candidates.push(write_regular(&dir, expected_codex_basename()));
        assert_eq!(
            observe_codex_host_presence_among(&candidates),
            CodexHostPresence::Absent
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn unc_candidate_is_absent() {
        assert_eq!(
            observe_codex_host_presence_among(&[PathBuf::from(r"\\server\share\codex.exe")]),
            CodexHostPresence::Absent
        );
    }

    #[test]
    fn oversized_path_is_absent() {
        let oversized = PathBuf::from(format!(
            r"C:\{}\{}",
            "a".repeat(PATH_BYTES_MAX),
            expected_codex_basename()
        ));
        assert_eq!(
            observe_codex_host_presence_among(&[oversized]),
            CodexHostPresence::Absent
        );
    }

    #[test]
    fn symlink_candidate_is_absent_when_links_are_available() {
        let dir = scratch_dir();
        let target = write_regular(&dir, "target.bin");
        let link = dir.join(expected_codex_basename());
        let linked = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&target, &link).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target, &link).is_ok()
            }
            #[cfg(not(any(windows, unix)))]
            {
                let _ = (target, &link);
                false
            }
        };
        if linked {
            assert_eq!(
                observe_codex_host_presence_among(&[link]),
                CodexHostPresence::Absent
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parent_symlink_directory_is_absent_when_links_are_available() {
        let dir = scratch_dir();
        let real = dir.join("real");
        fs::create_dir_all(&real).unwrap();
        write_regular(&real, expected_codex_basename());
        let link_dir = dir.join("link");
        let linked = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&real, &link_dir).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&real, &link_dir).is_ok()
            }
            #[cfg(not(any(windows, unix)))]
            {
                let _ = (real, &link_dir);
                false
            }
        };
        if linked {
            assert_eq!(
                observe_codex_host_presence_among(&[link_dir.join(expected_codex_basename())]),
                CodexHostPresence::Absent
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
