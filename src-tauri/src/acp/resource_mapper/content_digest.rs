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
    /// Too large, changing while read, or not a regular file.
    Unreadable,
}

impl NativeResourceMapper {
    pub(crate) fn content_state(&self, resource: &SafeResourceRef) -> ContentState {
        let Some(root) = self
            .roots
            .iter()
            .find(|root| root.root_id == resource.root_id)
        else {
            return ContentState::Unreadable;
        };
        let relative = resource.relative_path.as_str();
        let file = match open_workspace_text_file(&root.native_root, root.identity, relative, None)
        {
            Ok(file) => file,
            Err(_) => {
                let missing = root
                    .native_root
                    .join(relative)
                    .symlink_metadata()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                return if missing {
                    ContentState::Missing
                } else {
                    ContentState::Unreadable
                };
            }
        };
        digest_stable(&file).map_or(ContentState::Unreadable, ContentState::Present)
    }
}

fn digest_stable(file: &std::fs::File) -> Option<[u8; 32]> {
    let before = file.metadata().ok()?;
    if !before.is_file() || before.len() > CONTENT_DIGEST_BYTES_MAX {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut reader = file.take(CONTENT_DIGEST_BYTES_MAX + 1);
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
