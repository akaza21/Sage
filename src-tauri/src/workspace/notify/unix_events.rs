#[cfg(any(target_os = "linux", test))]
use std::collections::HashMap;
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
pub(super) const IN_DELETE_SELF: u32 = 0x0000_0400;
#[cfg(any(target_os = "linux", test))]
pub(super) const IN_MOVE_SELF: u32 = 0x0000_0800;
#[cfg(any(target_os = "linux", test))]
const IN_UNMOUNT: u32 = 0x0000_2000;
#[cfg(any(target_os = "linux", test))]
const IN_IGNORED: u32 = 0x0000_8000;
#[cfg(any(target_os = "linux", test))]
const IN_ISDIR: u32 = 0x4000_0000;

/// Each inotify watch descriptor's folder, relative to the first watched
/// folder with `/` separators (empty for that folder). A `None` folder has a
/// path that is not UTF-8, or its descriptor is shared by two paths: its
/// events are delivered as an overflow resync rather than under a wrong name.
///
/// With `subfolders`, the watch covers a folder tree whose shape can change:
/// a folder created, deleted or moved, a watched folder removed, or a lost
/// queue makes the batch ask for the tree to be watched again, and every
/// later record in that batch is folded into one resync.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
pub(super) struct InotifyFolders {
    by_watch: HashMap<i32, Option<String>>,
    subfolders: bool,
}

#[cfg(any(target_os = "linux", test))]
impl InotifyFolders {
    pub(super) fn new(subfolders: bool) -> Self {
        Self {
            by_watch: HashMap::new(),
            subfolders,
        }
    }

    pub(super) fn record(&mut self, watch: i32, relative: Option<String>) {
        self.by_watch
            .entry(watch)
            .and_modify(|known| {
                if *known != relative {
                    *known = None;
                }
            })
            .or_insert(relative);
    }

    #[cfg(target_os = "linux")]
    pub(super) fn subfolders(&self) -> bool {
        self.subfolders
    }

    /// The watch descriptors this map holds that `next` does not.
    #[cfg(target_os = "linux")]
    pub(super) fn dropped_from(&self, next: &Self) -> Vec<i32> {
        self.by_watch
            .keys()
            .filter(|watch| !next.by_watch.contains_key(watch))
            .copied()
            .collect()
    }

    fn changes_tree(&self, watch: i32, mask: u32) -> bool {
        self.subfolders
            && (mask & IN_Q_OVERFLOW != 0
                || (mask & IN_ISDIR != 0
                    && mask & (IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO) != 0)
                || (mask & IN_WATCH_GONE != 0 && self.by_watch.contains_key(&watch)))
    }

    /// The tail of a watch this notifier already removed. Its preceding
    /// rewatch/resync covered these records, including directory events.
    fn stale(&self, watch: i32, mask: u32) -> bool {
        mask & IN_Q_OVERFLOW == 0
            && !self.by_watch.contains_key(&watch)
            && (self.subfolders || mask & IN_WATCH_GONE != 0)
    }

    /// Every watched subfolder has a watched parent. Use the parent's named
    /// event for its attributes, deletion or movement, rather than publishing
    /// multiple resyncs for one operation (possibly across reads). A client
    /// could otherwise recreate a folder after the first resync, mistake a
    /// queued deletion resync for its creation, and write before it is watched.
    /// Keep self events for the root, ambiguous mappings, and unmounts.
    fn reported_by_parent(&self, watch: i32, mask: u32, unnamed: bool) -> bool {
        self.subfolders
            && unnamed
            && mask & (IN_UNMOUNT | IN_Q_OVERFLOW) == 0
            && mask & (IN_ATTRIB | IN_DELETE_SELF | IN_MOVE_SELF | IN_IGNORED) != 0
            && matches!(self.by_watch.get(&watch), Some(Some(path)) if !path.is_empty())
    }
}

/// Records that say a watched folder itself is gone or moved.
#[cfg(any(target_os = "linux", test))]
const IN_WATCH_GONE: u32 = IN_DELETE_SELF | IN_MOVE_SELF | IN_UNMOUNT | IN_IGNORED;

