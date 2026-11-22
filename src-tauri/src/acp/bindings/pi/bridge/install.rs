//! Only the bridge bytes compiled into Sage can enable the bridge: the bundled
//! resource, or a profile copy written from those bytes when the resource is absent.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
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
/// The one staging file a copy is written through before it replaces the copy.
const STAGED_COPY: &str = "extension.mjs.tmp";
/// Earlier builds staged through `extension.mjs.<pid>.<n>.tmp`; at most this
/// many directory entries are examined when sweeping leftovers.
const SWEEP_LIMIT: usize = 64;
static PROFILE_COPY_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Serialises copy writes within this process; the profile lock excludes others.
static COPY_WRITE: Mutex<()> = Mutex::new(());

/// A binary run without its bundle resources (a lone `sage.exe`) still carries
/// the bridge bytes; Pi then loads an exact copy kept under the Sage profile.
/// Called once this process holds the profile lock: it sweeps staging
/// leftovers and writes the copy when there is no bundled resource, so later
/// checks only verify it.
pub(crate) fn use_profile_copy(dir: PathBuf) {
    start_profile_copy(resource_root(), dir);
}

pub(crate) fn start_profile_copy(resources: Option<PathBuf>, dir: PathBuf) {
    let dir = PROFILE_COPY_DIR.get_or_init(|| dir);
    sweep_staged(dir);
    if let Err(error) = locate(resources, Some(dir)) {
        tracing::warn!(?error, "the Pi bridge is not available at startup");
    }
}

/// Removes staging files a crashed write left behind.
fn sweep_staged(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.take(SWEEP_LIMIT).flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("extension.mjs.") && name.ends_with(".tmp") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
    installed_with(version, resource_root(), profile_copy_dir())
}

/// The profile copy directory set at startup, if any.
pub(crate) fn profile_copy_dir() -> Option<&'static Path> {
    PROFILE_COPY_DIR.get().map(PathBuf::as_path)
}

pub(crate) fn installed_with(
    version: &str,
    resources: Option<PathBuf>,
    profile_copy: Option<&Path>,
) -> Result<PathBuf, Unavailable> {
    if !characterise_against(version, BRIDGE_PI_VERSION).runs_tested_features() {
        return Err(Unavailable::UnsupportedVersion);
    }
    locate(resources, profile_copy)
}

/// The bundled resource wins. Only a missing one falls back to the profile
/// copy, which is verified and rewritten only if it drifted; an altered
/// resource still refuses launch.
pub(crate) fn locate(
    resources: Option<PathBuf>,
    profile_copy: Option<&Path>,
) -> Result<PathBuf, Unavailable> {
    match resources.ok_or(Unavailable::Missing).and_then(bundled) {
        Err(Unavailable::Missing) => {
            let dir = profile_copy.ok_or(Unavailable::Missing)?;
            let path = dir.join(PROFILE_COPY);
            match matches_install(dir, &path) {
                Ok(()) => Ok(path),
                Err(_) => write_profile_copy(dir),
            }
        }
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

/// Keeps one copy per profile. Sage holds the profile lock and this process
/// serialises its writers, so one fixed staging file is renamed over the copy
/// once fully written. A writer that waited re-checks and finds it correct.
fn write_profile_copy(dir: &Path) -> Result<PathBuf, Unavailable> {
    let _writing = COPY_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::fs::create_dir_all(dir).map_err(|_| Unavailable::Missing)?;
    let path = dir.join(PROFILE_COPY);
    if matches_install(dir, &path).is_ok() {
        return Ok(path);
    }
    let staged = dir.join(STAGED_COPY);
    let written = std::fs::File::create(&staged)
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
