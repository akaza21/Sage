//! Exclusive ownership of a durable profile directory.
//!
//! Startup recovery treats every row it finds as left over from a previous
//! run, so two Sage processes on one profile would recover each other's live
//! Turns, side chats and browser operations. The process that holds this lock
//! is the only one that may open the profile's databases. The operating system
//! releases the lock when the holder exits, crashes included, so a stale lock
//! file never blocks the next launch.

use std::{
    fmt,
    fs::{File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
};

const LOCK_FILE_NAME: &str = "sage.profile.lock";

/// Proof that this process owns a profile directory. Dropping it releases
/// ownership; Sage keeps it for the whole process lifetime.
#[derive(Debug)]
pub(crate) struct ProfileLock {
    _file: File,
}

#[derive(Debug)]
pub(crate) enum ProfileLockError {
    /// Another live process holds the profile.
    Held(PathBuf),
    Io(io::Error),
}

impl fmt::Display for ProfileLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Held(directory) => write!(
                formatter,
                "another Sage process owns the profile at {}",
                directory.display()
            ),
            Self::Io(error) => write!(formatter, "could not lock the Sage profile: {error}"),
        }
    }
}

impl std::error::Error for ProfileLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Held(_) => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for ProfileLockError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl ProfileLock {
    /// Takes the profile without waiting. `durable_dir` must already exist.
    pub(crate) fn acquire(durable_dir: &Path) -> Result<Self, ProfileLockError> {
        let file = open_plain_file(&durable_dir.join(LOCK_FILE_NAME))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ProfileLockError::Held(durable_dir.to_owned())),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

/// Opens the lock file itself, never what a link in its place points to: a
/// symbolic or hard link could tie the lock to a file another profile also
/// uses. The checks are made on the opened handle, so nothing can swap the
/// entry in between.
fn open_plain_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    platform::restrict(&mut options);
    let file = options.open(path)?;
    if !file.metadata()?.file_type().is_file() || !platform::is_single_plain_file(&file)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the Sage profile lock is not a plain file",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        io,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    };

    /// Opens a reparse point itself rather than its target, and withholds
    /// delete sharing so the file cannot be removed or renamed while this
    /// process owns the profile.
    pub(super) fn restrict(options: &mut OpenOptions) {
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }

    pub(super) fn is_single_plain_file(file: &File) -> io::Result<bool> {
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(
            information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && information.nNumberOfLinks == 1,
        )
    }
}

#[cfg(unix)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        io,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };

    /// Refuses a symbolic link at open.
    pub(super) fn restrict(options: &mut OpenOptions) {
        options.custom_flags(libc::O_NOFOLLOW);
    }

    pub(super) fn is_single_plain_file(file: &File) -> io::Result<bool> {
        Ok(file.metadata()?.nlink() == 1)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = crate::test_paths::temp_dir()
                .join(format!("sage-profile-lock-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_second_owner_is_refused_while_the_first_lives_and_admitted_after_it_drops() {
        let directory = TestDir::new();
        let first = ProfileLock::acquire(&directory.0).unwrap();

        let refused = ProfileLock::acquire(&directory.0).unwrap_err();
        assert!(
            matches!(&refused, ProfileLockError::Held(held) if *held == directory.0),
            "{refused}"
        );

        drop(first);
        let second = ProfileLock::acquire(&directory.0).unwrap();
        drop(second);
    }

    #[test]
    fn separate_profiles_lock_independently() {
        let normal = TestDir::new();
        let disposable = TestDir::new();
        let _normal = ProfileLock::acquire(&normal.0).unwrap();
        let _disposable = ProfileLock::acquire(&disposable.0).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_in_place_of_the_lock_is_refused() {
        let directory = TestDir::new();
        let elsewhere = TestDir::new();
        let target = elsewhere.0.join(LOCK_FILE_NAME);
        fs::write(&target, b"").unwrap();
        std::os::unix::fs::symlink(&target, directory.0.join(LOCK_FILE_NAME)).unwrap();

        let error = ProfileLock::acquire(&directory.0).unwrap_err();
        assert!(matches!(error, ProfileLockError::Io(_)), "{error}");
        // The link's target was never locked on the refused profile's behalf.
        let _owner = ProfileLock::acquire(&elsewhere.0).unwrap();
    }

    #[test]
    fn a_hard_link_in_place_of_the_lock_is_refused() {
        let directory = TestDir::new();
        let elsewhere = TestDir::new();
        let target = elsewhere.0.join(LOCK_FILE_NAME);
        fs::write(&target, b"").unwrap();
        fs::hard_link(&target, directory.0.join(LOCK_FILE_NAME)).unwrap();

        let error = ProfileLock::acquire(&directory.0).unwrap_err();
        assert!(matches!(error, ProfileLockError::Io(_)), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn the_lock_file_cannot_be_deleted_while_a_process_owns_the_profile() {
        let directory = TestDir::new();
        let owner = ProfileLock::acquire(&directory.0).unwrap();

        assert!(fs::remove_file(directory.0.join(LOCK_FILE_NAME)).is_err());
        assert!(matches!(
            ProfileLock::acquire(&directory.0),
            Err(ProfileLockError::Held(_))
        ));
        drop(owner);
    }
}
