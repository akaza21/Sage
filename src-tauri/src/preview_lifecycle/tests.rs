use super::{on_committed_project_transition_epoch, stop_preview_children_before_epoch};

use std::sync::Mutex;

static NOTIFIED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

pub(super) fn note(new_epoch: u64) {
    NOTIFIED.lock().expect("preview epoch log").push(new_epoch);
}

fn take_notified() -> Vec<u64> {
    std::mem::take(&mut *NOTIFIED.lock().expect("preview epoch log"))
}

#[test]
fn committed_epoch_hook_stops_preview_children_for_that_epoch() {
    let _ = take_notified();
    on_committed_project_transition_epoch(7);
    assert_eq!(take_notified(), vec![7]);
    stop_preview_children_before_epoch(8);
}

#[test]
fn every_production_epoch_increment_calls_the_preview_cleanup_hook() {
    const SOURCES: &[(&str, &str)] = &[
        (
            "project_commands.rs",
            include_str!("../project_commands.rs"),
        ),
        ("chat_commands.rs", include_str!("../chat_commands.rs")),
        (
            "file_rename_commands.rs",
            include_str!("../file_rename_commands.rs"),
        ),
        (
            "resource_trash_commands.rs",
            include_str!("../resource_trash_commands.rs"),
        ),
    ];
    for (name, source) in SOURCES {
        let increments = increment_sites(source);
        assert!(
            !increments.is_empty(),
            "{name} must still contain a committed epoch increment"
        );
        for site in increments {
            let window = source.get(site..site.saturating_add(280)).unwrap_or("");
            assert!(
                window.contains("on_committed_project_transition_epoch"),
                "{name} increment at byte {site} must call on_committed_project_transition_epoch"
            );
        }
    }
}

fn increment_sites(source: &str) -> Vec<usize> {
    let mut sites = Vec::new();
    let mut rest = source;
    let mut offset = 0;
    while let Some(relative) = rest.find("*transition_epoch") {
        let start = offset + relative;
        let tail = &source[start..];
        if tail.starts_with("*transition_epoch += 1")
            || tail.starts_with("*transition_epoch = committed_epoch")
        {
            sites.push(start);
        }
        offset = start + 1;
        rest = &source[offset..];
    }
    sites
}