/// The path of the folder `name` inside the folder at `parent`, relative to
/// the root with `/` separators. `None` when either is not UTF-8.
#[cfg(any(target_os = "linux", test))]
pub(super) fn inotify_child_relative(parent: Option<&str>, name: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(name).ok()?;
    Some(match parent? {
        "" => name.to_string(),
        parent => format!("{parent}/{name}"),
    })
}

/// What one read of inotify records delivers. When `rewatch` is set, the
/// caller must watch the folder tree again before it delivers, since the
/// trailing resync stands for every change from the tree change on.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct InotifyBatch {
    pub(super) deliveries: Vec<(String, u32)>,
    pub(super) rewatch: bool,
}

#[cfg(any(target_os = "linux", test))]
fn inotify_u32(buffer: &[u8], start: usize) -> Option<u32> {
    buffer
        .get(start..start.checked_add(4)?)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn unix_watch_deliveries_from_inotify_bytes(
    buffer: &[u8],
    folders: &InotifyFolders,
) -> InotifyBatch {
    let mut offset = 0_usize;
    let mut deliveries = Vec::new();
    let mut rewatch = false;
    while offset
        .checked_add(INOTIFY_EVENT_BYTES)
        .is_some_and(|end| end <= buffer.len())
    {
        let (Some(wd), Some(mask), Some(name_len)) = (
            inotify_u32(buffer, offset).map(|wd| wd as i32),
            inotify_u32(buffer, offset + 4),
            inotify_u32(buffer, offset + 12).and_then(|len| usize::try_from(len).ok()),
        ) else {
            break;
        };
        let name_start = offset + INOTIFY_EVENT_BYTES;
        let Some(name_end) = name_start.checked_add(name_len) else {
            break;
        };
        if name_end > buffer.len() {
            break;
        }
        offset = name_end;
        let unnamed = name_len == 0 || buffer[name_start] == 0;
        if folders.stale(wd, mask) || folders.reported_by_parent(wd, mask, unnamed) {
            continue;
        }
        if rewatch || folders.changes_tree(wd, mask) {
            rewatch = true;
            continue;
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
        let path = match folders.by_watch.get(&wd) {
            Some(Some(folder)) if folder.is_empty() => name.map(str::to_string),
            Some(Some(folder)) => name.map(|name| format!("{folder}/{name}")),
            _ => None,
        };
        let kind = unix_kind_from_inotify_mask(mask);
        if let Ok(delivery) = unix_watch_delivery(path.as_deref(), kind) {
            deliveries.push(delivery);
        }
    }
    if rewatch {
        // A tree resync also covers any earlier ambiguous-path notice in
        // this read. Publish one acknowledgement of the rebuilt tree.
        deliveries.retain(|(_, action)| *action != OVERFLOW_NOTIFY_ACTION);
        deliveries.push((OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION));
    }
    InotifyBatch {
        deliveries,
        rewatch,
    }
}

pub(super) const MAX_UNIX_WATCH_DIRECTORIES: usize = 64;

#[cfg(test)]
pub(super) fn unix_workspace_notify_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

/// The most folder entries one walk of a watched tree examines, the same
/// bound as a workspace file walk. Past it the walk stops as it does at the
/// folder cap.
pub(super) const MAX_UNIX_WATCH_ENTRIES: u64 = crate::workspace::file_walk::MAX_WORKSPACE_FILES;

pub(super) fn bounded_watch_directories(
    root: &Path,
) -> Result<(Vec<PathBuf>, bool), WorkspaceNotifyError> {
    bounded_watch_directories_within(root, MAX_UNIX_WATCH_ENTRIES)
}

fn bounded_watch_directories_within(
    root: &Path,
    max_entries: u64,
) -> Result<(Vec<PathBuf>, bool), WorkspaceNotifyError> {
    let root = root
        .canonicalize()
        .map_err(|_| reject("notify-substituted"))?;
    if !root.is_dir() {
        return Err(reject("notify-substituted"));
    }
    let mut found = vec![root];
    let mut index = 0_usize;
    let mut examined = 0_u64;
    let mut truncated = false;
    while index < found.len() {
        let current = found[index].clone();
        index += 1;
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            examined += 1;
            if crate::workspace::file_walk::walk_over_cap(examined, max_entries) {
                truncated = true;
                break;
            }
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

    fn inotify_record(wd: i32, mask: u32, name: &str) -> Vec<u8> {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0);
        while !name_bytes.len().is_multiple_of(4) {
            name_bytes.push(0);
        }
        let mut bytes = vec![0_u8; INOTIFY_EVENT_BYTES];
        bytes[0..4].copy_from_slice(&wd.to_le_bytes());
        bytes[4..8].copy_from_slice(&mask.to_le_bytes());
        bytes[12..16].copy_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        bytes.extend(name_bytes);
        bytes
    }

    fn root_and_src_folders() -> InotifyFolders {
        let mut folders = InotifyFolders::new(true);
        folders.record(1, Some(String::new()));
        folders.record(2, Some("src".to_string()));
        folders
    }

    fn overflow() -> (String, u32) {
        (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION)
    }

    fn deliveries(buffer: &[u8], folders: &InotifyFolders) -> Vec<(String, u32)> {
        unix_watch_deliveries_from_inotify_bytes(buffer, folders).deliveries
    }

    #[test]
    fn inotify_bytes_yield_named_create_and_overflow() {
        let mut buffer = inotify_record(1, IN_CREATE, "main.ts");
        buffer.extend(inotify_record(-1, IN_Q_OVERFLOW, ""));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer, &root_and_src_folders()),
            InotifyBatch {
                deliveries: vec![("main.ts".to_string(), 1), overflow()],
                rewatch: true,
            }
        );
    }

    #[test]
    fn inotify_bytes_join_event_name_to_its_watch_folder() {
        let mut buffer = inotify_record(1, IN_MODIFY, "main.ts");
        buffer.extend(inotify_record(2, IN_MODIFY, "main.ts"));
        buffer.extend(inotify_record(2, IN_DELETE, "old.ts"));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer, &root_and_src_folders()),
            InotifyBatch {
                deliveries: vec![
                    ("main.ts".to_string(), 3),
                    ("src/main.ts".to_string(), 3),
                    ("src/old.ts".to_string(), 2),
                ],
                rewatch: false,
            }
        );
    }

    #[test]
    fn inotify_bytes_fold_ambiguous_paths_into_the_queue_loss_resync() {
        let mut folders = root_and_src_folders();
        folders.record(3, None);
        let mut buffer = inotify_record(7, IN_CREATE, "main.ts");
        buffer.extend(inotify_record(3, IN_CREATE, "main.ts"));
        buffer.extend(inotify_record(2, IN_Q_OVERFLOW, "main.ts"));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer, &folders),
            InotifyBatch {
                deliveries: vec![overflow()],
                rewatch: true,
            }
        );
    }

    #[test]
    fn inotify_watch_shared_by_two_folders_resyncs() {
        let mut folders = root_and_src_folders();
        folders.record(2, Some("lib".to_string()));
        let buffer = inotify_record(2, IN_CREATE, "main.ts");
        assert_eq!(deliveries(&buffer, &folders), vec![overflow()]);
    }

    #[test]
    fn inotify_folder_tree_changes_ask_for_rewatch_and_fold_into_one_resync() {
        let mut folders = root_and_src_folders();
        folders.record(3, None);
        for (watch, mask, name) in [
            (1, IN_ISDIR | IN_MOVED_FROM, "src"),
            (1, IN_ISDIR | IN_MOVED_TO, "lib"),
            (1, IN_ISDIR | IN_CREATE, "new"),
            (1, IN_ISDIR | IN_DELETE, "src"),
            (1, IN_MOVE_SELF, ""),
            (1, IN_DELETE_SELF, ""),
            (1, IN_IGNORED, ""),
            (2, IN_UNMOUNT, ""),
            (3, IN_MOVE_SELF, ""),
            (3, IN_DELETE_SELF, ""),
            (3, IN_IGNORED, ""),
        ] {
            let mut buffer = inotify_record(2, IN_MODIFY, "before.ts");
            buffer.extend(inotify_record(3, IN_MODIFY, "ambiguous.ts"));
            buffer.extend(inotify_record(watch, mask, name));
            buffer.extend(inotify_record(2, IN_MODIFY, "after.ts"));
            assert_eq!(
                unix_watch_deliveries_from_inotify_bytes(&buffer, &folders),
                InotifyBatch {
                    deliveries: vec![("src/before.ts".to_string(), 3), overflow()],
                    rewatch: true,
                },
                "mask {mask:#x} on watch {watch}"
            );
        }
    }

    #[test]
    fn inotify_subfolder_self_events_wait_for_the_parent_across_reads() {
        let folders = root_and_src_folders();
        for mask in [IN_ATTRIB, IN_DELETE_SELF, IN_IGNORED, IN_MOVE_SELF] {
            assert_eq!(
                unix_watch_deliveries_from_inotify_bytes(&inotify_record(2, mask, ""), &folders),
                InotifyBatch {
                    deliveries: Vec::new(),
                    rewatch: false
                },
                "mask {mask:#x}"
            );
        }
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(
                &inotify_record(1, IN_ISDIR | IN_DELETE, "src"),
                &folders
            ),
            InotifyBatch {
                deliveries: vec![overflow()],
                rewatch: true
            }
        );
        assert_eq!(
            deliveries(&inotify_record(1, IN_ATTRIB, "src"), &folders),
            vec![("src".to_string(), 3)]
        );
    }

    #[test]
    fn inotify_tail_of_a_removed_watch_is_dropped() {
        let mut buffer = inotify_record(9, IN_MOVE_SELF, "");
        buffer.extend(inotify_record(9, IN_DELETE_SELF, ""));
        buffer.extend(inotify_record(9, IN_UNMOUNT, ""));
        buffer.extend(inotify_record(9, IN_IGNORED, ""));
        buffer.extend(inotify_record(9, IN_ATTRIB, ""));
        buffer.extend(inotify_record(9, IN_ISDIR | IN_DELETE, "old"));
        buffer.extend(inotify_record(9, IN_CREATE, "outside.ts"));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer, &root_and_src_folders()),
            InotifyBatch {
                deliveries: Vec::new(),
                rewatch: false,
            }
        );
    }

    #[test]
    fn single_folder_watch_keeps_bare_names_for_folder_events() {
        let mut folders = InotifyFolders::new(false);
        folders.record(1, Some(String::new()));
        let mut buffer = inotify_record(1, IN_ISDIR | IN_CREATE, "new");
        buffer.extend(inotify_record(1, IN_MODIFY, "main.ts"));
        buffer.extend(inotify_record(1, IN_IGNORED, ""));
        assert_eq!(
            unix_watch_deliveries_from_inotify_bytes(&buffer, &folders),
            InotifyBatch {
                deliveries: vec![
                    ("new".to_string(), 1),
                    ("main.ts".to_string(), 3),
                    overflow(),
                ],
                rewatch: false,
            }
        );
    }

    #[test]
    fn inotify_child_relative_uses_slashes_and_rejects_non_utf8() {
        assert_eq!(
            inotify_child_relative(Some(""), b"src"),
            Some("src".to_string())
        );
        assert_eq!(
            inotify_child_relative(Some("src"), b"lib"),
            Some("src/lib".to_string())
        );
        assert_eq!(inotify_child_relative(Some("src"), b"bad\xff"), None);
        assert_eq!(inotify_child_relative(None, b"lib"), None);
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
    fn bounded_watch_directories_stop_after_the_entry_limit() {
        let root = std::env::temp_dir().join(format!(
            "sage-unix-watch-entries-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("a").join("b").join("c")).expect("nested folders");
        let walk = |max_entries| bounded_watch_directories_within(&root, max_entries);
        let (within, within_truncated) = walk(3).expect("walk within the limit");
        let (past, past_truncated) = walk(2).expect("walk past the limit");
        std::fs::remove_dir_all(&root).expect("cleanup");
        assert!(!within_truncated);
        assert_eq!(within.len(), 4);
        assert!(past_truncated);
        assert_eq!(past.len(), 3);
        assert!(
            past.iter()
                .all(|path| path.file_name() != Some("c".as_ref()))
        );
        assert_eq!(MAX_UNIX_WATCH_ENTRIES, 200_000);
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
