//! Fail-closed host presence for the Codex catalog.
//!
//! A present file is not launch, discovery, or admission authority. Full
//! harness discovery lives in `crate::harness_discovery`; this compatibility
//! projection intentionally retains the existing PATH-only result and callers.

use std::path::{Path, PathBuf};

use crate::harness_discovery::{
    PATH_BYTES_MAX, authentic_harness_file, path_candidates_for_basename,
};

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
    if cfg!(windows) { "codex.exe" } else { "codex" }
}

pub(crate) fn observe_codex_host_presence() -> CodexHostPresence {
    // The shared discovery probe additionally considers known roots and executes
    // `--version`. This legacy projection intentionally remains PATH-only and
    // side-effect-free for its existing install-state callers.
    observe_codex_host_presence_among(&path_candidates_for_basename(expected_codex_basename()))
}

pub(crate) fn observe_codex_host_presence_among(candidates: &[PathBuf]) -> CodexHostPresence {
    if candidates
        .iter()
        .any(|candidate| authentic_codex_file(candidate))
    {
        CodexHostPresence::Present
    } else {
        CodexHostPresence::Absent
    }
}

fn authentic_codex_file(path: &Path) -> bool {
    authentic_harness_file(path, expected_codex_basename())
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
    fn seventeenth_candidate_is_observed() {
        let dir = scratch_dir();
        let mut candidates: Vec<PathBuf> = (0..16)
            .map(|index| {
                dir.join(format!("missing-{index}"))
                    .join(expected_codex_basename())
            })
            .collect();
        candidates.push(write_regular(&dir, expected_codex_basename()));
        assert_eq!(
            observe_codex_host_presence_among(&candidates),
            CodexHostPresence::Present
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
