use std::sync::Mutex;

use super::*;

#[test]
fn user_download_is_bounded_and_agent_download_is_denied_before_a_path_exists() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::Agent);
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/file", "file.txt")
        .unwrap();
    let view = &downloads.views("browser-1")[0];
    assert_eq!(view.id, id);
    assert_eq!(view.source, "https://example.com");
    assert_eq!(view.state, "choosingDestination");
    assert!(
        downloads
            .begin("browser-1", "file:///secret", "secret")
            .is_err()
    );
}

#[test]
fn download_cannot_enter_writing_state_without_a_chosen_destination() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/report", "report.pdf")
        .unwrap();
    assert!(!downloads.destination_chosen(id));
    assert_eq!(downloads.views("browser-1")[0].state, "choosingDestination");
}

#[test]
fn only_completed_downloads_reveal_the_user_chosen_containing_folder() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/report", "report.pdf")
        .unwrap();
    let folder = std::env::temp_dir().join("sage-bf2d");
    assert!(downloads.record_destination(id, folder.join("report.pdf")));
    assert!(downloads.destination_chosen(id));
    assert_eq!(downloads.folder_for_completed("browser-1", id), None);
    assert!(downloads.settle(id, "completed"));
    assert_eq!(downloads.folder_for_completed("browser-2", id), None);
    assert_eq!(
        downloads.folder_for_completed("browser-1", id),
        Some(folder)
    );
    assert!(
        !serde_json::to_string(&downloads.views("browser-1"))
            .unwrap()
            .contains("sage-bf2d")
    );
}

#[test]
fn progress_and_terminal_state_remain_bounded_and_cannot_restart() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/file", "file.txt")
        .unwrap();
    assert!(!downloads.progress(id, 4, Some(8)));
    assert!(downloads.record_destination(id, std::env::temp_dir().join("file.txt")));
    assert!(downloads.destination_chosen(id));
    assert!(downloads.progress(id, 4, Some(8)));
    assert_eq!(downloads.views("browser-1")[0].received_bytes, 4);
    assert!(downloads.settle(id, "completed"));
    assert!(!downloads.progress(id, 8, Some(8)));
    assert!(!downloads.settle(id, "completed"));
}

#[test]
fn active_capacity_does_not_evict_in_progress_downloads() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    for _ in 0..MAX_ACTIVE_DOWNLOADS {
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .unwrap();
    }
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    assert_eq!(downloads.views("browser-1").len(), MAX_ACTIVE_DOWNLOADS);
    assert!(downloads.settle(1, "canceled"));
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_ok()
    );
}

#[test]
fn active_download_ceiling_is_global_across_tabs() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    downloads.set_provenance("browser-2", NavigationProvenance::User);
    for _ in 0..MAX_ACTIVE_DOWNLOADS {
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .unwrap();
    }
    assert!(
        downloads
            .begin("browser-2", "https://example.com/file", "file.txt")
            .is_err()
    );
    assert!(downloads.views("browser-2").is_empty());
}

#[test]
fn late_picker_choice_cannot_restart_a_canceled_download() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/file", "file.txt")
        .unwrap();
    assert!(downloads.settle(id, "canceled"));
    assert!(!downloads.record_destination(id, std::env::temp_dir().join("file.txt")));
    assert!(!downloads.destination_chosen(id));
    assert_eq!(downloads.views("browser-1")[0].state, "canceled");
}

#[test]
fn closing_a_tab_cancels_its_active_downloads_and_revokes_provenance() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    downloads.set_provenance("browser-2", NavigationProvenance::User);
    let first = downloads
        .begin("browser-1", "https://a.example/file", "a.txt")
        .unwrap();
    let other = downloads
        .begin("browser-2", "https://b.example/file", "b.txt")
        .unwrap();
    assert_eq!(downloads.cancel_tab("browser-1"), vec![first]);
    assert_eq!(downloads.views("browser-1")[0].state, "canceled");
    assert_eq!(downloads.views("browser-2")[0].id, other);
    assert!(
        downloads
            .begin("browser-1", "https://a.example/file", "a.txt")
            .is_err()
    );
}

