use std::{cell::RefCell, collections::BTreeSet, sync::Mutex};

use super::*;

#[derive(Default)]
pub(super) struct Overlay {
    calls: RefCell<Vec<String>>,
    /// Tabs with a click listener: start adds one and every stop removes it.
    pub(super) watching: RefCell<BTreeSet<String>>,
    /// Asked when the page hides: whether it was already marked frozen.
    pub(super) probe: Option<Box<dyn Fn() -> bool>>,
    refuse_start: bool,
    refuse_stop: bool,
    pub(super) refuse_pick: bool,
    pub(super) refuse_release: bool,
}

impl InspectOverlay for Overlay {
    fn start(&self, label: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("start {label}"));
        if self.refuse_start {
            return Err("start refused".into());
        }
        self.watching.borrow_mut().insert(label.to_owned());
        Ok(())
    }
    fn stop(&self, label: &str, closing: bool) -> Result<(), String> {
        let closing = if closing { " closing" } else { "" };
        self.calls
            .borrow_mut()
            .push(format!("stop {label}{closing}"));
        self.watching.borrow_mut().remove(label);
        if self.refuse_stop {
            return Err("stop refused".into());
        }
        Ok(())
    }
    fn pick(
        &self,
        label: &str,
        backend_node_id: u64,
        freeze: &dyn Fn(),
    ) -> Result<Selection, String> {
        self.calls
            .borrow_mut()
            .push(format!("pick {label} {backend_node_id}"));
        freeze();
        if let Some(probe) = &self.probe {
            self.calls
                .borrow_mut()
                .push(format!("hide frozen={}", probe()));
        }
        if self.refuse_pick {
            return Err("pick refused".into());
        }
        Ok(selection::tests::selection(label))
    }
    fn release(&self, label: &str, reveal: bool, resume: bool) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("release {label} reveal={reveal} resume={resume}"));
        if self.refuse_release {
            return Err("release refused".into());
        }
        Ok(())
    }
    fn focus(&self, label: &str, page: bool) -> Result<(), String> {
        let target = if page { label } else { "main" };
        self.calls.borrow_mut().push(format!("focus {target}"));
        Ok(())
    }
}

impl Overlay {
    pub(super) fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

pub(super) fn on(label: &str) -> AnnotateChange {
    change(label.into(), true)
}

pub(super) fn off(label: &str) -> AnnotateChange {
    change(label.into(), false)
}

#[test]
fn only_the_annotate_actions_are_recognised() {
    assert_eq!(action("annotateOn"), Some(Action::On));
    assert_eq!(action("annotateOff"), Some(Action::Off));
    assert_eq!(action("annotateCancel"), Some(Action::Cancel));
    assert_eq!(action("annotateConfirm"), Some(Action::Confirm));
    assert_eq!(action("annotateFrame"), Some(Action::Frame));
    for other in ["annotate", "AnnotateOn", "annotateSelect", "reload", ""] {
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
fn an_agent_marker_clear_never_disables_an_overlay_that_annotate_is_starting_or_using() {
    let annotate = Annotate::default();
    let overlay = Overlay::default();
    let disabled = RefCell::new(Vec::new());
    let disable = |label: &str| {
        annotate.disable_unless_annotating(label, || disabled.borrow_mut().push(label.to_owned()))
    };
    // A clear that lands while Annotate's start holds the mode lock, after
    // its enables ran but before it records the tab, skips the disables.
    let raced = annotate.run(
        |_| {},
        |mode| {
            let (changes, started) = mode.start(&overlay, "browser-1");
            (changes, started.map(|()| disable("browser-1")))
        },
    );
    assert_eq!(raced, Ok(false));
    assert!(
        !disable("browser-1"),
        "the annotating tab keeps its overlay"
    );
    assert!(disable("browser-2"));
    assert_eq!(*disabled.borrow(), ["browser-2"]);
}

#[test]
fn start_turns_the_overlay_on_and_starting_again_restarts_and_reannounces_it() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    assert_eq!(
        mode.start(&overlay, "browser-1"),
        (vec![on("browser-1")], Ok(()))
    );
    // A remounted chrome that shows Annotate off asks again and learns it is on.
    assert_eq!(
        mode.start(&overlay, "browser-1"),
        (vec![on("browser-1")], Ok(()))
    );
    assert_eq!(overlay.calls(), ["start browser-1", "start browser-1"]);
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
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![], Ok(()))
    );
    assert_eq!(
        mode.leave(&overlay, "browser-2", Exit::Reveal),
        (vec![off("browser-2")], Ok(()))
    );
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
    assert_eq!(
        mode.leave(&overlay, "browser-2", Exit::Reveal),
        (vec![], Ok(()))
    );
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![], Ok(()))
    );
}

#[test]
fn leaving_stops_the_overlay_once_and_ignores_tabs_that_are_not_annotating() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![], Ok(()))
    );
    mode.start(&overlay, "browser-1").1.unwrap();
    assert_eq!(
        mode.leave(&overlay, "browser-2", Exit::Reveal),
        (vec![], Ok(()))
    );
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![off("browser-1")], Ok(()))
    );
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![], Ok(()))
    );
    assert_eq!(overlay.calls(), ["start browser-1", "stop browser-1"]);
}

