use serde_json::json;

use super::*;

const JWT: &str =
    "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

fn page() -> PageElement {
    PageElement {
        selector: "#checkout > :nth-child(2 of button)".into(),
        positional: "html > body > form > :nth-child(2 of button)".into(),
        text: "Place order\n£24.00".into(),
        sensitive: false,
        controls: false,
        title: "Cart".into(),
        rect: Rect {
            x: 412.4,
            y: 1880.6,
            width: 160.0,
            height: 40.2,
        },
        viewport: Viewport {
            x: 0.0,
            y: 1600.0,
            width: 1280.0,
            height: 720.0,
            client_width: 1265.0,
            client_height: 720.0,
            scale: 1.5,
        },
    }
}

fn picked() -> Picked {
    pick(
        &page(),
        "button",
        "Place order",
        "browser-12",
        "https://example.com/cart",
    )
    .unwrap()
}

fn element() -> BrowserElement {
    confirm(
        &picked(),
        3,
        "This should be disabled until terms are ticked",
    )
    .unwrap()
}

fn rejected(change: impl FnOnce(&mut BrowserElement)) -> String {
    let mut element = element();
    change(&mut element);
    validate(&element).unwrap_err()
}

#[test]
fn the_item_carries_exactly_the_contract_fields() {
    assert_eq!(
        serde_json::to_value(element()).unwrap(),
        json!({
            "kind": "browserElement",
            "target": "element",
            "ordinal": 3,
            "tabId": "browser-12",
            "url": "https://example.com/cart",
            "title": "Cart",
            "selector": "#checkout > :nth-child(2 of button)",
            "role": "button",
            "name": "Place order",
            "text": "Place order\n£24.00",
            "box": {"x": 412, "y": 1881, "width": 160, "height": 40},
            "comment": "This should be disabled until terms are ticked"
        })
    );
}

#[test]
fn picking_redacts_the_url_name_text_and_title() {
    let mut page = page();
    page.title = format!("Reset {JWT}");
    page.text = "key: hunter2-secret and https://user:pw@example.com/?token=abc".into();
    let picked = pick(
        &page,
        "textbox",
        "Authorization: Bearer name-secret",
        "browser-1",
        &format!("https://user:pw@example.com/reset/{JWT}?client_secret=s&q=boats"),
    )
    .unwrap();
    assert_eq!(
        picked.url,
        "https://example.com/reset/%5Bredacted%5D?client_secret=[redacted]&q=boats"
    );
    let all = format!("{picked:?}");
    for secret in ["eyJ", "hunter2", "name-secret", "user:pw", "token=abc"] {
        assert!(!all.contains(secret), "{secret} in {all}");
    }
    assert_eq!(picked.name, "Authorization: [redacted]");
}

#[test]
fn a_sensitive_field_or_a_name_that_may_hold_a_value_keeps_its_role_only() {
    let mut page = page();
    page.sensitive = true;
    let picked = pick(&page, "textbox", "Password", "browser-1", "https://e.com/").unwrap();
    assert_eq!(
        (picked.role.as_str(), picked.name.as_str()),
        ("textbox", "")
    );
    page.sensitive = false;
    page.controls = true;
    let picked = pick(
        &page,
        "button",
        "Ship to typed",
        "browser-1",
        "https://e.com/",
    )
    .unwrap();
    assert_eq!((picked.role.as_str(), picked.name.as_str()), ("button", ""));
}

#[test]
fn an_unusable_role_becomes_generic_and_controls_and_bidi_are_dropped() {
    let mut page = page();
    page.title = "Ca\u{202e}rt\n".into();
    page.text = "line\u{2066} one\nline\ttwo".into();
    let picked = pick(
        &page,
        "role with spaces",
        "Na\u{200f}me",
        "browser-1",
        "https://e.com/",
    )
    .unwrap();
    assert_eq!(picked.role, "generic");
    assert_eq!(picked.title, "Cart");
    assert_eq!(picked.name, "Name");
    assert_eq!(picked.text, "line one\nlinetwo");
    let long_role = "a".repeat(ROLE_BYTES + 1);
    let picked = pick(&page, &long_role, "", "browser-1", "https://e.com/").unwrap();
    assert_eq!(picked.role, "generic");
}

