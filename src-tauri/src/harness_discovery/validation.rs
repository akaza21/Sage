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
            HarnessExecutable::Codex => &["codex.exe", "codex.cmd"],
            HarnessExecutable::Claude => &["claude.exe", "claude.cmd"],
            HarnessExecutable::Pi => &["pi.exe", "pi.cmd"],
            HarnessExecutable::Gemini => &["gemini.exe", "gemini.cmd"],
            HarnessExecutable::OpenCode => &["opencode.exe", "opencode.cmd"],
            HarnessExecutable::CursorAgent => &["cursor-agent.exe", "cursor-agent.cmd"],
            HarnessExecutable::OhMyPi => &["omp.exe", "omp.cmd"],
        }
    }
    #[cfg(not(windows))]
    {
        match harness {
            HarnessExecutable::Codex => &["codex"],
            HarnessExecutable::Claude => &["claude"],
            HarnessExecutable::Pi => &["pi"],
            HarnessExecutable::Gemini => &["gemini"],
            HarnessExecutable::OpenCode => &["opencode"],
            HarnessExecutable::CursorAgent => &["cursor-agent"],
            HarnessExecutable::OhMyPi => &["omp"],
        }
    }
}

pub(super) fn authentic_harness_file(path: &Path, expected_basename: &str) -> bool {
    path.file_name() == Some(OsStr::new(expected_basename)) && authentic_file_with_parents(path)
}

pub(super) fn authentic_regular_file(path: &Path) -> bool {
    authentic_file_with_parents(path)
}

/// Validates a package file relative to a bounded, plain npm-global root. Every
/// path segment is inspected so a package, `node_modules`, or adapter entry can
/// never cross a symlink/reparse-point boundary after root discovery.
pub(super) fn authentic_descendant_file(root: &Path, relative: impl AsRef<Path>) -> bool {
    let relative = relative.as_ref();
    if !is_local_probe_root(root)
        || relative.as_os_str().is_empty()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return false;
    }
    let mut current = root.to_path_buf();
    if !usable_probe_directory(&current) {
        return false;
    }
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(name) = part else {
            return false;
        };
        current.push(name);
        let metadata = current.symlink_metadata();
        if parts.peek().is_some() {
            if !matches!(metadata, Ok(meta) if meta.is_dir() && !is_link(&meta)) {
                return false;
            }
        } else if !matches!(metadata, Ok(meta) if meta.is_file() && !is_link(&meta)) {
            return false;
        }
    }
    path_native_bytes(current.as_os_str()) <= PATH_BYTES_MAX
}

fn authentic_file_with_parents(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path_native_bytes(path.as_os_str()) > PATH_BYTES_MAX {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    usable_probe_directory(parent)
        && matches!(path.symlink_metadata(), Ok(meta) if meta.is_file() && !is_link(&meta))
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
        use std::{
            os::windows::ffi::OsStrExt,
            path::{PathBuf, Prefix},
        };
        let Some(Component::Prefix(prefix)) = dir.components().next() else {
            return false;
        };
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) || !dir.is_absolute()
        {
            return false;
        }
        let mut volume = PathBuf::from(prefix.as_os_str());
        volume.push(r"\");
        let mut wide: Vec<u16> = volume.as_os_str().encode_wide().collect();
        wide.push(0);
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        const DRIVE_RAMDISK: u32 = 6;
        matches!(
            unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) },
            DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK
        )
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
