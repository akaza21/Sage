//! The item is never copied: a volume with no usable Trash refuses before
//! anything moves.
//!
//! The `.trashinfo` record is created exclusively and flushed before the item
//! moves, so a trashed item always has the record that restores it. The move
//! is a no-replace rename from the held parent into the held `files`
//! directory, so it can neither replace an older trashed item nor follow a
//! path that changed.

use std::{
    ffi::{CStr, CString},
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt},
        },
    },
    path::{Path, PathBuf},
    sync::Arc,
};

use sage_core::error::{AppError, AppResult};

use super::{ResourceTrashAuthority, TRASH_NAME, open_directory, stat_at, stat_identity};

/// The first name, then `name.2.ext` … `name.1000.ext`.
const MAX_NAME_ATTEMPTS: u32 = 1000;
const MAX_VOLUME_DEPTH: usize = 4096;

pub(super) struct TrashDirectory {
    pub(super) files: File,
    pub(super) info: File,
}

fn no_trash() -> AppError {
    AppError::Unsupported(
        "This drive has no Trash that Sage can use, so nothing was moved. Delete the item from a file manager or terminal instead."
            .into(),
    )
}

/// `$XDG_DATA_HOME`, or `$HOME/.local/share`, when absolute.
fn home_data_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = super::tests::home_override() {
        return Some(home);
    }
    let absolute = |value: std::ffi::OsString| {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    };
    std::env::var_os("XDG_DATA_HOME")
        .and_then(absolute)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|home| home.join(".local/share"))
        })
}

fn owned_directory(parent: &File, name: &CStr) -> std::io::Result<File> {
    // SAFETY: mkdirat reads a NUL-terminated name relative to a held directory.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error);
        }
    }
    let directory = open_directory(Some(parent), name, libc::O_RDONLY)?;
    // SAFETY: getuid cannot fail.
    if directory.metadata()?.uid() != unsafe { libc::getuid() } {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(directory)
}

pub(super) fn trash_directory(base: &File) -> std::io::Result<TrashDirectory> {
    Ok(TrashDirectory {
        files: owned_directory(base, c"files")?,
        info: owned_directory(base, c"info")?,
    })
}

fn home_trash(volume: u64) -> Option<TrashDirectory> {
    let base = home_data_dir()?.join("Trash");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&base)
        .ok()?;
    // The home Trash may itself be a link, as GLib allows; it must still be
    // this user's directory.
    let home = File::open(&base).ok()?;
    let metadata = home.metadata().ok()?;
    // SAFETY: getuid cannot fail.
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::getuid() } {
        return None;
    }
    let trash = trash_directory(&home).ok()?;
    (trash.files.metadata().ok()?.dev() == volume).then_some(trash)
}

/// Also returns how many levels above `parent` the top is.
pub(super) fn volume_top(parent: &File, volume: u64) -> AppResult<(File, usize)> {
    let mut current = open_directory(Some(parent), c".", libc::O_PATH)?;
    if current.metadata()?.dev() != volume {
        return Err(no_trash());
    }
    for steps in 0..MAX_VOLUME_DEPTH {
        let up = open_directory(Some(&current), c"..", libc::O_PATH)?;
        let (here, above) = (current.metadata()?, up.metadata()?);
        if above.dev() != volume || (above.dev(), above.ino()) == (here.dev(), here.ino()) {
            return Ok((current, steps));
        }
        current = up;
    }
    Err(AppError::ResourceLimit(
        "the Trash volume search exceeded its depth bound".into(),
    ))
}

fn uid_name(prefix: &str) -> CString {
    // SAFETY: getuid cannot fail.
    CString::new(format!("{prefix}{}", unsafe { libc::getuid() }))
        .expect("a decimal uid contains no NUL")
}

