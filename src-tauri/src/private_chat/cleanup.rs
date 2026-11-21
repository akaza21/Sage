use super::*;
use std::path::Path;

const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 64;

pub(super) fn require_empty_folder(folder: &Path) -> AppResult<()> {
    ordinary_directory(folder)?;
    if fs::read_dir(folder)?.next().transpose()?.is_some() {
        return Err(AppError::InvalidRequest(
            "This chat's folder has files. Start a new chat in the project instead.".into(),
        ));
    }
    Ok(())
}

pub(super) fn empty_projects(database: &Database) -> AppResult<()> {
    let storage = lock_storage();
    empty_projects_with_storage(database, &storage, |_| {})
}

pub(super) fn empty_projects_with_storage(
    database: &Database,
    _storage: &MutexGuard<'_, ()>,
    mut before_remove: impl FnMut(&Path),
) -> AppResult<()> {
    let ids = {
        let connection = database.connection();
        let mut statement = connection.prepare("SELECT id FROM projects p WHERE kind='privateChat' AND NOT EXISTS(SELECT 1 FROM chats WHERE project_id=p.id) ORDER BY updated_at,id LIMIT 64").db()?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?
    };
    if ids.is_empty() {
        return Ok(());
    }
    let base = storage_root(database)?;
    fs::create_dir_all(&base)?;
    admit_base(&base)?;
    let base = base.canonicalize()?;
    let profile = base
        .parent()
        .ok_or_else(|| AppError::InvalidRequest("missing profile".into()))?;
    let _base_guard = sage_workspace::open_held_parent(
        profile,
        sage_workspace::root_identity_path(profile)?,
        "chat-workspaces",
    )?;
    let mut seen = 0;
    let mut failure = None;
    for id in ids {
        let folder = base.join(&id);
        // The receipt's revision claims this attempt durably. Rotate even a
        // refused attempt, so failures cannot starve the next bounded batch.
        let claim = {
            let mut connection = database.write_connection()?;
            let transaction = connection.transaction().db()?;
            let empty: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM projects p WHERE id=?1 AND kind='privateChat' AND NOT EXISTS(SELECT 1 FROM chats WHERE project_id=p.id))", [&id], |row| row.get(0)).db()?;
            if !empty {
                continue;
            }
            transaction.execute("UPDATE projects SET updated_at=(SELECT MAX(updated_at)+1 FROM projects) WHERE id=?1", [&id]).db()?;
            let revision: i64 = transaction
                .query_row(
                    "SELECT updated_at FROM projects WHERE id=?1",
                    [&id],
                    |row| row.get(0),
                )
                .db()?;
            let admitted = require_unreferenced(&transaction, &id, &folder);
            transaction.commit().db()?;
            admitted.map(|()| revision)
        };
        let result = (|| {
            let revision = claim?;
            if !Uuid::parse_str(&id).is_ok_and(|uuid| uuid.to_string() == id) {
                return Err(AppError::InvalidRequest(
                    "invalid private Chat folder identity".into(),
                ));
            }
            // Only the namespace guard is held here. Other durable work can
            // use SQLite while bounded recursive deletion runs.
            before_remove(&folder);
            // Derived path only, with no traversal or stored absolute path used.
            if fs::symlink_metadata(&folder)
                .map(|_| true)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(error)
                    }
                })?
            {
                #[cfg(windows)]
                remove_tree(&folder, &mut seen, 0)?;
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd;
                    let name = std::ffi::CString::new(id.as_bytes()).map_err(|_| {
                        AppError::InvalidRequest("invalid private Chat identity".into())
                    })?;
                    remove_at(_base_guard.as_raw_fd(), &name, &mut seen, 0)?;
                }
            }
            let mut connection = database.write_connection()?;
            let transaction = connection.transaction().db()?;
            let owned: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM projects p WHERE id=?1 AND kind='privateChat' AND updated_at=?2 AND NOT EXISTS(SELECT 1 FROM chats WHERE project_id=p.id))", params![id, revision], |row| row.get(0)).db()?;
            if owned {
                require_unreferenced(&transaction, &id, &folder)?;
                forget_empty(&transaction, &id)?;
            }
            transaction.commit().db()?;
            Ok(())
        })();
        if let Err(error) = result {
            failure.get_or_insert(error);
        }
        if seen >= MAX_ENTRIES {
            break;
        }
    }
    failure.map_or(Ok(()), Err)
}