#[test]
fn a_failed_stop_still_ends_the_mode_and_a_closing_tab_only_lets_go_of_its_listener() {
    let mut overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    mode.start(&overlay, "browser-1").1.unwrap();
    overlay.refuse_stop = true;
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![off("browser-1")], Err("stop refused".into()))
    );
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Reveal),
        (vec![], Ok(()))
    );

    overlay.refuse_stop = false;
    mode.start(&overlay, "browser-3").1.unwrap();
    assert_eq!(
        mode.leave(&overlay, "browser-3", Exit::Closed),
        (vec![off("browser-3")], Ok(()))
    );
    assert_eq!(
        overlay.calls(),
        [
            "start browser-1",
            "stop browser-1",
            "start browser-3",
            "stop browser-3 closing"
        ]
    );
    assert!(overlay.watching.borrow().is_empty());
}

#[test]
fn closing_an_annotating_tab_leaves_no_click_listener_behind() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    mode.start(&overlay, "browser-1").1.unwrap();
    mode.pick(&overlay, "browser-1", 7, &|| ()).1.unwrap();
    assert_eq!(
        *overlay.watching.borrow(),
        BTreeSet::from(["browser-1".into()])
    );
    assert_eq!(
        mode.leave(&overlay, "browser-1", Exit::Closed),
        (vec![off("browser-1")], Ok(()))
    );
    assert!(overlay.watching.borrow().is_empty());
    assert_eq!(overlay.calls().last().unwrap(), "stop browser-1 closing");
    assert!(
        !overlay
            .calls()
            .iter()
            .any(|call| call.starts_with("release"))
    );
}

#[test]
fn changes_are_announced_before_the_mode_lock_is_released() {
    let annotate = Annotate::default();
    let overlay = Overlay::default();
    let announced = RefCell::new(Vec::new());
    let announce = |change: AnnotateChange| {
        assert!(
            annotate.mode.try_lock().is_err(),
            "announced after the lock was released"
        );
        announced.borrow_mut().push(change);
    };
    annotate
        .run(announce, |mode| mode.start(&overlay, "browser-1"))
        .unwrap();
    annotate
        .run(announce, |mode| mode.start(&overlay, "browser-2"))
        .unwrap();
    annotate
        .run(announce, |mode| {
            mode.leave(&overlay, "browser-2", Exit::Reveal)
        })
        .unwrap();
    assert_eq!(
        announced.into_inner(),
        [
            on("browser-1"),
            off("browser-1"),
            on("browser-2"),
            off("browser-2")
        ]
    );
}

#[test]
fn racing_starts_and_leaves_announce_the_final_state_last() {
    struct Slow;
    impl InspectOverlay for Slow {
        fn start(&self, _: &str) -> Result<(), String> {
            std::thread::yield_now();
            Ok(())
        }
        fn stop(&self, _: &str, _: bool) -> Result<(), String> {
            std::thread::yield_now();
            Ok(())
        }
        fn pick(&self, label: &str, _: u64, _: &dyn Fn()) -> Result<Selection, String> {
            Ok(selection::tests::selection(label))
        }
        fn release(&self, _: &str, _: bool, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn focus(&self, _: &str, _: bool) -> Result<(), String> {
            Ok(())
        }
    }
    let annotate = Annotate::default();
    let announced = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for on in [true, false, true, false] {
            let (annotate, announced) = (&annotate, &announced);
            scope.spawn(move || {
                for _ in 0..200 {
                    let _ = annotate.run(
                        |change| announced.lock().unwrap().push(change.active),
                        |mode| {
                            if on {
                                mode.start(&Slow, "browser-1")
                            } else {
                                mode.leave(&Slow, "browser-1", Exit::Reveal)
                            }
                        },
                    );
                }
            });
        }
    });
    let announced = announced.into_inner().unwrap();
    assert_eq!(
        announced.last().copied(),
        Some(annotate.is_annotating("browser-1"))
    );
    // Every "off" follows an "on": no stale "off" overtakes a newer "on".
    let mut on = false;
    for active in announced {
        assert!(active || on, "off announced while already off");
        on = active;
    }
}

#[test]
fn the_lock_free_tab_copy_follows_the_mode() {
    let annotate = Annotate::default();
    let overlay = Overlay::default();
    assert!(!annotate.is_annotating("browser-1"));
    assert!(!annotate.is_annotating("browser-0"));
    annotate
        .run(|_| (), |mode| mode.start(&overlay, "browser-12"))
        .unwrap();
    assert!(annotate.is_annotating("browser-12"));
    assert!(!annotate.is_annotating("browser-1"));
    assert!(!annotate.is_annotating("main"));
    annotate
        .run(
            |_| (),
            |mode| mode.leave(&overlay, "browser-12", Exit::Closed),
        )
        .unwrap();
    assert!(!annotate.is_annotating("browser-12"));
}

#[test]
fn the_chrome_is_told_why_annotate_is_unavailable() {
    assert_eq!(UNAVAILABLE_REASON.is_none(), cfg!(windows));
    let mut tabs = BrowserTabs::<()>::new();
    tabs.insert("browser-1".into(), (), false);
    let reason = |tabs: &BrowserTabs<()>| unavailable_reason(&tabs.snapshot("browser-1").unwrap());
    let no_page = UNAVAILABLE_REASON.or(Some("Open a page to annotate"));
    assert_eq!(reason(&tabs), no_page, "a new tab has no page");
    assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
    assert_eq!(reason(&tabs), no_page, "a loading tab");
    assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
    assert_eq!(reason(&tabs), UNAVAILABLE_REASON, "a shown page");
}
