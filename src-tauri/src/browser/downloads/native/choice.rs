use std::{path::PathBuf, time::Instant};

use super::super::DownloadRegistry;

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