pub(super) fn topdir_trash(top: &File, volume: u64) -> Option<TrashDirectory> {
    // The shared directory counts only when it is a sticky directory, not a
    // link, so another user cannot have planted it.
    let shared = open_directory(Some(top), c".Trash", libc::O_RDONLY)
        .ok()
        .filter(|shared| {
            shared
                .metadata()
                .is_ok_and(|metadata| metadata.mode() & libc::S_ISVTX != 0)
        })
        .and_then(|shared| owned_directory(&shared, &uid_name("")).ok())
        .and_then(|base| trash_directory(&base).ok());
    let trash = match shared {
        Some(trash) => trash,
        None => trash_directory(&owned_directory(top, &uid_name(".Trash-")).ok()?).ok()?,
    };
    (trash.files.metadata().ok()?.dev() == volume).then_some(trash)
}

/// The source's path below the volume top, which records in a per-volume
/// Trash hold so the item restores wherever the volume is mounted. The path
/// must lead to the same top that the descriptor walk found.
pub(super) fn path_below_top(source: &Path, top: &File, steps: usize) -> AppResult<Vec<u8>> {
    let parent = source.parent().ok_or_else(no_trash)?;
    let top_path = parent.ancestors().nth(steps).ok_or_else(no_trash)?;
    let (visible, held) = (std::fs::metadata(top_path)?, top.metadata()?);
    if (visible.dev(), visible.ino()) != (held.dev(), held.ino()) {
        return Err(no_trash());
    }
    let relative = source.strip_prefix(top_path).map_err(|_| no_trash())?;
    Ok(relative.as_os_str().as_bytes().to_vec())
}

