use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use tauri::{AppHandle, Manager};

use super::super::{
    BrowserState, DownloadRegistry,
    folder::{self, Plan},
};
use crate::{settings_commands::browser_download_folder::remember_folder, state::AppState};

/// Silent save into the saved download folder, or the Save dialog.
pub(super) fn plan_download(app: &AppHandle, suggested: &str) -> Plan {
    let stored = app
        .state::<AppState>()
        .settings
        .browser_download_folder()
        .ok()
        .flatten();
    app.state::<BrowserState>()
        .downloads
        .lock()
        .ok()
        .map(|registry| folder::plan(stored.as_deref(), suggested, |path| registry.claims(path)))
        .unwrap_or_else(|| Plan::Ask {
            file_name: folder::sanitize_file_name(suggested),
            remember: false,
            directory: None,
        })
}

/// The first download's folder becomes the saved download folder, unless
/// Settings saved a usable one while the Save dialog was open.
pub(super) fn remember_choice(app: &AppHandle, chosen: &Path) {
    let settings = &app.state::<AppState>().settings;
    let saved = settings
        .browser_download_folder()
        .ok()
        .flatten()
        .as_deref()
        .and_then(folder::admit_folder);
    if saved.is_none()
        && let Some(folder) = folder::folder_of_choice(chosen)
    {
        let _ = remember_folder(settings, &folder);
    }
}

pub(super) fn accept_destination(
    registry: &mut DownloadRegistry,
    id: u64,
    path: PathBuf,
    armed_at: Instant,
    now: Instant,
) -> bool {
    if super::watchdog::deadline_reached(armed_at, now) {
        registry.settle(id, "canceled");
        return false;
    }
    registry.record_destination(id, path) && registry.destination_chosen(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::downloads::NavigationProvenance;
    use std::time::Duration;

    #[test]
    fn late_save_choice_cancels_without_a_destination() {
        let mut registry = DownloadRegistry::default();
        registry.set_provenance("browser-one", NavigationProvenance::User);
        let id = registry
            .begin("browser-one", "https://example.com/file", "file.bin")
            .unwrap();
        let path = std::env::temp_dir().join("late-download-choice.bin");
        let armed_at = Instant::now();
        assert!(!accept_destination(
            &mut registry,
            id,
            path,
            armed_at,
            armed_at + Duration::from_secs(5 * 60 + 1),
        ));
        assert_eq!(registry.views("browser-one")[0].state, "canceled");
        assert!(!registry.destinations.contains_key(&id));
    }

    #[test]
    fn save_choice_before_deadline_can_start_the_download() {
        let mut registry = DownloadRegistry::default();
        registry.set_provenance("browser-one", NavigationProvenance::User);
        let id = registry
            .begin("browser-one", "https://example.com/file", "file.bin")
            .unwrap();
        let path = std::env::temp_dir().join("timely-download-choice.bin");
        let armed_at = Instant::now();
        assert!(accept_destination(
            &mut registry,
            id,
            path.clone(),
            armed_at,
            armed_at + Duration::from_secs(5 * 60 - 1),
        ));
        assert_eq!(registry.views("browser-one")[0].state, "downloading");
        assert_eq!(registry.destinations.get(&id), Some(&path));
    }
}
