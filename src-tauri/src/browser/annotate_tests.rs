use std::cell::RefCell;

use super::*;

#[derive(Default)]
struct Overlay {
    calls: RefCell<Vec<String>>,
    refuse_start: bool,
    refuse_stop: bool,
}

impl InspectOverlay for Overlay {
    fn start(&self, label: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("start {label}"));
        if self.refuse_start {
            return Err("start refused".into());
        }
        Ok(())
    }
    fn stop(&self, label: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("stop {label}"));
        if self.refuse_stop {
            return Err("stop refused".into());
        }
        Ok(())
    }
}

impl Overlay {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

fn on(label: &str) -> AnnotateChange {
    change(label.into(), true)
}

fn off(label: &str) -> AnnotateChange {
    change(label.into(), false)
}

#[test]
fn only_the_two_annotate_actions_are_recognised() {
    assert_eq!(action("annotateOn"), Some(true));
    assert_eq!(action("annotateOff"), Some(false));
    for other in ["annotate", "AnnotateOn", "reload", ""] {
        assert_eq!(action(other), None, "{other}");
    }
}

#[test]
fn annotate_starts_only_on_a_known_tab_showing_a_committed_page() {
    let mut tabs = BrowserTabs::<&str>::new();
    tabs.insert("browser-1".into(), "webview-1", true);
    assert_eq!(
        target(&tabs, "browser-1"),
        Err("browser page is still loading".into())
    );
    assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
    assert_eq!(target(&tabs, "browser-1"), Ok("webview-1"));
    assert_eq!(
        target(&tabs, "browser-2"),
        Err("browser tab unavailable".into())
    );
    for invalid in ["main", "browser-01", "browser-0", "browser-1/../x"] {
        assert_eq!(
            target(&tabs, invalid),
            Err("invalid browser tab id".into()),
            "{invalid}"
        );
    }
    assert!(tabs.start_navigation("browser-1", 2, "https://example.com/next"));
    assert_eq!(
        target(&tabs, "browser-1"),
        Err("browser page is still loading".into())
    );
}

#[test]
fn start_turns_the_overlay_on_once_and_announces_it() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    assert_eq!(
        mode.start(&overlay, "browser-1"),
        (vec![on("browser-1")], Ok(()))
    );
    assert_eq!(mode.start(&overlay, "browser-1"), (vec![], Ok(())));
    assert_eq!(overlay.calls(), ["start browser-1"]);
}

#[test]
fn starting_on_another_tab_first_leaves_the_previous_one() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    mode.start(&overlay, "browser-1").1.unwrap();
    assert_eq!(
        mode.start(&overlay, "browser-2"),
        (vec![off("browser-1"), on("browser-2")], Ok(()))
    );
    assert_eq!(
        overlay.calls(),
        ["start browser-1", "stop browser-1", "start browser-2"]
    );
    assert_eq!(mode.leave(&overlay, "browser-1", true), None);
    assert_eq!(mode.leave(&overlay, "browser-2", true), Some(Ok(())));
}

#[test]
fn a_refused_start_leaves_annotate_off_but_still_announces_the_previous_tab() {
    let mut overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    mode.start(&overlay, "browser-1").1.unwrap();
    overlay.refuse_start = true;
    assert_eq!(
        mode.start(&overlay, "browser-2"),
        (vec![off("browser-1")], Err("start refused".into()))
    );
    assert_eq!(mode.leave(&overlay, "browser-2", true), None);
    assert_eq!(mode.leave(&overlay, "browser-1", true), None);
}

#[test]
fn leaving_stops_the_overlay_once_and_ignores_tabs_that_are_not_annotating() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    assert_eq!(mode.leave(&overlay, "browser-1", true), None);
    mode.start(&overlay, "browser-1").1.unwrap();
    assert_eq!(mode.leave(&overlay, "browser-2", true), None);
    assert_eq!(mode.leave(&overlay, "browser-1", true), Some(Ok(())));
    assert_eq!(mode.leave(&overlay, "browser-1", true), None);
    assert_eq!(overlay.calls(), ["start browser-1", "stop browser-1"]);
}

#[test]
fn a_failed_stop_still_ends_the_mode_and_a_closing_tab_skips_the_overlay() {
    let mut overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    mode.start(&overlay, "browser-1").1.unwrap();
    overlay.refuse_stop = true;
    assert_eq!(
        mode.leave(&overlay, "browser-1", true),
        Some(Err("stop refused".into()))
    );
    assert_eq!(mode.leave(&overlay, "browser-1", true), None);

    overlay.refuse_stop = false;
    mode.start(&overlay, "browser-3").1.unwrap();
    assert_eq!(mode.leave(&overlay, "browser-3", false), Some(Ok(())));
    assert_eq!(
        overlay.calls(),
        ["start browser-1", "stop browser-1", "start browser-3"]
    );
}

#[test]
fn annotate_is_reachable_only_through_the_guarded_browser_control_command() {
    let source = include_str!("../ipc_commands.rs");
    let (guard, handlers) = source
        .split_once("tauri::generate_handler![")
        .expect("guarded command list");
    assert!(guard.contains("browser::ipc_guard::admits_main_invoke(webview.label(), &url)"));
    assert!(handlers.contains("browser::navigation::control::browser_control,"));
    assert!(!source.contains("annotate"), "no separate Annotate command");
}

#[test]
fn the_platform_reason_is_given_exactly_where_there_is_no_inspect_overlay() {
    assert_eq!(UNAVAILABLE_REASON.is_none(), cfg!(windows));
}