#[test]
fn long_text_is_cut_between_words_and_settles_under_redaction() {
    let mut page = page();
    page.text = format!("{} token=abcdefghij", "word ".repeat(203));
    let picked = pick(&page, "button", "", "browser-1", "https://e.com/").unwrap();
    assert!(picked.text.len() <= TEXT_BYTES);
    assert!(!picked.text.contains("abcdefghij"));
    assert_eq!(redact_console(&picked.text), picked.text);
    page.text = format!("{}token=abcdefghijklmnopqrstuvwxyz", "w ".repeat(505));
    let picked = pick(&page, "button", "", "browser-1", "https://e.com/").unwrap();
    assert!(picked.text.len() <= TEXT_BYTES);
    assert_eq!(redact_console(&picked.text), picked.text);
    assert!(!picked.text.contains("abc"));
}

#[test]
fn a_selector_that_redaction_would_change_falls_back_to_positions() {
    let mut page = page();
    page.selector = format!("#{JWT} > :nth-child(2 of button)");
    let picked = pick(&page, "button", "", "browser-1", "https://e.com/").unwrap();
    assert_eq!(picked.selector, page.positional);
}

#[test]
fn a_tag_with_a_pseudo_class_would_read_as_a_url_scheme_and_is_refused() {
    let mut page = page();
    page.selector = "#checkout > button:nth-of-type(2)".into();
    page.positional = "html > body > form > button:first-child".into();
    assert_eq!(
        pick(&page, "button", "", "browser-1", "https://e.com/"),
        Err("browser element selector unavailable".into())
    );
}

#[test]
fn a_long_selector_drops_its_outer_steps() {
    let mut page = page();
    page.selector = vec![":nth-child(12 of div)"; 40].join(" > ");
    let picked = pick(&page, "button", "", "browser-1", "https://e.com/").unwrap();
    assert!(picked.selector.len() <= SELECTOR_BYTES);
    assert!(picked.selector.starts_with(":nth-child(12 of div)"));
    assert!(page.selector.ends_with(&picked.selector));
}

#[test]
fn the_box_is_rounded_and_clamped_to_the_contract_ranges() {
    let mut page = page();
    page.rect = Rect {
        x: -4.0,
        y: 2e7,
        width: 0.2,
        height: 3e5,
    };
    let picked = pick(&page, "button", "", "browser-1", "https://e.com/").unwrap();
    assert_eq!(
        picked.bounds,
        ElementBox {
            x: 0,
            y: COORDINATE_MAX,
            width: 1,
            height: EXTENT_MAX
        }
    );
    page.rect.x = f64::NAN;
    assert!(pick(&page, "button", "", "browser-1", "https://e.com/").is_err());
}

#[test]
fn every_text_field_admits_its_limit_and_rejects_one_byte_more() {
    for (field, limit) in [
        ("title", TITLE_BYTES),
        ("selector", SELECTOR_BYTES),
        ("role", ROLE_BYTES),
        ("name", NAME_BYTES),
        ("text", TEXT_BYTES),
        ("comment", ATTACHED_CONTEXT_NOTE_BYTES),
    ] {
        let set = |element: &mut BrowserElement, value: String| match field {
            "title" => element.title = value,
            "selector" => element.selector = value,
            "role" => element.role = value,
            "name" => element.name = value,
            "text" => element.text = value,
            _ => element.comment = value,
        };
        let mut at_limit = element();
        at_limit.comment.clear();
        set(&mut at_limit, "a".repeat(limit));
        assert_eq!(validate(&at_limit), Ok(()), "{field} at its limit");
        set(&mut at_limit, "a".repeat(limit + 1));
        assert_eq!(
            validate(&at_limit),
            Err(format!("browser element {field} rejected"))
        );
    }
    let url = |length: usize| {
        let base = "https://example.com/";
        format!("{base}{}", "a/".repeat((length - base.len()) / 2))
    };
    let mut element = element();
    element.url = url(URL_BYTES);
    assert_eq!(element.url.len(), URL_BYTES);
    assert_eq!(validate(&element), Ok(()));
    element.url = url(URL_BYTES + 2);
    assert_eq!(
        validate(&element),
        Err("browser element url rejected".into())
    );
}