/// Local time as the specification's `YYYY-MM-DDThh:mm:ss`.
fn deletion_date() -> String {
    // SAFETY: time accepts a null output pointer.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: an all-zero tm is a valid value for localtime_r to overwrite.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live, writable values.
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return "1970-01-01T00:00:00".into();
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// The record that restores `original`: an absolute path in the home
/// Trash, a path below the volume top otherwise. It is percent-encoded as
/// GLib does, leaving only unreserved characters and `/`.
pub(super) fn trash_info(original: &[u8], date: &str) -> String {
    let mut path = String::with_capacity(original.len());
    for &byte in original {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            path.push(char::from(byte));
        } else {
            path.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("[Trash Info]\nPath={path}\nDeletionDate={date}\n")
}

fn unlink(directory: &File, name: &CStr) {
    // SAFETY: unlinkat reads a NUL-terminated name relative to a held directory.
    unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
}

/// `Ok(false)` means the name is taken.
fn write_info(info: &File, name: &CStr, contents: &str) -> std::io::Result<bool> {
    // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
    let fd = unsafe {
        libc::openat(
            info.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::AlreadyExists {
            Ok(false)
        } else {
            Err(error)
        };
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    let mut file = unsafe { File::from_raw_fd(fd) };
    if let Err(error) = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
    {
        unlink(info, name);
        return Err(error);
    }
    Ok(true)
}

fn rename_noreplace(from: (&File, &CStr), to: (&File, &CStr)) -> std::io::Result<()> {
    // SAFETY: both names are NUL-terminated and relative to held directories.
    if unsafe {
        libc::renameat2(
            from.0.as_raw_fd(),
            from.1.as_ptr(),
            to.0.as_raw_fd(),
            to.1.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The `attempt`th name for `source` in the trash, numbered before the
/// extension as GLib does, so `notes.txt` becomes `notes.2.txt`.
pub(super) fn numbered(source: &str, attempt: u32) -> String {
    if attempt == 1 {
        return source.to_owned();
    }
    match source.rfind('.').filter(|&dot| dot > 0) {
        Some(dot) => format!("{}.{attempt}{}", &source[..dot], &source[dot..]),
        None => format!("{source}.{attempt}"),
    }
}

/// Every failure leaves the source in place; removing its `.trashinfo` record
/// is best-effort.
fn move_into(
    authority: &ResourceTrashAuthority,
    trash: &TrashDirectory,
    contents: &str,
) -> AppResult<(CString, CString)> {
    let source = authority
        .name
        .to_str()
        .map_err(|_| AppError::InvalidRequest("Trash source name is not UTF-8".into()))?;
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let name = numbered(source, attempt);
        let info_name =
            CString::new(format!("{name}.trashinfo")).expect("a validated name contains no NUL");
        let name = CString::new(name).expect("a validated name contains no NUL");
        if !write_info(&trash.info, &info_name, contents)? {
            continue;
        }
        match rename_noreplace((&authority.parent, &authority.name), (&trash.files, &name)) {
            Ok(()) => return Ok((name, info_name)),
            Err(error) => {
                // A reply lost on a network volume can report a rename that
                // happened; the held source arriving proves it did.
                if stat_at(&trash.files, &name)
                    .is_ok_and(|stat| stat_identity(stat) == authority.proof.source)
                {
                    return Ok((name, info_name));
                }
                unlink(&trash.info, &info_name);
                match error.raw_os_error() {
                    // An item without a record already holds the name.
                    Some(libc::EEXIST) => continue,
                    // Replacing would be the only fallback, so there is none.
                    Some(libc::EINVAL | libc::ENOTSUP) => return Err(no_trash()),
                    _ => return Err(error.into()),
                }
            }
        }
    }
    Err(AppError::ResourceLimit(
        "the Trash already holds too many items with this name".into(),
    ))
}

pub(super) fn move_to_trash<C>(
    authority: &Arc<ResourceTrashAuthority>,
    mutation_started: C,
) -> AppResult<Option<String>>
where
    C: FnOnce(Arc<ResourceTrashAuthority>),
{
    let volume = authority.proof.parent.volume;
    let date = deletion_date();
    let home = home_trash(volume).map(|trash| {
        let contents = trash_info(authority.source_path.as_os_str().as_bytes(), &date);
        let moved = move_into(authority, &trash, &contents);
        (trash, moved)
    });
    let (trash, (name, info_name)) = match home {
        // A bind mount can share the device yet refuse the rename; nothing
        // moved, so the volume's own Trash is tried as GLib does.
        Some((_, Err(AppError::Io(error)))) if error.raw_os_error() == Some(libc::EXDEV) => {
            move_to_volume_trash(authority, volume, &date)?
        }
        Some((trash, moved)) => (trash, moved?),
        None => move_to_volume_trash(authority, volume, &date)?,
    };
    // The rename is atomic, so the filesystem has changed only from here.
    mutation_started(Arc::clone(authority));
    Ok(settle(authority, &trash, &name, &info_name))
}

fn move_to_volume_trash(
    authority: &ResourceTrashAuthority,
    volume: u64,
    date: &str,
) -> AppResult<(TrashDirectory, (CString, CString))> {
    let (top, steps) = volume_top(&authority.parent, volume)?;
    // Checked before any Trash folder is made, so a top that the visible
    // path does not reach is never written to.
    let below = path_below_top(&authority.source_path, &top, steps)?;
    let trash = topdir_trash(&top, volume).ok_or_else(no_trash)?;
    let moved = move_into(authority, &trash, &trash_info(&below, date))?;
    Ok((trash, moved))
}

fn changed_item_warning(put_back: bool) -> String {
    if put_back {
        format!(
            "A different item was at this name when it was moved to the {TRASH_NAME}, so Sage put that item back; Explorer is refreshing."
        )
    } else {
        format!(
            "A different item was at this name when it was moved to the {TRASH_NAME} and could not be put back; restore it from the {TRASH_NAME}. Explorer is refreshing."
        )
    }
}

/// Checks that the item now in the trash is the held source. The move is by
/// name, so an item swapped in at the source name in the instant after the
/// last check would have moved instead. Rollback is attempted the same way, which
/// replaces nothing; on failure the item stays in Trash with a warning.
pub(super) fn settle(
    authority: &ResourceTrashAuthority,
    trash: &TrashDirectory,
    name: &CStr,
    info_name: &CStr,
) -> Option<String> {
    if stat_at(&trash.files, name).is_ok_and(|stat| stat_identity(stat) == authority.proof.source) {
        return None;
    }
    let put_back =
        rename_noreplace((&trash.files, name), (&authority.parent, &authority.name)).is_ok();
    if put_back {
        unlink(&trash.info, info_name);
    }
    Some(changed_item_warning(put_back))
}
