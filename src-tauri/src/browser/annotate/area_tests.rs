use super::*;
use crate::browser::annotate::element::{self, Viewport};

fn viewport() -> Viewport {
    Viewport {
        x: 0.0,
        y: 100.0,
        width: 1280.0,
        height: 720.0,
        client_width: 1280.0,
        client_height: 720.0,
        scale: 1.0,
    }
}

#[test]
fn area_capture_bounds_elements_text_and_redacts_at_admission() {
    let node = AreaNode {
        role: "heading".into(),
        text: "Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz0123456789".into(),
    };
    let page = AreaPage {
        title: "Cart".into(),
        viewport: viewport(),
        nodes: vec![node; 64],
        masks: vec![],
    };
    let picked = pick(
        &page,
        Rect {
            x: 10.0,
            y: 120.0,
            width: 300.0,
            height: 200.0,
        },
        "browser-1",
        "https://example.com/?token=secret",
    )
    .unwrap();
    let item = element::confirm(&picked, 1, "Fix this").unwrap();
    assert!(item.text.len() <= 1024);
    assert!(!item.text.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(!item.url.contains("secret"));
    assert!(element::validate(&item).is_ok());
    let mut too_many = page;
    too_many.nodes.push(AreaNode {
        role: "button".into(),
        text: "Extra".into(),
    });
    assert!(
        pick(
            &too_many,
            Rect {
                x: 10.0,
                y: 120.0,
                width: 300.0,
                height: 200.0
            },
            "browser-1",
            "https://example.com/"
        )
        .is_err()
    );
}

#[test]
fn area_rejects_nonfinite_outside_or_zero_boxes_and_excess_masks() {
    let mut page = AreaPage {
        title: "Cart".into(),
        viewport: viewport(),
        nodes: vec![],
        masks: vec![],
    };
    for rect in [
        Rect {
            x: f64::NAN,
            y: 100.0,
            width: 20.0,
            height: 20.0,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        Rect {
            x: 0.0,
            y: 100.0,
            width: 0.0,
            height: 20.0,
        },
    ] {
        assert!(pick(&page, rect, "browser-1", "https://example.com/").is_err());
    }
    page.masks = vec![
        Rect {
            x: 0.0,
            y: 100.0,
            width: 10.0,
            height: 10.0
        };
        513
    ];
    assert!(
        pick(
            &page,
            Rect {
                x: 0.0,
                y: 100.0,
                width: 20.0,
                height: 20.0
            },
            "browser-1",
            "https://example.com/"
        )
        .is_err()
    );
}
