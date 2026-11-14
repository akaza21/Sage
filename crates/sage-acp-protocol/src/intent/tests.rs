use super::{ErrorKind, NormalizedPayload};

#[test]
fn durable_error_rows_decode_future_kinds_as_none() {
    for (kind, expected) in [
        (Some("attempt"), Some(ErrorKind::Attempt)),
        (Some("futureErrorKind"), None),
        (Some(""), None),
        (None, None),
    ] {
        let mut row = serde_json::json!({
            "kind": "error",
            "payload": {"code": "retry", "message": "safe diagnostic", "retryable": true}
        });
        row["payload"]["kind"] = serde_json::json!(kind);
        let decoded: NormalizedPayload = serde_json::from_value(row).expect("durable error row");
        assert!(matches!(&decoded, NormalizedPayload::Error { kind, .. } if *kind == expected));
        let encoded = serde_json::to_value(decoded).expect("encode durable error row");
        assert_eq!(
            encoded["payload"].get("kind"),
            expected.map(|_| serde_json::json!("attempt")).as_ref()
        );
    }
    let legacy: NormalizedPayload = serde_json::from_str(
        r#"{"kind":"error","payload":{"code":"retry","message":"safe diagnostic","retryable":true}}"#,
    ).expect("legacy row without kind");
    assert!(matches!(
        legacy,
        NormalizedPayload::Error { kind: None, .. }
    ));
}
