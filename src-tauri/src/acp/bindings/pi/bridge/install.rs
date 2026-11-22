//! Only the bridge bytes compiled into Sage can enable the bridge: the bundled
//! resource, or a profile copy written from those bytes when the resource is absent.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::acp::bindings::release_version::characterise_against;
/// The Pi release whose extension API the bridge was tested against. Later
/// releases load it too. Native launch requires this pinned resource.
pub(crate) const BRIDGE_PI_VERSION: &str = "1.5.0";
pub(crate) const SOURCE: &[u8] = include_bytes!("extension.mjs");
pub(crate) fn digest() -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(SOURCE)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
/// The bridge's path under Sage's resource directory. `bundle.resources` in
/// tauri.conf.json ships the file at exactly this path.
pub(crate) const RESOURCE: &str = "src/acp/bindings/pi/bridge/extension.mjs";
/// The profile copy's file name, inside the directory given at startup.
const PROFILE_COPY: &str = "extension.mjs";
static PROFILE_COPY_DIR: OnceLock<PathBuf> = OnceLock::new();

/// A binary run without its bundle resources (a lone `sage.exe`) still carries
/// the bridge bytes; Pi then loads an exact copy kept under the Sage profile.
/// Set once this process owns the profile.
pub(crate) fn use_profile_copy(dir: PathBuf) {
    let _ = PROFILE_COPY_DIR.set(dir);
}

/// Why the bridge cannot be enabled; native launch is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Unavailable {
    #[error("the Sage bridge needs Pi {BRIDGE_PI_VERSION} or later")]
    UnsupportedVersion,
    #[error("this Sage build is missing its Pi bridge resource")]
    Missing,
    #[error("this Sage build's Pi bridge resource does not match the pinned bytes")]
    Modified,
}

pub(crate) fn installed(version: &str) -> Result<PathBuf, Unavailable> {
    if !characterise_against(version, BRIDGE_PI_VERSION).runs_tested_features() {
        return Err(Unavailable::UnsupportedVersion);
    }
    locate(
        resource_root(),
        PROFILE_COPY_DIR.get().map(PathBuf::as_path),
    )
}

/// The bundled resource wins. Only a missing one falls back to the profile
/// copy; an altered resource still refuses launch.
pub(crate) fn locate(
    resources: Option<PathBuf>,
    profile_copy: Option<&Path>,
) -> Result<PathBuf, Unavailable> {
    match resources.ok_or(Unavailable::Missing).and_then(bundled) {
        Err(Unavailable::Missing) => profile_copy
            .ok_or(Unavailable::Missing)
            .and_then(write_profile_copy),
        found => found,
    }
}

fn bundled(root: PathBuf) -> Result<PathBuf, Unavailable> {
    let path = root.join(RESOURCE);
    let directory = path.parent().ok_or(Unavailable::Missing)?;
    let (Ok(directory), Ok(root)) = (directory.canonicalize(), root.canonicalize()) else {
        return Err(Unavailable::Missing);
    };
    if !directory.starts_with(root) {
        return Err(Unavailable::Modified);
    }
    matches_install(&directory, &path).map(|()| path)
}

/// A development build runs from a Cargo target directory that other checkouts may share and
/// overwrite, so it reads the bridge from the checkout it was compiled from.
/// Unit-test executables are not installed apps, including with `custom-protocol` enabled.
fn resource_root() -> Option<PathBuf> {
    if cfg!(test) || tauri::is_dev() {
        return Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    }
    let package = tauri::utils::PackageInfo {
        name: "Sage".into(),
        version: env!("CARGO_PKG_VERSION").parse().ok()?,
        authors: "",
        description: "",
        crate_name: "sage",
    };
    tauri::utils::platform::resource_dir(&package, &tauri::Env::default()).ok()
}

/// Keeps one copy per profile. Sage holds the profile lock, so only this
/// process replaces it, by renaming a fully written sibling over it.
fn write_profile_copy(dir: &Path) -> Result<PathBuf, Unavailable> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::fs::create_dir_all(dir).map_err(|_| Unavailable::Missing)?;
    let path = dir.join(PROFILE_COPY);
    if matches_install(dir, &path).is_ok() {
        return Ok(path);
    }
    let staged = dir.join(format!(
        "{PROFILE_COPY}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::File::create_new(&staged)
        .and_then(|mut file| file.write_all(SOURCE).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&staged, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
        return Err(Unavailable::Missing);
    }
    matches_install(dir, &path).map(|()| path)
}

pub(crate) fn matches_install(root: &Path, path: &Path) -> Result<(), Unavailable> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Unavailable::Missing);
        }
        Err(_) => return Err(Unavailable::Modified),
    };
    let root = root.canonicalize().map_err(|_| Unavailable::Missing)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != SOURCE.len() as u64 {
        return Err(Unavailable::Modified);
    }
    let canonical = path.canonicalize().map_err(|_| Unavailable::Modified)?;
    if canonical.parent() != Some(root.as_path()) {
        return Err(Unavailable::Modified);
    }
    let file = std::fs::File::open(path).map_err(|_| Unavailable::Modified)?;
    let mut bytes = Vec::new();
    file.take(SOURCE.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Unavailable::Modified)?;
    if bytes == SOURCE {
        Ok(())
    } else {
        Err(Unavailable::Modified)
    }
}
