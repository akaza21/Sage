use std::path::{Path, PathBuf};

use super::{
    OVERFLOW_NOTIFY_ACTION, OVERFLOW_RELATIVE_PATH, WorkspaceNotifyError, reject,
    relative_from_notify_name,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "only inotify reports creates and renames")
)]
pub(crate) enum UnixWatchKind {
    Created,
    Deleted,
    Changed,
    RenamedOld,
    RenamedNew,
    Overflow,
}

pub(crate) fn unix_watch_delivery(
    name: Option<&str>,
    kind: UnixWatchKind,
) -> Result<(String, u32), WorkspaceNotifyError> {
    let Some(name) = name.filter(|_| kind != UnixWatchKind::Overflow) else {
        return Ok((OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION));
    };
    let relative = relative_from_notify_name(name)?;
    let action = match kind {
        UnixWatchKind::Created => 1,
        UnixWatchKind::Deleted => 2,
        UnixWatchKind::Changed => 3,
        UnixWatchKind::RenamedOld => 4,
        UnixWatchKind::RenamedNew => 5,
        UnixWatchKind::Overflow => OVERFLOW_NOTIFY_ACTION,
    };
    Ok((relative, action))
}

#[cfg(any(target_os = "linux", test))]
pub(super) const IN_MODIFY: u32 = 0x0000_0002;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_ATTRIB: u32 = 0x0000_0004;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_MOVED_FROM: u32 = 0x0000_0040;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_MOVED_TO: u32 = 0x0000_0080;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_CREATE: u32 = 0x0000_0100;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_DELETE: u32 = 0x0000_0200;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_Q_OVERFLOW: u32 = 0x0000_4000;

#[cfg(any(target_os = "linux", test))]
pub(super) fn unix_kind_from_inotify_mask(mask: u32) -> UnixWatchKind {
    if mask & IN_Q_OVERFLOW != 0 {
        UnixWatchKind::Overflow
    } else if mask & IN_CREATE != 0 {
        UnixWatchKind::Created
    } else if mask & IN_DELETE != 0 {
        UnixWatchKind::Deleted
    } else if mask & IN_MOVED_FROM != 0 {
        UnixWatchKind::RenamedOld
    } else if mask & IN_MOVED_TO != 0 {
        UnixWatchKind::RenamedNew
    } else if mask & (IN_MODIFY | IN_ATTRIB) != 0 {
        UnixWatchKind::Changed
    } else {
        UnixWatchKind::Overflow
    }
}

#[cfg(any(target_os = "linux", test))]
const INOTIFY_EVENT_BYTES: usize = 16;

