use super::*;

#[test]
fn diagnostics_never_contain_sensitive_input() {
    let canary = b"token=never-log-this";
    let mut ring = StderrRing::new(8, 4);
    ring.ingest(canary);
    let serialized = serde_json::to_string(&ring.safe_summary()).unwrap();
    assert!(!serialized.contains("never-log-this"));
    assert_eq!(ring.bytes.len(), 4);
    assert!(ring.safe_summary().truncated);

    let fingerprint =
        serde_json::to_string(&ExternalIdFingerprint::from_external_id(canary)).unwrap();
    assert!(!fingerprint.contains("token"));
}

#[test]
fn sensitive_bytes_have_normalizer_only_access() {
    let bytes = SensitiveBytes::new(b"secret".to_vec());
    assert_eq!(bytes.expose_to_normalizer().len(), 6);
}

#[test]
fn sensitive_bytes_redact_through_the_central_typed_boundary() {
    let canary = "secret.central-boundary.7e31";
    let diagnostic =
        SensitiveBytes::new(canary.as_bytes().to_vec()).redact(DiagnosticCode::IdentityMismatch);
    assert_eq!(diagnostic.code, DiagnosticCode::IdentityMismatch);
    assert_eq!(diagnostic.observed_bytes, canary.len() as u64);
    assert!(!diagnostic.truncated);
    assert!(!serde_json::to_string(&diagnostic).unwrap().contains(canary));
}

#[test]
fn stderr_ring_allocates_lazily_and_never_retains_over_its_limit() {
    let mut ring = StderrRing::new(8, 8);
    assert_eq!(ring.bytes.capacity(), 0);
    ring.ingest(b"123456789");
    assert!(ring.bytes.capacity() <= 8);
    assert_eq!(ring.bytes.len(), 8);
    assert!(ring.safe_summary().truncated);
}

#[test]
fn stderr_diagnostic_extracts_a_redacted_json_rpc_error() {
    let mut ring = StderrRing::new(512, 512);
    ring.ingest(
        br#"{"method":"thread/start","error":{"code":-32601,"message":"Bearer secret-token at C:\\private\\file"}}"#,
    );

    let diagnostic = ring.safe_diagnostic();

    assert_eq!(diagnostic.method.as_deref(), Some("thread/start"));
    assert_eq!(diagnostic.error_code.as_deref(), Some("-32601"));
    assert_eq!(
        diagnostic.error_message.as_deref(),
        Some("Bearer [redacted] at [path]")
    );
}

#[test]
fn stderr_diagnostic_redacts_unstructured_secret_text() {
    let mut ring = StderrRing::new(512, 512);
    ring.ingest(b"token=never-log-this");

    let diagnostic = ring.safe_diagnostic();

    assert_eq!(diagnostic.method, None);
    assert_eq!(diagnostic.error_code.as_deref(), Some("child-stderr"));
    assert_eq!(diagnostic.error_message.as_deref(), Some("[redacted]"));
}

