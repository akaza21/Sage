use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use tauri::{AppHandle, Manager};

use super::super::{
    BrowserState, DownloadRegistry,
    folder::{self, Plan},
    reservation::Reservation,
};
use crate::{
    settings_commands::browser_download_folder::{emit_folder_changed, remember_first_choice},
    settings_store::DownloadFolderRecord,
    state::AppState,
};

/// What to do with a download, decided on an isolated probe. A silent save's
/// placeholder is owned by its [`Reservation`] from the moment it exists.
pub(super) enum Step {
    Silent(Reservation),
    Ask {
        file_name: String,
        remember: bool,
        directory: Option<PathBuf>,
    },
}

/// A step and the folder revision it was decided against.
pub(super) struct Planned {
    pub(super) step: Step,
    pub(super) revision: i64,
}

/// A snapshot for isolated filesystem planning. The registry lock is held
/// only while copying claimed paths.
pub(super) struct Request {
    record: Option<DownloadFolderRecord>,
    claimed: folder::Claimed,
    suggested: String,
    mime: Option<String>,
}

impl Request {
    pub(super) fn new(
        record: Option<DownloadFolderRecord>,
        claimed: folder::Claimed,
        suggested: String,
        mime: Option<String>,
    ) -> Self {
        Self {
            record,
            claimed,
            suggested,
            mime,
        }
    }
    /// A deadline or saturation is treated like an unavailable folder, without
    /// giving the picker a path that may itself be unresponsive.
    pub(super) fn fallback(&self) -> Planned {
        let file_name = folder::sanitize_file_name(&self.suggested);
        let dangerous = folder::is_dangerous(&file_name)
            || self.mime.as_deref().is_some_and(folder::is_dangerous_mime);
        Planned {
            step: Step::Ask {
                file_name,
                remember: self
                    .record
                    .as_ref()
                    .is_some_and(|record| record.path.is_none() || !dangerous),
                directory: None,
            },
            revision: self.record.as_ref().map_or(-1, |record| record.revision),
        }
    }

    /// All folder I/O, including private staging, belongs to the isolated probe.
    pub(super) fn plan(self) -> Planned {
        let Some(record) = &self.record else {
            return self.fallback();
        };
        let step = match folder::plan(
            record.path.as_deref(),
            &self.suggested,
            self.mime.as_deref(),
            |path| self.claimed.contains(path),
        ) {
            Plan::Silent(placeholder) => {
                let mut reservation =
                    Reservation::adopt(placeholder, &folder::sanitize_file_name(&self.suggested));
                if !reservation.handoff() {
                    reservation.discard();
                    return self.fallback();
                }
                Step::Silent(reservation)
            }
            Plan::Ask {
                file_name,
                remember,
                directory,
            } => Step::Ask {
                file_name,
                remember,
                directory,
            },
        };
        Planned {
            step,
            revision: record.revision,
        }
    }
}

impl Planned {
    pub(super) fn discard(self) {
        if let Step::Silent(reservation) = self.step {
            reservation.discard();
        }
    }
}

/// Snapshot durable settings and claimed paths without any folder I/O.
pub(super) fn prepare_download(
    app: &AppHandle,
    suggested: String,
    mime: Option<String>,
) -> Request {
    let record = app
        .state::<AppState>()
        .settings
        .browser_download_folder_record()
        .ok();
    let claimed = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .map(|registry| registry.claimed())
        .unwrap_or_default();
    Request::new(record, claimed, suggested, mime)
}

/// The first download's folder becomes the saved download folder, unless
/// the folder changed (Settings) since the download read it. The folder
/// admission uses the bounded settings probe, away from publication.
pub(super) fn remember_choice(app: &AppHandle, revision: i64, chosen: &Path) {
    let app = app.clone();
    let chosen = chosen.to_owned();
    tauri::async_runtime::spawn(async move {
        let settings = app.state::<AppState>().settings.clone();
        if remember_first_choice(&settings, revision, &chosen).await {
            emit_folder_changed(&app, &settings);
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
