use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use tauri::{AppHandle, Manager};

use super::super::{
    BrowserState, DownloadRegistry,
    folder::{self, Plan},
};
use crate::{
    settings_commands::browser_download_folder::{emit_folder_changed, remember_first_choice},
    state::AppState,
};

/// A plan and the stored folder it was made against.
pub(super) struct Planned {
    pub(super) plan: Plan,
    pub(super) expected: Option<PathBuf>,
}

/// Silent save into the saved download folder, or the Save dialog. Runs off
/// the UI thread; the registry lock is held only to copy the claimed paths.
pub(super) fn plan_download(app: &AppHandle, suggested: &str, mime: Option<&str>) -> Planned {
    let Ok(stored) = app.state::<AppState>().settings.browser_download_folder() else {
        return Planned {
            plan: Plan::Ask {
                file_name: folder::sanitize_file_name(suggested),
                remember: false,
                directory: None,
            },
            expected: None,
        };
    };
    let claimed = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .map(|registry| registry.claimed())
        .unwrap_or_default();
    Planned {
        plan: folder::plan(stored.as_deref(), suggested, mime, |path| {
            claimed.contains(path)
        }),
        expected: stored,
    }
}

/// The first download's folder becomes the saved download folder, unless
/// Settings changed it while the Save dialog was open.
pub(super) fn remember_choice(app: &AppHandle, expected: Option<&Path>, chosen: &Path) {
    let settings = &app.state::<AppState>().settings;
    if remember_first_choice(settings, expected, chosen) {
        emit_folder_changed(app, settings);
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