fn require_unreferenced(
    connection: &rusqlite::Connection,
    id: &str,
    folder: &Path,
) -> AppResult<()> {
    use crate::project_registry::conservative_overlap_key;
    let folder = std::path::PathBuf::from(conservative_overlap_key(folder));
    let mut statement = connection
        .prepare("SELECT absolute_path FROM project_roots WHERE project_id<>?1")
        .db()?;
    let mut rows = statement.query([id]).db()?;
    while let Some(row) = rows.next().db()? {
        let stored = std::path::PathBuf::from(row.get::<_, String>(0).db()?);
        // Check the persisted identity even if the root is missing, and the
        // current identity if a registered path has since become a link.
        let resolved = match stored.canonicalize() {
            Ok(root) => Some(root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        for root in [Some(stored.clone()), resolved].into_iter().flatten() {
            let root = std::path::PathBuf::from(conservative_overlap_key(&root));
            // Ancestor projects may contain the entire Sage profile. They do
            // not own this managed folder; only a root at or inside it does.
            if root.starts_with(&folder) {
                return Err(AppError::InvalidRequest(
                    "private Chat folder overlaps another project root".into(),
                ));
            }
        }
    }
    Ok(())
}

fn forget_empty(transaction: &rusqlite::Transaction<'_>, id: &str) -> AppResult<()> {
    transaction
        .execute(
            "UPDATE app_state SET active_project_id=NULL WHERE active_project_id=?1",
            [id],
        )
        .db()?;
    transaction.execute("DELETE FROM acp_project_usage WHERE project_id=?1 AND chat_count=0 AND session_count=0 AND turn_count=0 AND item_count=0 AND fact_count=0 AND payload_bytes=0", [id]).db()?;
    transaction.execute("DELETE FROM projects WHERE id=?1 AND kind='privateChat' AND NOT EXISTS(SELECT 1 FROM chats WHERE project_id=?1)", [id]).db()?;
    Ok(())
}

pub(super) fn admit_base(path: &Path) -> AppResult<()> {
    ordinary_directory(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::InvalidRequest("missing private Chat profile".into()))?
        .canonicalize()?;
    let canonical = path.canonicalize()?;
    if canonical
        .parent()
        .map(crate::project_registry::authority_path_key)
        != Some(crate::project_registry::authority_path_key(&parent))
    {
        return Err(AppError::InvalidRequest(
            "private Chat storage escaped its profile".into(),
        ));
    }
    Ok(())
}

fn ordinary_directory(path: &Path) -> AppResult<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let link = metadata.file_type().is_symlink();
    if link || !metadata.is_dir() {
        return Err(AppError::InvalidRequest(
            "private Chat cleanup refuses linked directories".into(),
        ));
    }
    Ok(())
}

// Stream one entry at a time. Unlink links; refuse excessive depth and excessive
// work; retain the hidden project as a durable retry receipt on any failure.
#[cfg(windows)]
fn remove_tree(path: &Path, seen: &mut usize, depth: usize) -> AppResult<()> {
    if depth >= MAX_DEPTH {
        return Err(AppError::ResourceLimit(
            "private Chat cleanup depth exceeded".into(),
        ));
    }
    ordinary_directory(path)?;
    let identity = sage_workspace::root_identity_path(path)?;
    let guard = sage_workspace::open_held_parent(path, identity, "")?;
    for entry in fs::read_dir(path)? {
        *seen += 1;
        if *seen > MAX_ENTRIES {
            return Err(AppError::ResourceLimit(
                "private Chat cleanup entry limit exceeded".into(),
            ));
        }
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        #[cfg(windows)]
        let link = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let link = metadata.file_type().is_symlink();
        if link {
            // symlink_metadata reports directory links as links, not dirs.
            // Windows still requires RemoveDirectory for these reparse points.
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x10 != 0 {
                fs::remove_dir(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        } else if metadata.is_dir() {
            remove_tree(&entry.path(), seen, depth + 1)?;
        } else if metadata.is_file() {
            fs::remove_file(entry.path())?;
        } else {
            return Err(AppError::InvalidRequest(
                "private Chat cleanup refuses special files".into(),
            ));
        }
    }
    if sage_workspace::root_identity_path(path)? != identity {
        return Err(AppError::InvalidRequest(
            "private Chat folder changed during cleanup".into(),
        ));
    }
    drop(guard);
    fs::remove_dir(path)?;
    Ok(())
}

#[cfg(unix)]
fn remove_at(
    parent: std::os::fd::RawFd,
    name: &std::ffi::CStr,
    seen: &mut usize,
    depth: usize,
) -> AppResult<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if depth >= MAX_DEPTH {
        return Err(AppError::ResourceLimit(
            "private Chat cleanup depth exceeded".into(),
        ));
    }
    // All traversal and unlinking stays relative to retained descriptors.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let directory = unsafe { fs::File::from_raw_fd(fd) };
    let scan_fd = unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if scan_fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stream = unsafe { libc::fdopendir(scan_fd) };
    if stream.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(scan_fd);
        }
        return Err(error.into());
    }
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let stream = Stream(stream);
    loop {
        // Clear errno so end-of-directory is distinguishable from read failure.
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location() = 0;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error.into());
            }
            break;
        }
        let child = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if child.to_bytes() == b"." || child.to_bytes() == b".." {
            continue;
        }
        *seen += 1;
        if *seen > MAX_ENTRIES {
            return Err(AppError::ResourceLimit(
                "private Chat cleanup entry limit exceeded".into(),
            ));
        }
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                child.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let kind = unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT;
        if kind == libc::S_IFDIR {
            remove_at(directory.as_raw_fd(), child, seen, depth + 1)?;
        } else if kind == libc::S_IFREG || kind == libc::S_IFLNK {
            if unsafe { libc::unlinkat(directory.as_raw_fd(), child.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        } else {
            return Err(AppError::InvalidRequest(
                "private Chat cleanup refuses links and special files".into(),
            ));
        }
    }
    if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
