use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use tauri::{AppHandle, Manager};

use super::super::{
    BrowserState, DownloadRegistry,
    folder::{self, Plan},
    reservation::Reservation,
    worker,
};
use crate::{
    settings_commands::browser_download_folder::{emit_folder_changed, remember_first_choice},
    state::AppState,
};

/// A plan, the folder revision it was made against, and the placeholder a
/// silent plan reserved.
pub(super) struct Planned {
    pub(super) plan: Plan,
    pub(super) revision: i64,
    pub(super) reservation: Option<Reservation>,
}

/// Silent save into the saved download folder, or the Save dialog. Runs on
/// the download worker; the registry lock is held only to copy the claimed
/// paths.
pub(super) fn plan_download(app: &AppHandle, suggested: &str, mime: Option<&str>) -> Planned {
    let settings = app.state::<AppState>().settings.clone();
    let Ok(record) = settings.browser_download_folder_record() else {
        return Planned {
            plan: Plan::Ask {
                file_name: folder::sanitize_file_name(suggested),
                remember: false,
                directory: None,
            },
            revision: -1,
            reservation: None,
        };
    };
    let claimed = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .map(|registry| registry.claimed())
        .unwrap_or_default();
    let plan = folder::plan(record.path.as_deref(), suggested, mime, |path| {
        claimed.contains(path)
    });
    let reservation = match &plan {
        Plan::Silent(reserved) => Some(Reservation::adopt(settings, reserved.clone())),
        Plan::Ask { .. } => None,
    };
    Planned {
        plan,
        revision: record.revision,
        reservation,
    }
}

/// The first download's folder becomes the saved download folder, unless
/// the folder changed (Settings) since the download read it. The folder
/// checks and the store write run on the download worker.
pub(super) fn remember_choice(app: &AppHandle, revision: i64, chosen: &Path) {
    let app = app.clone();
    let chosen = chosen.to_owned();
    worker::submit(move || {
        let settings = &app.state::<AppState>().settings;
        if remember_first_choice(settings, revision, &chosen) {
            emit_folder_changed(&app, settings);
        }
    });
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
