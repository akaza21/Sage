use std::cell::Cell;

use super::PageVisibility;
use crate::browser::annotate::{Annotate, Exit};

fn interleave_hide(cover: bool) {
    let visibility = PageVisibility::default();
    let annotate = Annotate::default();
    visibility.place();
    annotate.placed("browser-1");
    let sampled_cover = annotate.is_covered("browser-1");
    assert!(!sampled_cover);

    // The main-thread hide lands after zoom's sample, before its release.
    visibility.hide();
    assert_eq!(
        annotate.hiding("browser-1", cover),
        if cover { None } else { Some(Exit::Hidden) }
    );
    let shown = Cell::new(false);
    let focused = Cell::new(false);
    if !sampled_cover {
        visibility
            .reveal(|| {
                shown.set(true);
                Ok(())
            })
            .unwrap();
        visibility
            .reveal(|| {
                focused.set(true);
                Ok(())
            })
            .unwrap();
    }
    assert!(!shown.get(), "a stale zoom sample must not reveal the page");
    assert!(!focused.get(), "a hidden page must not take focus");

    // Only the next placement makes this webview eligible again.
    visibility.place();
    visibility
        .reveal(|| {
            shown.set(true);
            Ok(())
        })
        .unwrap();
    assert!(shown.get());
}

#[test]
fn a_real_hide_between_zoom_sample_and_release_keeps_the_page_hidden() {
    interleave_hide(false);
}

#[test]
fn a_cover_between_zoom_sample_and_release_keeps_the_page_hidden() {
    interleave_hide(true);
}

#[test]
fn an_unplaced_webview_cannot_be_revealed_by_a_release() {
    let visibility = PageVisibility::default();
    visibility.reveal(|| panic!("unplaced page shown")).unwrap();
}
