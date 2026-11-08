use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf},
    ptr,
};

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA, FileRenameInfoEx,
    GetFileInformationByHandle, GetFinalPathNameByHandleW, SetFileInformationByHandle,
    VOLUME_NAME_DOS,
};

fn refused() -> io::Error {
    io::ErrorKind::PermissionDenied.into()
}

fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut buffer = vec![0_u16; 32 * 1024];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| refused())?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn open_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(refused());
    }
    if final_path(&file)? != path.canonicalize()? {
        return Err(refused());
    }
    Ok(file)
}

fn hold_parent(root: &Path, identity: [u8; 24], parent: &Path) -> io::Result<Vec<File>> {
    let relative = parent.strip_prefix(root).map_err(|_| refused())?;
    let root_guard = open_directory(root)?;
    let observed = crate::workspace::root_identity_file(&root_guard).map_err(|_| refused())?;
    if observed.stable_bytes() != identity {
        return Err(refused());
    }
    let mut guards = vec![root_guard];
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(refused());
        };
        path.push(name);
        guards.push(open_directory(&path)?);
    }
    Ok(guards)
}

fn check_target(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(refused());
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(refused());
            }
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) }
                == 0
                || information.nNumberOfLinks != 1
                || final_path(&file)? != path
            {
                return Err(refused());
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn already_contains(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    let mut file = match OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || final_path(&file)? != path
    {
        return Err(refused());
    }
    let mut existing = Vec::with_capacity(bytes.len().saturating_add(1));
    Read::by_ref(&mut file)
        .take(u64::try_from(bytes.len().saturating_add(1)).map_err(|_| refused())?)
        .read_to_end(&mut existing)?;
    Ok(existing == bytes)
}

/// What the final rename may do with a file already at the target name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Existing {
    Replace,
    /// Create only: a file that appeared after the last check is never replaced.
    Refuse,
}

