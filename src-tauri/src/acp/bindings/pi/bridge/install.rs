//! Only the fixed resource bundled with Sage can enable the bridge.
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use crate::acp::bindings::release_version::characterise_against;
/// The Pi release whose extension API the bridge was tested against. Later
/// releases load it too; if a release stops loading it, Pi starts with no
/// tools rather than unguarded ones.
const BRIDGE_PI_VERSION: &str = "1.5.0";
pub(crate) const SOURCE: &[u8] = include_bytes!("extension.mjs");
/// The bridge's path under Sage's resource directory. `bundle.resources` in
/// tauri.conf.json ships the file at exactly this path.
pub(crate) const RESOURCE: &str = "src/acp/bindings/pi/bridge/extension.mjs";

/// Why the bridge cannot be enabled; Pi then runs with read-only tools.
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
    let root = resource_root().ok_or(Unavailable::Missing)?;
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

/// Sage's resource directory. A development build runs from a Cargo target
/// directory that other checkouts may share and overwrite, so it reads the
/// bridge from the checkout it was compiled from, which holds the same bytes it
/// embeds as [`SOURCE`]. A packaged build reads its own bundled resource.
fn resource_root() -> Option<PathBuf> {
    if tauri::is_dev() {
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

pub(super) fn matches_install(root: &Path, path: &Path) -> Result<(), Unavailable> {
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
