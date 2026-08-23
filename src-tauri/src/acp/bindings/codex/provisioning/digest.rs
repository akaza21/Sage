use std::path::Path;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

use sha2::{Digest, Sha256};

use super::SourceFile;

pub(crate) fn source_proof_digest(root: &Path, files: &[SourceFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sage-codex-source-proof-v1\0");
    hash_native_path(&mut hasher, root);
    for file in files {
        hasher.update(b"\0");
        hash_native_path(&mut hasher, &file.relative_path);
        hasher.update(b"\0");
        hasher.update(file.sha256);
        hasher.update(b"\0");
        hasher.update(file.byte_len.to_le_bytes());
    }
    hex(&hasher.finalize())
}

#[cfg(windows)]
pub(crate) fn hash_native_path(hasher: &mut Sha256, path: &Path) {
    for unit in path.as_os_str().encode_wide() {
        hasher.update(unit.to_le_bytes());
    }
}

#[cfg(not(windows))]
pub(crate) fn hash_native_path(hasher: &mut Sha256, path: &Path) {
    hasher.update(path.as_os_str().as_encoded_bytes());
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    value
}
