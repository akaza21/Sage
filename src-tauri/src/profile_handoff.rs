//! Launching Sage again brings the running copy to the front.
//!
//! The process that owns a profile listens on a doorbell named after that
//! profile. A later launch that finds the profile owned rings it and exits;
//! the owner raises its main window. Each profile has its own doorbell, so a
//! test build on a disposable profile never rings the installed app.
//!
//! The doorbell is a Windows named event in the session's `Local\` namespace,
//! created with the process token's default security: one listener thread,
//! and repeated rings coalesce into one raise. Other platforms have no
//! doorbell; a second launch there reports on stderr instead.

use std::{io, path::Path};

#[cfg(windows)]
mod named_event;

/// Starts raising `app`'s main window whenever a later launch of this
/// profile rings. Call it once, after the main window exists, from the
/// process that owns the profile.
#[cfg(windows)]
pub(crate) fn listen(app: &tauri::AppHandle, durable_dir: &Path) -> io::Result<()> {
    let app = app.clone();
    named_event::listen(&doorbell_name(durable_dir)?, move || {
        let raised = app.clone();
        let _ = app.run_on_main_thread(move || raise(&raised));
    })
}

#[cfg(not(windows))]
pub(crate) fn listen(_app: &tauri::AppHandle, _durable_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Asks the process that owns `durable_dir` to come to the front. Fails when
/// no owner is listening.
#[cfg(windows)]
pub(crate) fn ring(durable_dir: &Path) -> io::Result<()> {
    named_event::ring(&doorbell_name(durable_dir)?)
}

#[cfg(not(windows))]
pub(crate) fn ring(_durable_dir: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(windows)]
fn raise(app: &tauri::AppHandle) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// One name per profile folder, however the path was spelled. NTFS folds
/// case, so the Windows key does too.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn doorbell_name(durable_dir: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};

    let canonical = durable_dir.canonicalize()?;
    let key = canonical.to_string_lossy();
    let key = if cfg!(windows) {
        key.to_lowercase()
    } else {
        key.into_owned()
    };
    let digest = Sha256::digest(key.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(r"Local\ai.sage.desktop.profile.{hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_profile_folder_has_one_session_scoped_doorbell() {
        let root = crate::test_paths::temp_dir()
            .join(format!("sage-profile-handoff-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();

        let name = doorbell_name(&first).unwrap();
        let respelled = doorbell_name(&first.join("..").join("first")).unwrap();
        let other = doorbell_name(&second).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        assert!(name.starts_with(r"Local\"));
        assert_eq!(name, respelled);
        assert_ne!(name, other);
    }
}