#[test]
fn ordinals_tab_ids_and_boxes_outside_the_contract_are_rejected() {
    for ordinal in [1, 8] {
        let mut element = element();
        element.ordinal = ordinal;
        assert_eq!(validate(&element), Ok(()));
    }
    for ordinal in [0, 9] {
        assert_eq!(
            rejected(|element| element.ordinal = ordinal),
            "browser element ordinal rejected"
        );
    }
    for tab in ["main", "browser-0", "browser-01", ""] {
        assert_eq!(
            rejected(|element| element.tab_id = tab.into()),
            "browser element tab rejected"
        );
    }
    let boxes = [
        (-1, 0, 1, 1),
        (0, COORDINATE_MAX + 1, 1, 1),
        (0, 0, 0, 1),
        (0, 0, 1, EXTENT_MAX + 1),
    ];
    for (x, y, width, height) in boxes {
        assert_eq!(
            rejected(|element| element.bounds = ElementBox {
                x,
                y,
                width,
                height
            }),
            "browser element box rejected"
        );
    }
    assert_eq!(
        rejected(|element| element.selector.clear()),
        "browser element selector rejected"
    );
}

#[test]
fn bidi_and_control_characters_are_rejected_and_only_the_text_may_break_lines() {
    for (field, bad) in [
        ("title", "a\u{202e}b"),
        ("title", "a\nb"),
        ("selector", "div\u{2066}"),
        ("selector", "div\n> p"),
        ("name", "a\u{200f}"),
        ("name", "a\u{0007}"),
        ("text", "a\u{2069}"),
        ("text", "a\tb"),
    ] {
        assert_eq!(
            rejected(|element| match field {
                "title" => element.title = bad.into(),
                "selector" => element.selector = bad.into(),
                "name" => element.name = bad.into(),
                _ => element.text = bad.into(),
            }),
            format!("browser element {field} rejected"),
            "{bad:?}"
        );
    }
    let mut element = element();
    element.text = "one\ntwo".into();
    assert_eq!(validate(&element), Ok(()));
}

#[test]
fn forged_unredacted_values_are_rejected() {
    for url in [
        "https://example.com/?token=abc",
        "https://user:pw@example.com/",
        "javascript:alert(1)",
        "file:///etc/passwd",
    ] {
        assert_eq!(
            rejected(|element| element.url = url.into()),
            "browser element url rejected",
            "{url}"
        );
    }
    assert_eq!(
        rejected(|element| element.url = format!("https://example.com/r/{JWT}")),
        "browser element url rejected"
    );
    assert_eq!(
        rejected(|element| element.title = format!("Reset {JWT}")),
        "browser element title rejected"
    );
    assert_eq!(
        rejected(|element| element.name = "Authorization: Bearer abc".into()),
        "browser element name rejected"
    );
    assert_eq!(
        rejected(|element| element.selector = "#password=hunter2".into()),
        "browser element selector rejected"
    );
    assert_eq!(
        rejected(|element| element.text = "key: sk-ant-abcdefghijklmnopqrstu".into()),
        "browser element text rejected"
    );
    assert_eq!(
        rejected(|element| element.role = "button\"".into()),
        "browser element role rejected"
    );
}

#[test]
fn a_whole_entry_over_eight_kib_is_rejected() {
    let mut element = element();
    element.text = "a".repeat(TEXT_BYTES);
    element.name = "b".repeat(NAME_BYTES);
    element.title = "c".repeat(TITLE_BYTES);
    element.selector = "d".repeat(SELECTOR_BYTES);
    element.comment = "e".repeat(ATTACHED_CONTEXT_NOTE_BYTES);
    assert_eq!(validate(&element), Ok(()));
    element.url = format!("https://example.com/{}", "f/".repeat(1000));
    assert_eq!(
        validate(&element),
        Err("browser element entry rejected".into())
    );
}

#[test]
fn viewports_outside_sane_ranges_are_refused() {
    assert!(page().viewport.admitted());
    for change in [
        |viewport: &mut Viewport| viewport.width = 0.0,
        |viewport: &mut Viewport| viewport.scale = f64::INFINITY,
        |viewport: &mut Viewport| viewport.scale = 41.0,
        |viewport: &mut Viewport| viewport.y = -1.0,
        |viewport: &mut Viewport| viewport.client_height = f64::NAN,
    ] {
        let mut viewport = page().viewport;
        change(&mut viewport);
        assert!(!viewport.admitted());
    }
}