#[test]
fn only_a_json_rpc_error_message_is_offered_for_storage() {
    let mut structured = StderrRing::new(512, 512);
    structured.ingest(br#"{"error":{"code":-32000,"message":"usage limit reached"}}"#);
    assert_eq!(
        structured.safe_diagnostic().json_rpc_error_message(),
        Some("usage limit reached")
    );

    let mut unstructured = StderrRing::new(512, 512);
    unstructured.ingest(b"panic: worker crashed at main.rs:12");
    let diagnostic = unstructured.safe_diagnostic();
    assert_eq!(diagnostic.json_rpc_error_message(), None);
}

#[test]
fn diagnostic_credentials_survive_whitespace_and_quoted_value_boundaries() {
    for (input, expected) in [
        ("Bearer  private-token", "Bearer  [redacted]"),
        ("Bearer\r\rprivate-token", "Bearer\r\r[redacted]"),
        ("--token\r\rprivate-token", "--token\r\r[redacted]"),
        (
            "Authorization: Basic dXNlcjpwYXNz",
            "Authorization: [redacted] [redacted]",
        ),
        ("Authorization:Basic dXNlcjpwYXNz", "[redacted] [redacted]"),
        (
            "Authorization: Digest username=\"private multi word\" denied",
            "Authorization: [redacted] [redacted] denied",
        ),
        (
            "aUtHoRiZaTiOn:  bAsIc  private-token",
            "aUtHoRiZaTiOn:  [redacted]  [redacted]",
        ),
        ("Authorization=Basic private-token", "[redacted] [redacted]"),
        ("Bearer:  private-token", "[redacted]  [redacted]"),
        (
            "Authorization :  private-token",
            "Authorization :  [redacted]",
        ),
        ("Bearer \t\n private-token", "Bearer \t\n [redacted]"),
        (
            "Bearer\u{2003}\u{2003}private-token",
            "Bearer\u{2003}\u{2003}[redacted]",
        ),
        (
            "Bearer \"private multi word\" denied",
            "Bearer [redacted] denied",
        ),
        (
            "Bearer 'private multi word' denied",
            "Bearer [redacted] denied",
        ),
        (
            "Authorization:  Bearer  private-token",
            "Authorization:  [redacted]  [redacted]",
        ),
        (
            "Authorization:  private-token",
            "Authorization:  [redacted]",
        ),
        (
            "Authorization:Bearer private-token",
            "[redacted] [redacted]",
        ),
        (
            "Authorization: \"Bearer private token\" denied",
            "Authorization: [redacted] denied",
        ),
        (
            "Authorization:\"Bearer private token\" denied",
            "[redacted] denied",
        ),
        ("Bearer:'private multi word' denied", "[redacted] denied"),
        (
            "--token  private-token denied",
            "--token  [redacted] denied",
        ),
        (
            "--password\t'private multi word' denied",
            "--password\t[redacted] denied",
        ),
        (
            "--secret \"private multi word\" denied",
            "--secret [redacted] denied",
        ),
        (
            "--api-key  private-token denied",
            "--api-key  [redacted] denied",
        ),
        ("--token=\"private multi word\" denied", "[redacted] denied"),
        ("KEY=\"private multi word\" denied", "[redacted] denied"),
        ("KEY='private multi word' denied", "[redacted] denied"),
        ("KEY=\" private multi word\" denied", "[redacted] denied"),
        ("KEY=' private multi word' denied", "[redacted] denied"),
        (
            "--token=\" private multi word\" denied",
            "[redacted] denied",
        ),
        ("--token=' private multi word' denied", "[redacted] denied"),
        (
            "KEY=  \"private multi word\" denied",
            "[redacted]  [redacted] denied",
        ),
        (
            "KEY = \"private multi word\" denied",
            "KEY = [redacted] denied",
        ),
        (
            r#"--secret "private \"multi\" word" denied"#,
            "--secret [redacted] denied",
        ),
        (
            "--secret \"private\nmulti word\" denied",
            "--secret [redacted] denied",
        ),
        (
            "--password 'private\r\nmulti word' denied",
            "--password [redacted] denied",
        ),
    ] {
        assert_eq!(sanitize_diagnostic_text(input), expected, "{input:?}");
        assert_eq!(
            sanitize_diagnostic_text(expected),
            expected,
            "idempotent: {input:?}"
        );
        let mut ring = StderrRing::new(512, 512);
        ring.ingest(input.as_bytes());
        assert_eq!(
            ring.safe_diagnostic().error_message.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn quoting_diagnostic_prose_cannot_hide_credential_markers() {
    for (input, expected) in [
        (r#""Bearer private-token""#, r#""Bearer [redacted]""#),
        (
            r#""failed Bearer private-token""#,
            r#""failed Bearer [redacted]""#,
        ),
        (
            r#""Authorization: private-token""#,
            r#""Authorization: [redacted]""#,
        ),
        (
            r#""failed --token private-token""#,
            r#""failed --token [redacted]""#,
        ),
        (
            r#""failed --password private-token""#,
            r#""failed --password [redacted]""#,
        ),
        (
            r#""failed --secret private-token""#,
            r#""failed --secret [redacted]""#,
        ),
        (
            r#""failed --api-key private-token""#,
            r#""failed --api-key [redacted]""#,
        ),
    ] {
        assert_eq!(sanitize_diagnostic_text(input), expected);
    }
    assert_eq!(
        sanitize_diagnostic_text("don't retry 'ordinary quoted prose'"),
        "don't retry 'ordinary quoted prose'"
    );
}

#[test]
fn diagnostic_quotes_preserve_prose_around_secrets_and_paths() {
    for (input, expected) in [
        (
            r#""message": "Incorrect API key provided: sk-abc. You can find help.""#,
            r#""message": "Incorrect API key provided: [redacted]. You can find help.""#,
        ),
        (
            r#""File not found: /home/u/x""#,
            r#""File not found: [path]""#,
        ),
        (
            r#""/home/u/x: permission denied; retry later""#,
            r#""[path]: permission denied; retry later""#,
        ),
        (
            r#""Unexpected sk-abc. Retry later"#,
            r#""Unexpected [redacted]. Retry later"#,
        ),
        (
            r#""File /home/u/x\" Retry later"#,
            r#""File [path]\" Retry later"#,
        ),
        (r"'C:\' Retry later", "'[path]' Retry later"),
        (r#""C:\" Retry later"#, r#""[path]" Retry later"#),
        (
            r#""Invalid KEY=private-token; retry later""#,
            r#""Invalid [redacted]; retry later""#,
        ),
        (
            r#""KEY='private multi word' denied""#,
            r#""[redacted] denied""#,
        ),
        (
            r#"Bearer "private-token retry later"#,
            "Bearer [redacted] retry later",
        ),
    ] {
        assert_eq!(sanitize_diagnostic_text(input), expected, "{input:?}");
        assert_eq!(
            sanitize_diagnostic_text(expected),
            expected,
            "idempotent: {input:?}"
        );
        let mut ring = StderrRing::new(512, 512);
        ring.ingest(input.as_bytes());
        assert_eq!(
            ring.safe_diagnostic().error_message.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn diagnostic_prose_requires_real_credential_syntax() {
    for input in [
        "Authorization failed for this request",
        "authorization denied; retry later",
        "left == right",
        "left==right",
        "left = right",
        r#""Authorization failed for this request""#,
        r#""Authorization:" failed for this request"#,
    ] {
        assert_eq!(sanitize_diagnostic_text(input), input);
    }
    assert_eq!(
        sanitize_diagnostic_text("KEY= value remains prose"),
        "[redacted] [redacted] remains prose"
    );
}