fn rename_information(target: &Path, existing: Existing) -> io::Result<(Vec<usize>, u32)> {
    let name = target.as_os_str().encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(refused)?;
    let total = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .and_then(|length| length.checked_add(size_of::<u16>()))
        .ok_or_else(refused)?;
    // FileNameLength excludes the NUL; the Win32 request must still own it.
    let mut storage = vec![0_usize; total.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: aligned, zeroed storage holds the header, UTF-16 name and terminator.
    unsafe {
        // POSIX_SEMANTICS, plus REPLACE_IF_EXISTS when replacing: swap a
        // previously opened target name atomically without writing through its
        // old file handle. Without it the rename fails if the name exists.
        (*info).Anonymous.Flags = if existing == Existing::Replace {
            1 | 2
        } else {
            2
        };
        (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = u32::try_from(name_bytes).map_err(|_| refused())?;
        ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    Ok((storage, u32::try_from(total).map_err(|_| refused())?))
}

fn rename_held(source: &File, target: &Path, existing: Existing) -> io::Result<()> {
    let (mut storage, total) = rename_information(target, existing)?;
    // SAFETY: the request owns its aligned header and terminated UTF-16 name.
    unsafe {
        if SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfoEx,
            storage.as_mut_ptr().cast(),
            total,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(
    root: &Path,
    identity: [u8; 24],
    path: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    atomic_write_checked(root, identity, path, bytes, Existing::Replace, || Ok(()))
}

/// Writes through a temporary sibling while the parent chain is held.
/// `verify` runs after the temporary file is durable and immediately before
/// the rename, so a caller can re-check that the target is still what it
/// approved.
pub(crate) fn atomic_write_checked(
    root: &Path,
    identity: [u8; 24],
    path: &Path,
    bytes: &[u8],
    existing: Existing,
    verify: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(refused)?.canonicalize()?;
    let _guards = hold_parent(root, identity, &parent)?;
    let target = parent.join(path.file_name().ok_or_else(refused)?);
    check_target(&target)?;
    if existing == Existing::Replace && already_contains(&target, bytes)? {
        return Ok(());
    }
    let temp = parent.join(format!(".sage-acp-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_DATA)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        check_target(&target)?;
        verify()?;
        rename_held(&file, &target, existing)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_information_terminates_every_utf16_alignment() {
        for suffix in ["", "a", "ab", "abc"] {
            let path = PathBuf::from(format!(r"C:\fixture\lock-🔒{suffix}.txt"));
            let expected = path.as_os_str().encode_wide().collect::<Vec<_>>();
            let (storage, length) = rename_information(&path, Existing::Replace).unwrap();
            let end = offset_of!(FILE_RENAME_INFO, FileName) + expected.len() * 2;
            assert!(
                length as usize >= end + 2,
                "request must include its terminator"
            );
            assert!(storage.len() * size_of::<usize>() >= length as usize);
            // SAFETY: the builder owns the header and the bounds above include the terminator.
            unsafe {
                let info = storage.as_ptr().cast::<FILE_RENAME_INFO>();
                assert_eq!((*info).FileNameLength as usize, expected.len() * 2);
                let name =
                    std::slice::from_raw_parts((*info).FileName.as_ptr(), expected.len() + 1);
                assert_eq!(&name[..expected.len()], expected);
                assert_eq!(name[expected.len()], 0);
            }
        }
    }

    #[test]
    fn atomic_write_reaches_the_exact_target_for_every_alignment() {
        let root = std::env::temp_dir().join(format!("sage-write-nul-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let identity = crate::workspace::root_identity_path(&root)
            .unwrap()
            .stable_bytes();
        for suffix in ["", "a", "ab", "abc"] {
            let target = root.join(format!("after{suffix}.txt"));
            atomic_write(&root, identity, &target, b"retained").unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), b"retained");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn held_parent_component_cannot_be_swapped() {
        let root = std::env::temp_dir().join(format!("sage-opencode-pin-{}", uuid::Uuid::new_v4()));
        let child = root.join("child");
        std::fs::create_dir_all(&child).expect("dirs");
        let identity = crate::workspace::root_identity_path(&root)
            .expect("identity")
            .stable_bytes();
        let guards = hold_parent(&root, identity, &child).expect("held parent");
        assert!(std::fs::rename(&child, root.join("swapped")).is_err());
        drop(guards);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn atomic_write_accepts_an_identical_open_target() {
        let root =
            std::env::temp_dir().join(format!("sage-opencode-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let target = root.join("target.txt");
        std::fs::write(&target, b"old").expect("old file");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&target)
            .expect("hold target without delete sharing");
        let identity = crate::workspace::root_identity_path(&root)
            .expect("identity")
            .stable_bytes();
        atomic_write(
            &root.canonicalize().expect("canonical root"),
            identity,
            &target,
            b"old",
        )
        .expect("idempotent write");
        assert_eq!(std::fs::read(&target).expect("same file"), b"old");
        drop(held);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn create_only_never_replaces_a_file_that_appears_before_the_rename() {
        let root = std::env::temp_dir().join(format!("sage-create-only-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let root = root.canonicalize().expect("canonical root");
        let identity = crate::workspace::root_identity_path(&root)
            .expect("identity")
            .stable_bytes();
        let target = root.join("new.txt");
        let raced =
            atomic_write_checked(&root, identity, &target, b"agent", Existing::Refuse, || {
                std::fs::write(&target, b"user")
            });
        assert!(raced.is_err());
        assert_eq!(std::fs::read(&target).expect("user file"), b"user");
        let duplicate =
            atomic_write_checked(&root, identity, &target, b"user", Existing::Refuse, || {
                Ok(())
            });
        assert!(
            duplicate.is_err(),
            "an identical existing file is still not a create"
        );
        let changed = root.join("changed.txt");
        std::fs::write(&changed, b"old").expect("old");
        let vetoed =
            atomic_write_checked(&root, identity, &changed, b"new", Existing::Replace, || {
                Err(refused())
            });
        assert!(vetoed.is_err());
        assert_eq!(std::fs::read(&changed).expect("kept"), b"old");
        assert_eq!(
            std::fs::read_dir(&root).expect("list").count(),
            2,
            "no temporary file left"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }
    #[test]
    fn atomic_write_replaces_an_unlocked_target() {
        let root =
            std::env::temp_dir().join(format!("sage-opencode-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let target = root.join("target.txt");
        std::fs::write(&target, b"old").expect("old file");
        let identity = crate::workspace::root_identity_path(&root)
            .expect("identity")
            .stable_bytes();
        atomic_write(
            &root.canonicalize().expect("canonical root"),
            identity,
            &target,
            b"new",
        )
        .expect("atomic replacement");
        assert_eq!(std::fs::read(&target).expect("new file"), b"new");
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
