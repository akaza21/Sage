use std::{
    fs::{File, OpenOptions, ReadDir},
    path::{Component, Path, PathBuf},
};

pub(super) struct Directory {
    anchors: Vec<File>,
    path: PathBuf,
}

impl Directory {
    pub(super) fn root(path: &Path) -> Option<Self> {
        Some(Self {
            anchors: vec![open_directory(path).ok()?],
            path: path.to_path_buf(),
        })
    }

    pub(super) fn child(&self, relative: &Path) -> Option<Self> {
        self.child_result(relative).ok()
    }

    fn child_result(&self, relative: &Path) -> std::io::Result<Self> {
        let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
        let mut anchors = vec![self.anchors.last().ok_or_else(invalid)?.try_clone()?];
        let mut path = self.path.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid());
            };
            path.push(name);
            #[cfg(unix)]
            let opened_path = descriptor_path(anchors.last().ok_or_else(invalid)?).join(name);
            #[cfg(not(unix))]
            let opened_path = path.clone();
            anchors.push(open_directory(&opened_path)?);
        }
        Ok(Self { anchors, path })
    }

    pub(super) fn identity(&self) -> Option<[u8; 24]> {
        let file = self.anchors.last()?;
        crate::workspace::leaf_identity(file, &file.metadata().ok()?)
            .ok()
            .map(|identity| identity.stable_bytes())
    }

    pub(super) fn entries(&self) -> Option<ReadDir> {
        #[cfg(unix)]
        let path = descriptor_path(self.anchors.last()?);
        #[cfg(not(unix))]
        let path = self.path.clone();
        std::fs::read_dir(path).ok()
    }

    pub(super) fn metadata(&self, relative: &Path) -> Option<Option<std::fs::Metadata>> {
        let parent_path = relative.parent()?;
        let parent = match self.child_result(parent_path) {
            Ok(parent) => parent,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(None),
            Err(_) => return None,
        };
        #[cfg(unix)]
        let path = descriptor_path(parent.anchors.last()?).join(relative.file_name()?);
        #[cfg(not(unix))]
        let path = parent.path.join(relative.file_name()?);
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return None,
        };
        (self.child(parent_path)?.identity()? == parent.identity()?).then_some(metadata)
    }
}

#[cfg(unix)]
fn descriptor_path(file: &File) -> PathBuf {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    let prefix = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    let prefix = "/dev/fd";
    Path::new(prefix).join(file.as_raw_fd().to_string())
}

#[cfg(unix)]
fn open_directory(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_directory(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        Ok(file)
    } else {
        Err(std::io::ErrorKind::InvalidInput.into())
    }
}

#[cfg(not(any(unix, windows)))]
fn open_directory(_path: &Path) -> std::io::Result<File> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_directory_substitution_is_rejected_or_stays_pinned() {
        let root = std::env::temp_dir().join(format!("sage-anchor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("child")).unwrap();
        let anchor = Directory::root(&root).unwrap();
        let child = anchor.child(Path::new("child")).unwrap();
        let identity = child.identity();
        let renamed = std::fs::rename(root.join("child"), root.join("old"));
        if renamed.is_ok() {
            std::fs::create_dir(root.join("child")).unwrap();
            assert_ne!(
                anchor.child(Path::new("child")).unwrap().identity(),
                identity
            );
            assert_eq!(child.identity(), identity);
        } else {
            assert_eq!(
                anchor.child(Path::new("child")).unwrap().identity(),
                identity
            );
        }
        drop(child);
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_substitution_never_enumerates_the_target() {
        let root = std::env::temp_dir().join(format!("sage-anchor-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("outside")).unwrap();
        let anchor = Directory::root(&root).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("child")).unwrap();
        assert!(anchor.child(Path::new("child")).is_none());
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fingerprinting_after_enumeration_cannot_read_a_substituted_parent() {
        let root =
            std::env::temp_dir().join(format!("sage-fingerprint-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("child")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("child/file"), "original").unwrap();
        std::fs::write(
            root.join("outside/file"),
            "external target with a different size",
        )
        .unwrap();
        let anchor = Directory::root(&root).unwrap();
        let child = anchor.child(Path::new("child")).unwrap();
        assert_eq!(child.entries().unwrap().count(), 1);
        std::fs::rename(root.join("child"), root.join("old")).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("child")).unwrap();
        assert!(anchor.metadata(Path::new("child/file")).is_none());
        assert_eq!(child.metadata(Path::new("file")).unwrap().unwrap().len(), 8);
        drop(child);
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }
}
