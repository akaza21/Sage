use std::{fs::File, io::Read, path::Path};

use serde_json::Value;

use crate::acp::limits::PREVIEW_LIMITS;

const MAX_MANIFEST_BYTES: u64 = PREVIEW_LIMITS.manifest_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageManifestError {
    Open,
    Metadata,
    Read,
    Malformed,
}

/// Reads one package manifest using the shared bounded admission policy.
/// `Ok(None)` identifies a truthful over-cap manifest.
pub(crate) fn read_bounded_package_manifest(
    path: &Path,
) -> Result<Option<Value>, PackageManifestError> {
    if is_reparse_point(path)? {
        return Ok(None);
    }
    let file = File::open(path).map_err(|_| PackageManifestError::Open)?;
    let metadata = file
        .metadata()
        .map_err(|_| PackageManifestError::Metadata)?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageManifestError::Read)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| PackageManifestError::Malformed)
}

/// Match Preview discovery's no-reparse policy before opening a manifest.
#[cfg(windows)]
fn is_reparse_point(path: &Path) -> Result<bool, PackageManifestError> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let metadata = path
        .symlink_metadata()
        .map_err(|_| PackageManifestError::Metadata)?;
    Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
fn is_reparse_point(path: &Path) -> Result<bool, PackageManifestError> {
    path.symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .map_err(|_| PackageManifestError::Metadata)
}
