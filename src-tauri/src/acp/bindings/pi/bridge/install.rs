//! Only the fixed resource bundled with Sage can enable the bridge.
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const BRIDGE_PI_VERSION: &str = "0.83.0";
pub(crate) const SOURCE: &[u8] = include_bytes!("extension.mjs");

pub(crate) fn installed(version: &str) -> Option<PathBuf> {
    if version != super::super::PI_VERSION || version != BRIDGE_PI_VERSION {
        return None;
    }
    let package = tauri::utils::PackageInfo {
        name: "Sage".into(),
        version: env!("CARGO_PKG_VERSION").parse().ok()?,
        authors: "",
        description: "",
        crate_name: "sage",
    };
    let root = tauri::utils::platform::resource_dir(&package, &tauri::Env::default()).ok()?;
    let directory = root.join("src/acp/bindings/pi/bridge");
    if !directory
        .canonicalize()
        .ok()?
        .starts_with(root.canonicalize().ok()?)
    {
        return None;
    }
    let path = directory.join("extension.mjs");
    matches_install(&directory, &path).then_some(path)
}

pub(super) fn matches_install(root: &Path, path: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != SOURCE.len() as u64 {
        return false;
    }
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    if canonical.parent() != Some(root.as_path()) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    file.take(SOURCE.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes == SOURCE
}
