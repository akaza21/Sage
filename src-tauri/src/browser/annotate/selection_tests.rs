use serde_json::json;

use super::super::{
    Annotate, Exit,
    element::{PageElement, Rect},
    tests::{Overlay, off, on},
};
use super::*;

/// A selection as the native pick would make it.
pub(crate) fn selection(label: &str) -> Selection {
    let viewport = Viewport {
        x: 0.0,
        y: 100.0,
        width: 1280.0,
        height: 720.0,
        client_width: 1265.0,
        client_height: 720.0,
        scale: 1.0,
    };
    let page = PageElement {
        selector: "#press".into(),
        positional: "html > body:nth-of-type(1) > button:nth-of-type(1)".into(),
        text: "Press me".into(),
        sensitive: false,
        title: "Hostile page".into(),
        rect: Rect {
            x: 8.0,
            y: 140.0,
            width: 80.0,
            height: 24.0,
        },
        viewport,
    };
    Selection {
        picked: element::pick(&page, "button", "Press me", label, "https://example.com/").unwrap(),
        viewport,
        frame: Frame {
            jpeg_base64: "/9j/AAAA".into(),
            width: 1265,
            height: 720,
        },
    }
}

fn annotating(overlay: &Overlay, label: &str) -> AnnotateMode {
    let mut mode = AnnotateMode::default();
    mode.start(overlay, label).1.unwrap();
    mode
}

fn picked(overlay: &Overlay, label: &str) -> AnnotateMode {
    let mut mode = annotating(overlay, label);
    mode.pick(overlay, label, 7).1.unwrap();
    mode
}

#[test]
fn a_click_while_annotating_picks_the_element_and_announces_where_it_is() {
    let overlay = Overlay::default();
    let mut mode = annotating(&overlay, "browser-1");
    let (changes, result) = mode.pick(&overlay, "browser-1", 7);
    assert_eq!(result, Ok(()));
    assert_eq!(
        serde_json::to_value(&changes).unwrap(),
        json!([{
            "tabId": "browser-1",
            "active": true,
            "selection": {
                "role": "button",
                "name": "Press me",
                "box": {"x": 8, "y": 140, "width": 80, "height": 24},
                "viewport": {
                    "x": 0.0, "y": 100.0, "width": 1280.0, "height": 720.0,
                    "clientWidth": 1265.0, "clientHeight": 720.0, "scale": 1.0
                }
            }
        }])
    );
    assert_eq!(mode.pending, Some(selection("browser-1")));
    assert_eq!(overlay.calls(), ["start browser-1", "pick browser-1 7"]);
}

#[test]
fn clicks_on_other_tabs_after_leaving_or_while_an_element_waits_are_ignored() {
    let overlay = Overlay::default();
    let mut mode = AnnotateMode::default();
    assert_eq!(mode.pick(&overlay, "browser-1", 7), (vec![], Ok(())));
    let mut mode_on = picked(&overlay, "browser-1");
    assert_eq!(mode_on.pick(&overlay, "browser-1", 8), (vec![], Ok(())));
    assert_eq!(mode_on.pick(&overlay, "browser-2", 8), (vec![], Ok(())));
    mode.start(&overlay, "browser-3").1.unwrap();
    mode.leave(&overlay, "browser-3", Exit::Reveal).1.unwrap();
    assert_eq!(mode.pick(&overlay, "browser-3", 9), (vec![], Ok(())));
    let picks = overlay
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("pick"));
    assert_eq!(picks.collect::<Vec<_>>(), ["pick browser-1 7"]);
}

#[test]
fn a_failed_pick_keeps_annotate_on_and_says_why() {
    let mut overlay = Overlay::default();
    overlay.refuse_pick = true;
    let mut mode = annotating(&overlay, "browser-1");
    let (changes, result) = mode.pick(&overlay, "browser-1", 7);
    assert_eq!(result, Ok(()));
    let mut expected = on("browser-1");
    expected.error = Some("pick refused".into());
    assert_eq!(changes, [expected]);
    assert_eq!(mode.pending, None);
    assert_eq!(mode.tab.as_deref(), Some("browser-1"));
}

#[test]
fn cancelling_shows_the_page_resumes_hovering_and_clears_the_selection() {
    let overlay = Overlay::default();
    let mut mode = picked(&overlay, "browser-1");
    assert_eq!(
        mode.cancel(&overlay, "browser-1"),
        (vec![on("browser-1")], Ok(()))
    );
    assert_eq!(mode.pending, None);
    assert_eq!(
        overlay.calls().last().unwrap(),
        "release browser-1 reveal=true resume=true"
    );
    assert_eq!(mode.cancel(&overlay, "browser-1"), (vec![], Ok(())));
}