#[cfg(any(target_os = "linux", test))]
pub(super) fn unix_watch_deliveries_from_inotify_bytes(buffer: &[u8]) -> Vec<(String, u32)> {
    let mut offset = 0_usize;
    let mut deliveries = Vec::new();
    while offset
        .checked_add(INOTIFY_EVENT_BYTES)
        .is_some_and(|end| end <= buffer.len())
    {
        let Some(mask) = buffer
            .get(offset + 4..offset + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
        else {
            break;
        };
        let Some(name_len) = buffer
            .get(offset + 12..offset + 16)
            .and_then(|bytes| bytes.try_into().ok())
            .and_then(|bytes| usize::try_from(u32::from_le_bytes(bytes)).ok())
        else {
            break;
        };
        let name_start = offset + INOTIFY_EVENT_BYTES;
        let Some(name_end) = name_start.checked_add(name_len) else {
            break;
        };
        if name_end > buffer.len() {
            break;
        }
        let name = if name_len == 0 {
            None
        } else {
            let raw = &buffer[name_start..name_end];
            let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
            std::str::from_utf8(&raw[..end])
                .ok()
                .filter(|name| !name.is_empty())
        };
        if let Ok(delivery) = unix_watch_delivery(name, unix_kind_from_inotify_mask(mask)) {
            deliveries.push(delivery);
        }
        offset = name_end;
    }
    deliveries
}

pub(super) const MAX_UNIX_WATCH_DIRECTORIES: usize = 64;

#[cfg(test)]
pub(super) fn unix_workspace_notify_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

pub(super) fn bounded_watch_directories(
    root: &Path,
) -> Result<(Vec<PathBuf>, bool), WorkspaceNotifyError> {
    let root = root
        .canonicalize()
        .map_err(|_| reject("notify-substituted"))?;
    if !root.is_dir() {
        return Err(reject("notify-substituted"));
    }
    let mut found = vec![root];
    let mut index = 0_usize;
    let mut truncated = false;
    while index < found.len() {
        let current = found[index].clone();
        index += 1;
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            if crate::workspace::directory_skip::skip_workspace_directory_name(&entry.file_name()) {
                continue;
            }
            if found.len() >= MAX_UNIX_WATCH_DIRECTORIES {
                truncated = true;
                break;
            }
            found.push(entry.path());
        }
        if truncated {
            break;
        }
    }
    Ok((found, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_unix_watch_event_is_overflow_resync() {
        assert_eq!(
            unix_watch_delivery(None, UnixWatchKind::Changed).expect("unnamed overflow"),
            (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION)
        );
        assert_eq!(
            unix_watch_delivery(None, UnixWatchKind::Created).expect("create without name"),
            (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION)
        );
    }

    #[test]
    fn named_unix_kinds_map_to_windows_actions() {
        assert_eq!(
            unix_watch_delivery(Some("src/main.ts"), UnixWatchKind::Created).expect("created"),
            ("src/main.ts".to_string(), 1)
        );
        assert_eq!(
            unix_watch_delivery(Some("src/main.ts"), UnixWatchKind::Deleted).expect("deleted"),
            ("src/main.ts".to_string(), 2)
        );
        assert_eq!(
            unix_watch_delivery(Some("src/main.ts"), UnixWatchKind::Changed).expect("changed"),
            ("src/main.ts".to_string(), 3)
        );
        assert_eq!(
            unix_watch_delivery(Some("old.ts"), UnixWatchKind::RenamedOld).expect("rename old"),
            ("old.ts".to_string(), 4)
        );
        assert_eq!(
            unix_watch_delivery(Some("new.ts"), UnixWatchKind::RenamedNew).expect("rename new"),
            ("new.ts".to_string(), 5)
        );
        assert_eq!(
            unix_watch_delivery(Some("src/main.ts"), UnixWatchKind::Overflow).expect("overflow"),
            (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION)
        );
    }

    #[test]
    fn unix_name_normalizes_separators_and_rejects_escape() {
        assert_eq!(
            unix_watch_delivery(Some("src\\main.ts"), UnixWatchKind::Created).expect("backslash"),
            ("src/main.ts".to_string(), 1)
        );
        assert_eq!(
            unix_watch_delivery(Some("../escape"), UnixWatchKind::Created)
                .expect_err("escape")
                .code(),
            "schema-invalid"
        );
    }

    #[test]
    fn inotify_masks_map_to_unix_kinds() {
        assert_eq!(
            unix_kind_from_inotify_mask(IN_Q_OVERFLOW | IN_CREATE),
            UnixWatchKind::Overflow
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_CREATE),
            UnixWatchKind::Created
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_DELETE),
            UnixWatchKind::Deleted
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_MOVED_FROM),
            UnixWatchKind::RenamedOld
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_MOVED_TO),
            UnixWatchKind::RenamedNew
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_MODIFY),
            UnixWatchKind::Changed
        );
        assert_eq!(
            unix_kind_from_inotify_mask(IN_ATTRIB),
            UnixWatchKind::Changed
        );
        assert_eq!(unix_kind_from_inotify_mask(0), UnixWatchKind::Overflow);
    }

    fn inotify_record(mask: u32, name: &str) -> Vec<u8> {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0);
        while !name_bytes.len().is_multiple_of(4) {
            name_bytes.push(0);
        }
        let mut bytes = vec![0_u8; INOTIFY_EVENT_BYTES];
        bytes[4..8].copy_from_slice(&mask.to_le_bytes());
        bytes[12..16].copy_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        bytes.extend(name_bytes);
        bytes
    }

    #[test]
    fn inotify_bytes_yield_named_create_and_overflow() {
        let mut buffer = inotify_record(IN_CREATE, "src/main.ts");
        buffer.extend(inotify_record(IN_Q_OVERFLOW, ""));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer),
            vec![
                ("src/main.ts".to_string(), 1),
                (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION),
            ]
        );
    }

    #[test]
    fn unix_workspace_notify_supported_is_macos_or_linux_only() {
        assert_eq!(
            unix_workspace_notify_supported(),
            cfg!(any(target_os = "macos", target_os = "linux"))
        );
    }

    #[test]
    fn bounded_watch_directories_cap_and_skip_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "sage-unix-watch-dirs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("root");
        for index in 0..MAX_UNIX_WATCH_DIRECTORIES {
            std::fs::create_dir(root.join(format!("dir-{index}"))).expect("child dir");
        }
        let link = root.join("link-dir");
        let linked = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(root.join("dir-0"), &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(root.join("dir-0"), &link).is_ok()
            }
            #[cfg(not(any(unix, windows)))]
            {
                false
            }
        };
        let (directories, truncated) = bounded_watch_directories(&root).expect("walk");
        assert!(truncated);
        assert_eq!(directories.len(), MAX_UNIX_WATCH_DIRECTORIES);
        if linked {
            assert!(directories.iter().all(|path| path != &link));
        }
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn bounded_watch_directories_skip_git_and_node_modules() {
        let root = std::env::temp_dir().join(format!(
            "sage-unix-watch-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("root");
        std::fs::create_dir(root.join("src")).expect("src");
        std::fs::create_dir(root.join("vendor")).expect("vendor");
        std::fs::create_dir(root.join(".git")).expect("git");
        std::fs::create_dir_all(root.join("node_modules").join("pkg")).expect("modules");

        let (directories, truncated) = bounded_watch_directories(&root).expect("walk");
        let names = directories
            .iter()
            .filter_map(|path| path.file_name()?.to_str())
            .collect::<Vec<_>>();

        assert!(!truncated);
        assert_eq!(directories.len(), 3);
        assert!(names.contains(&"src"));
        assert!(names.contains(&"vendor"));
        assert!(!names.contains(&".git"));
        assert!(!names.contains(&"node_modules"));
        assert!(!names.contains(&"pkg"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