#[test]
fn cancellation_is_scoped_to_the_owning_tab_and_cannot_restart() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/file", "file.txt")
        .unwrap();
    assert!(!downloads.cancel("browser-2", id));
    assert_eq!(downloads.views("browser-1")[0].state, "choosingDestination");
    assert!(downloads.cancel("browser-1", id));
    assert!(!downloads.cancel("browser-1", id));
    assert_eq!(downloads.views("browser-1")[0].state, "canceled");
    assert!(!downloads.destination_chosen(id));
}

#[test]
fn failed_native_cancellation_keeps_download_visible_as_active() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/file", "file.txt")
        .unwrap();
    let downloads = Mutex::new(downloads);
    let result = cancel_after_native(&downloads, "browser-1", id, || {
        Err("native dispatch unavailable".into())
    });
    assert!(result.is_err());
    assert_eq!(
        downloads.lock().unwrap().views("browser-1")[0].state,
        "choosingDestination"
    );
}

#[test]
fn native_setup_error_releases_active_capacity() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    for _ in 0..MAX_ACTIVE_DOWNLOADS {
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .unwrap();
    }
    assert!(downloads.record_destination(1, std::env::temp_dir().join("file.txt")));
    assert!(downloads.destination_chosen(1));
    assert!(downloads.settle(1, "error"));
    assert_eq!(downloads.views("browser-1")[0].state, "error");
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_ok()
    );
}

#[test]
fn agent_provenance_clears_only_after_matching_user_navigation_completes() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::Agent);
    downloads.request_user_navigation("browser-1", "https://example.com/new");
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    downloads.start_user_navigation("browser-1", 42, "https://example.com/new");
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    downloads.finish_user_navigation("browser-1", 43, true);
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
    downloads.finish_user_navigation("browser-1", 42, true);
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_ok()
    );

    downloads.set_provenance("browser-1", NavigationProvenance::Agent);
    downloads.request_user_navigation("browser-1", "https://example.com/next");
    downloads.start_user_navigation("browser-1", 44, "https://example.com/next");
    downloads.set_provenance("browser-1", NavigationProvenance::Agent);
    downloads.finish_user_navigation("browser-1", 44, true);
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "file.txt")
            .is_err()
    );
}

#[test]
fn download_row_shows_the_name_the_file_was_saved_under() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin("browser-1", "https://example.com/report", "report.pdf")
        .unwrap();
    assert_eq!(downloads.views("browser-1")[0].file_name, "report.pdf");
    let folder = std::env::temp_dir().join("sage-saved-as");
    assert!(downloads.record_destination(id, folder.join("report (1).pdf")));
    assert_eq!(downloads.views("browser-1")[0].file_name, "report (1).pdf");
    assert!(downloads.destination_chosen(id));
    // WebView2's final ResultFilePath wins when it differs again.
    assert!(downloads.saved_as(id, &folder.join("report (2).pdf")));
    assert!(!downloads.saved_as(id, &folder.join("bad\u{7}name.pdf")));
    assert!(downloads.settle(id, "completed"));
    let view = &downloads.views("browser-1")[0];
    assert_eq!(view.file_name, "report (2).pdf");
    assert!(
        !serde_json::to_string(view)
            .unwrap()
            .contains("sage-saved-as")
    );
}

#[test]
fn download_names_drop_bidi_override_and_format_characters() {
    let mut downloads = DownloadRegistry::default();
    downloads.set_provenance("browser-1", NavigationProvenance::User);
    let id = downloads
        .begin(
            "browser-1",
            "https://example.com/file",
            "invoice\u{202e}fdp.exe",
        )
        .unwrap();
    assert_eq!(downloads.views("browser-1")[0].file_name, "invoicefdp.exe");
    let folder = std::env::temp_dir().join("sage-bidi");
    assert!(downloads.record_destination(id, folder.join("a\u{2066}b\u{200f}.txt")));
    assert_eq!(downloads.views("browser-1")[0].file_name, "ab.txt");
    assert!(
        downloads
            .begin("browser-1", "https://example.com/file", "\u{202e}\u{2069}")
            .is_err()
    );
}