#[test]
fn confirming_hands_out_the_numbered_commented_item_and_resumes() {
    let overlay = Overlay::default();
    let mut mode = picked(&overlay, "browser-1");
    let (changes, item) = mode.confirm(&overlay, "browser-1", 2, "Make this bigger");
    let item = item.unwrap();
    assert_eq!(
        (item.ordinal, item.comment.as_str()),
        (2, "Make this bigger")
    );
    assert_eq!(item.selector, "#press");
    assert_eq!(changes, [on("browser-1")]);
    assert_eq!(mode.pending, None);
    assert_eq!(
        overlay.calls().last().unwrap(),
        "release browser-1 reveal=true resume=true"
    );
}

#[test]
fn an_item_that_fails_the_contract_keeps_the_element_waiting() {
    let overlay = Overlay::default();
    let mut mode = picked(&overlay, "browser-1");
    let long = "a".repeat(4097);
    for (ordinal, comment, error) in [
        (1, long.as_str(), "browser element comment rejected"),
        (9, "", "browser element ordinal rejected"),
        (0, "", "browser element ordinal rejected"),
    ] {
        assert_eq!(
            mode.confirm(&overlay, "browser-1", ordinal, comment),
            (vec![], Err(error.into()))
        );
    }
    assert!(mode.pending.is_some());
    assert!(
        !overlay
            .calls()
            .iter()
            .any(|call| call.starts_with("release"))
    );
    assert_eq!(
        mode.confirm(&overlay, "browser-2", 1, "").1,
        Err("no browser element is waiting".into())
    );
}

#[test]
fn a_page_that_fails_to_come_back_still_hands_out_the_item_and_says_why() {
    let mut overlay = Overlay::default();
    let mut mode = picked(&overlay, "browser-1");
    overlay.refuse_release = true;
    let (changes, item) = mode.confirm(&overlay, "browser-1", 1, "");
    assert!(item.is_ok());
    assert_eq!(changes[0].error.as_deref(), Some("release refused"));
    assert_eq!(mode.pending, None);
}

#[test]
fn the_frame_is_served_only_for_the_waiting_element() {
    let overlay = Overlay::default();
    let mut mode = annotating(&overlay, "browser-1");
    assert!(mode.frame("browser-1").is_err());
    mode.pick(&overlay, "browser-1", 7).1.unwrap();
    assert_eq!(mode.frame("browser-1").unwrap().jpeg_base64, "/9j/AAAA");
    assert!(mode.frame("browser-2").is_err());
}

#[test]
fn leaving_with_an_element_waiting_shows_the_page_unless_the_tab_hid_or_closed() {
    for (exit, calls) in [
        (
            Exit::Reveal,
            vec![
                "release browser-1 reveal=true resume=false",
                "stop browser-1",
            ],
        ),
        (
            Exit::Hidden,
            vec![
                "release browser-1 reveal=false resume=false",
                "stop browser-1",
            ],
        ),
        (Exit::Closed, vec![]),
    ] {
        let overlay = Overlay::default();
        let mut mode = picked(&overlay, "browser-1");
        assert_eq!(
            mode.leave(&overlay, "browser-1", exit),
            (vec![off("browser-1")], Ok(()))
        );
        assert_eq!(mode.pending, None);
        assert_eq!(overlay.calls()[2..], calls, "{exit:?}");
    }
}

#[test]
fn starting_another_tab_first_shows_the_frozen_page_of_the_previous_one() {
    let overlay = Overlay::default();
    let mut mode = picked(&overlay, "browser-1");
    assert_eq!(
        mode.start(&overlay, "browser-2"),
        (vec![off("browser-1"), on("browser-2")], Ok(()))
    );
    assert_eq!(
        overlay.calls()[2..],
        [
            "release browser-1 reveal=true resume=false",
            "stop browser-1",
            "start browser-2"
        ]
    );
    assert_eq!(mode.pending, None);
}

#[test]
fn the_lock_free_frozen_copy_follows_the_waiting_element() {
    let annotate = Annotate::default();
    let overlay = Overlay::default();
    annotate
        .run(|_| (), |mode| mode.start(&overlay, "browser-4"))
        .unwrap();
    assert!(!annotate.is_frozen("browser-4"));
    annotate
        .run(|_| (), |mode| mode.pick(&overlay, "browser-4", 1))
        .unwrap();
    assert!(annotate.is_frozen("browser-4"));
    assert!(!annotate.is_frozen("browser-1"));
    annotate
        .run(|_| (), |mode| mode.cancel(&overlay, "browser-4"))
        .unwrap();
    assert!(!annotate.is_frozen("browser-4"));
    annotate
        .run(|_| (), |mode| mode.pick(&overlay, "browser-4", 1))
        .unwrap();
    annotate
        .run(
            |_| (),
            |mode| mode.leave(&overlay, "browser-4", Exit::Hidden),
        )
        .unwrap();
    assert!(!annotate.is_frozen("browser-4"));
}
