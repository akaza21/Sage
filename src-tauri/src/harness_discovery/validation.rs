use std::{
    ffi::OsStr,
    fs::Metadata,
    path::{Component, Path},
};

use super::{HarnessExecutable, PATH_BYTES_MAX};

pub(super) fn expected_basenames(harness: HarnessExecutable) -> &'static [&'static str] {
    #[cfg(windows)]
    {
        match harness {
            // npm global installations provide `.cmd`; native installers
            // commonly provide `.exe`. Both are exact, expected names.
            HarnessExecutable::Codex => &["codex.exe", "codex.cmd"],
            HarnessExecutable::Claude => &["claude.exe", "claude.cmd"],
        }
    }
    #[cfg(not(windows))]
    {
        match harness {
            HarnessExecutable::Codex => &["codex"],
            HarnessExecutable::Claude => &["claude"],
        }
    }
}

pub(super) fn authentic_harness_file(path: &Path, expected_basename: &str) -> bool {
    if path.as_os_str().is_empty()
        || path_native_bytes(path.as_os_str()) > PATH_BYTES_MAX
        || path.file_name() != Some(OsStr::new(expected_basename))
    {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if !usable_probe_directory(parent) {
        return false;
    }
    matches!(path.symlink_metadata(), Ok(meta) if meta.is_file() && !is_link(&meta))
}

pub(super) fn is_local_probe_root(dir: &Path) -> bool {
    if dir.as_os_str().is_empty() || path_native_bytes(dir.as_os_str()) > PATH_BYTES_MAX {
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

fn usable_probe_directory(dir: &Path) -> bool {
    if !is_local_probe_root(dir) {
        return false;
    }
    matches!(dir.symlink_metadata(), Ok(meta) if meta.is_dir() && !is_link(&meta))
}

fn path_native_bytes(path: &OsStr) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.encode_wide().count().saturating_mul(2)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_bytes().len()
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
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

pub(super) fn legacy_basenames(command: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![format!("{command}.exe"), format!("{command}.cmd")]
    }
    #[cfg(not(windows))]
    {
        vec![command.to_owned()]
    }
}
