use std::io::Read;

use sha2::{Digest, Sha256};

use super::*;

/// Larger files are not hashed: a rewind cannot tell whether they changed.
const CONTENT_DIGEST_BYTES_MAX: u64 = 16 * 1024 * 1024;

/// A file's bytes as one rewind check sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentState {
    Present([u8; 32]),
    Missing,
    /// Too large, over the caller's byte budget, changing while read, or not
    /// a regular file.
    Unreadable,
}

/// An owned, root-anchored handle to one file, so hashing can leave the
/// thread that mapped it. It holds no open file.
#[derive(Debug, Clone)]
pub(crate) struct ContentProbe {
    root: PathBuf,
    identity: RootIdentity,
    relative: String,
}

impl NativeResourceMapper {
    pub(crate) fn probe(&self, resource: &SafeResourceRef) -> Option<ContentProbe> {
        let root = self
            .roots
            .iter()
            .find(|root| root.root_id == resource.root_id)?;
        Some(ContentProbe {
            root: root.native_root.clone(),
            identity: root.identity,
            relative: resource.relative_path.as_str().to_owned(),
        })
    }
}

impl ContentProbe {
    /// Reads at most `budget` bytes, then charges what it read. A file larger
    /// than what is left reads as `Unreadable`.
    pub(crate) fn state(&self, budget: &mut u64) -> ContentState {
        let file = match open_workspace_text_file(&self.root, self.identity, &self.relative, None) {
            Ok(file) => file,
            Err(_) => {
                let missing = self
                    .root
                    .join(&self.relative)
                    .symlink_metadata()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                return if missing {
                    ContentState::Missing
                } else {
                    ContentState::Unreadable
                };
            }
        };
        digest_stable(&file, budget).map_or(ContentState::Unreadable, ContentState::Present)
    }
}

fn digest_stable(file: &std::fs::File, budget: &mut u64) -> Option<[u8; 32]> {
    let before = file.metadata().ok()?;
    if !before.is_file() || before.len() > CONTENT_DIGEST_BYTES_MAX.min(*budget) {
        return None;
    }
    *budget -= before.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut reader = file.take(before.len() + 1);
    let mut read = 0_u64;
    loop {
        let count = reader.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        read += count as u64;
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata().ok()?;
    (read == before.len()
        && read == after.len()
        && before.modified().ok()? == after.modified().ok()?)
    .then(|| hasher.finalize().into())
}
