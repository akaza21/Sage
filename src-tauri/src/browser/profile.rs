use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::launch_profile::{LaunchProfile, normalize_without_creating, require_plain_directory};

pub(super) fn browser_profile_dir(
    profile: &LaunchProfile,
    ui_webview_dir: PathBuf,
    active_project_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let ui_dir = normalize_without_creating(&ui_webview_dir).map_err(|error| error.to_string())?;
    let profile_dir = match profile {
        LaunchProfile::Normal { .. } => ui_dir
            .parent()
            .ok_or("browser profile parent is unavailable")?
            .join("ai.sage.desktop.browser"),
        LaunchProfile::Disposable { webview_dir, .. } => webview_dir
            .parent()
            .ok_or("browser profile parent is unavailable")?
            .join("browser"),
    };
    let profile_dir =
        normalize_without_creating(&profile_dir).map_err(|error| error.to_string())?;
    let durable_dir =
        normalize_without_creating(profile.durable_dir()).map_err(|error| error.to_string())?;
    if profile_dir.starts_with(&ui_dir)
        || ui_dir.starts_with(&profile_dir)
        || profile_dir.starts_with(&durable_dir)
        || durable_dir.starts_with(&profile_dir)
    {
        return Err("browser profile overlaps Sage storage".into());
    }
    let project_roots = active_project_roots
        .iter()
        .map(PathBuf::as_path)
        .chain(profile.project_root())
        .collect::<Vec<_>>();
    validate_project_roots(&profile_dir, &project_roots)?;
    fs::create_dir_all(&profile_dir).map_err(|error| error.to_string())?;
    require_plain_directory(&profile_dir).map_err(|error| error.to_string())?;
    if profile_dir
        .canonicalize()
        .map_err(|error| error.to_string())?
        != profile_dir
    {
        return Err("browser profile directory changed during admission".into());
    }
    Ok(profile_dir)
}

pub(super) fn validate_project_roots(profile_dir: &Path, roots: &[&Path]) -> Result<(), String> {
    for root in roots {
        let root = normalize_without_creating(root).map_err(|error| error.to_string())?;
        if profile_dir.starts_with(&root) || root.starts_with(profile_dir) {
            return Err("browser profile overlaps project storage".into());
        }
    }
    Ok(())
}
